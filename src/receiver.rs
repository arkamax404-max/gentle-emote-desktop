//! One owned network thread publishes complete images and bounded session snapshots.
//!
//! HTTP parsing belongs to Hyper, not this module. The small policy layer below
//! validates the request before decoding, then publishes only a fully valid PNG.
use crate::sessions::{ClockTile, Event, SessionRegistry, SessionSnapshot, SessionState};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    io::{self, Cursor},
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinSet, time::timeout};

pub const PRODUCTION_ADDRESS: &str = "127.0.0.1:8001";
pub const MAX_BODY_BYTES: usize = 65_536;
pub const BODY_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_CONNECTIONS: usize = 8;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const HEADER_BYTES: usize = 8192;
const DECODE_BYTES: usize = 256 * 1024;
const RGBA_BYTES: usize = 64 * 64 * 4;

/// Always 64x64, row-major, unpremultiplied RGBA8 (including the PNG's alpha).
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub rgba: Vec<u8>,
}

/// Cloning this handle does not clone the pixels. `get` takes only a short lock.
/// There is no pending-frame queue: an unread frame is simply replaced.
#[derive(Clone, Default)]
pub struct LatestFrame(Arc<Mutex<SessionRegistry>>);
impl LatestFrame {
    pub fn get(&self) -> Option<Arc<Frame>> {
        self.0.lock().expect("frame lock poisoned").latest()
    }

    /// Queue a focus request for a currently active session, never a legacy tile.
    pub fn request_focus(&self, session: &str) -> Option<String> {
        self.0
            .lock()
            .expect("frame lock poisoned")
            .request_focus(session, Instant::now())
    }

    /// Read presentation state atomically, without holding the lock during rendering.
    /// The only name-bearing accessor: titles, silence and `show_clock` come from one
    /// observation, so a caller cannot read a clock title that skips the silence rule.
    pub fn display_snapshot(&self) -> (Vec<ClockTile>, bool) {
        self.display_snapshot_at(Instant::now())
    }

    /// Same observation at a supplied instant, so the silence rule is testable
    /// without sleeping. Production reads the monotonic clock once per frame.
    pub(crate) fn display_snapshot_at(&self, now: Instant) -> (Vec<ClockTile>, bool) {
        let registry = self.0.lock().expect("frame lock poisoned");
        (registry.clock_tiles_at(now), registry.show_clock())
    }

    /// Active avatars in stable first-seen order, or the retained final avatar.
    /// Pixel Arcs remain valid across publication, expiry, and shutdown.
    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        self.0.lock().expect("frame lock poisoned").snapshots()
    }
}

/// Owns the listening socket, worker runtime, and every active connection.
/// Dropping it signals cancellation and joins the thread, just like `shutdown`.
pub struct Receiver {
    address: SocketAddr,
    latest: LatestFrame,
    stop: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}
impl Receiver {
    /// Production entry point. Bind errors (including port in use) reach the GUI.
    pub fn start(repaint: impl Fn() + Send + Sync + 'static) -> io::Result<Self> {
        Self::bind(
            PRODUCTION_ADDRESS.parse().expect("constant address"),
            repaint,
        )
    }

    /// Also supports ephemeral test ports, but never a non-127.0.0.1 address.
    /// `repaint` runs after publication, outside the frame lock, on the worker.
    /// It must be quick, nonblocking, and nonpanicking (e.g. egui request_repaint).
    pub fn bind(
        address: SocketAddr,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Self> {
        if address.ip() != Ipv4Addr::LOCALHOST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only 127.0.0.1 is allowed",
            ));
        }
        let socket = std::net::TcpListener::bind(address)?;
        let address = socket.local_addr()?;
        socket.set_nonblocking(true)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let listener = {
            let _guard = runtime.enter();
            TcpListener::from_std(socket)?
        };
        let latest = LatestFrame::default();
        let shared = latest.clone();
        let (stop, stopped) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("emote-receiver".into())
            .spawn(move || {
                runtime.block_on(serve(listener, stopped, shared, Arc::new(repaint)));
            })?;
        Ok(Self {
            address,
            latest,
            stop: Some(stop),
            worker: Some(worker),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }
    pub fn latest(&self) -> LatestFrame {
        self.latest.clone()
    }
    pub fn shutdown(self) {
        drop(self);
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn serve(
    listener: TcpListener,
    mut stopped: oneshot::Receiver<()>,
    latest: LatestFrame,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut connections = JoinSet::new();
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            _ = ticks.tick() => {
                let changed = latest.0.lock().expect("frame lock poisoned").expire(Instant::now());
                if changed { repaint(); }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { break };
                // Refuse excess sockets immediately; there is no application queue
                // of waiting clients or unbounded collection of spawned tasks.
                if connections.len() >= MAX_CONNECTIONS { continue; }
                let latest = latest.clone();
                let repaint = repaint.clone();
                connections.spawn(async move {
                    let service = service_fn(move |request| {
                        handle(request, latest.clone(), repaint.clone())
                    });
                    let mut builder = http1::Builder::new();
                    builder.keep_alive(false).max_headers(32)
                        .max_buf_size(HEADER_BYTES).max_header_size(HEADER_BYTES)
                        .timer(TokioTimer::new()).header_read_timeout(BODY_TIMEOUT);
                    // This outer deadline also bounds slow response readers. A
                    // connection gets exactly one request, not unlimited keepalive.
                    let _ = timeout(CONNECTION_TIMEOUT,
                        builder.serve_connection(TokioIo::new(socket), service)).await;
                });
            }
        }
    }
    // Cancellation drops even partial headers/bodies; shutdown never waits for
    // clients to cooperate or for their deadlines to expire.
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    latest.0.lock().expect("frame lock poisoned").stop_focus();
}

type Reply = Response<Full<Bytes>>;
fn reply(code: StatusCode, text: &'static str) -> Reply {
    Response::builder()
        .status(code)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Connection", "close")
        .body(Full::new(Bytes::from_static(text.as_bytes())))
        .expect("static response")
}

async fn handle(
    request: Request<Incoming>,
    latest: LatestFrame,
    repaint: Arc<dyn Fn() + Send + Sync>,
) -> Result<Reply, Infallible> {
    use StatusCode as S;
    // Reject *all* Origin-bearing requests, including null/same-origin values.
    // This is a local native protocol, never a CORS-enabled browser endpoint.
    let headers = request.headers();
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("");
    let lifecycle = matches!(path, "/emote/heartbeat" | "/emote/close");
    let failure = if headers.contains_key("origin") {
        Some((S::FORBIDDEN, "Origin is not allowed\n"))
    } else if path != "/emote" && !lifecycle {
        Some((S::NOT_FOUND, "not found\n"))
    } else if request.method() != hyper::Method::POST {
        Some((S::METHOD_NOT_ALLOWED, "method not allowed\n"))
    } else if !lifecycle
        && (headers.get_all("content-type").iter().count() != 1
            || !headers
                .get("content-type")
                .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"image/png")))
    {
        Some((S::UNSUPPORTED_MEDIA_TYPE, "expected image/png\n"))
    } else if headers.get_all("x-emote-version").iter().count() != 1
        || headers.get("x-emote-version").map(|v| v.as_bytes()) != Some(b"1")
    {
        Some((S::BAD_REQUEST, "expected X-Emote-Version: 1\n"))
    } else if headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        Some((S::PAYLOAD_TOO_LARGE, "body too large\n"))
    } else {
        None
    };
    if let Some((code, text)) = failure {
        return Ok(reply(code, text));
    }

    let identity = if lifecycle || headers.contains_key("x-emote-session-id") {
        let Some(identity) = session_headers(headers) else {
            return Ok(reply(S::BAD_REQUEST, "invalid session headers\n"));
        };
        Some(identity)
    } else {
        None
    };
    // Optional ACKs never cause a policy error. Duplicate, non-ASCII and
    // malformed values are ignored, and only an accepted request may consume one.
    let ack = (headers.get_all("x-emote-focus-ack").iter().count() == 1)
        .then(|| headers.get("x-emote-focus-ack")?.to_str().ok())
        .flatten()
        .filter(|value| valid_id(value))
        .map(str::to_owned);
    let close = path == "/emote/close";
    // Image metadata is validated before the body, but committed only after
    // decode and the atomic lifecycle guards. Lifecycle endpoints ignore it.
    let project_name = if lifecycle {
        None
    } else {
        match parse_project_name(headers) {
            Ok(name) => name,
            Err(()) => return Ok(reply(S::BAD_REQUEST, "invalid project name\n")),
        }
    };
    // Only the documented `clock` value selects the internal clock; absent or
    // unknown states stay ordinary images. Unrelated metadata remains ignored.
    let state = if headers.get("x-emote-state").and_then(|v| v.to_str().ok()) == Some("clock") {
        SessionState::Clock
    } else {
        SessionState::Normal
    };
    let read = async {
        let mut body = request.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| (S::BAD_REQUEST, "malformed body\n"))?;
            if let Ok(data) = frame.into_data() {
                if data.len() > MAX_BODY_BYTES - bytes.len() {
                    return Err((S::PAYLOAD_TOO_LARGE, "body too large\n"));
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok(bytes)
    };
    let bytes = match timeout(BODY_TIMEOUT, read).await {
        Err(_) => return Ok(reply(S::REQUEST_TIMEOUT, "body timed out\n")),
        Ok(Err((code, text))) => return Ok(reply(code, text)),
        Ok(Ok(bytes)) => bytes,
    };
    let event = if lifecycle {
        if !bytes.is_empty() {
            return Ok(reply(S::BAD_REQUEST, "expected empty body\n"));
        }
        if close {
            Event::Close
        } else {
            Event::Heartbeat
        }
    } else {
        let Some(frame) = decode(&bytes) else {
            return Ok(reply(S::UNPROCESSABLE_ENTITY, "expected a 64x64 PNG\n"));
        };
        Event::Frame(Arc::new(frame), state)
    };
    // Decode and await bodies without holding the publication mutex. Sequence,
    // closed-stream and capacity checks happen atomically here, after decoding.
    let mut response = reply(S::NO_CONTENT, "");
    let changed = {
        let mut registry = latest.0.lock().expect("frame lock poisoned");
        if let Some((session, stream, sequence)) = identity {
            match registry.apply_request(
                &session,
                &stream,
                sequence,
                event,
                project_name,
                ack.as_deref(),
                Instant::now(),
            ) {
                Err(_) => return Ok(reply(S::TOO_MANY_REQUESTS, "session limit reached\n")),
                Ok(None) => false,
                Ok(Some(applied)) => {
                    if let Some(id) = applied.focus {
                        let headers = response.headers_mut();
                        headers.insert(
                            "x-emote-focus-request-id",
                            id.parse().expect("generated ASCII focus request ID"),
                        );
                    }
                    applied.changed
                }
            }
        } else {
            let Event::Frame(frame, state) = event else {
                unreachable!()
            };
            registry.publish_legacy_named(frame, state, project_name);
            true
        }
    };
    if changed {
        repaint();
    }
    Ok(response)
}

// Match the existing producer's encodeProjectName/JavaScript string bound.
// No form decoding: a literal '+' is a plus and escapes are decoded once.
fn parse_project_name(headers: &hyper::HeaderMap) -> Result<Option<String>, ()> {
    let mut values = headers.get_all("x-emote-project-name").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let encoded = value.to_str().map_err(|_| ())?;
    if encoded.len() > 1152 {
        return Err(());
    }
    let mut decoded = Vec::with_capacity(encoded.len().min(384));
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let hex = |byte: u8| {
                char::from(byte)
                    .to_digit(16)
                    .map(|digit| digit as u8)
                    .ok_or(())
            };
            (hex(bytes.next().ok_or(())?)? << 4) | hex(bytes.next().ok_or(())?)?
        } else {
            byte
        };
        if decoded.len() == 384 {
            return Err(());
        }
        decoded.push(byte);
    }
    let name = String::from_utf8(decoded).map_err(|_| ())?;
    // JavaScript trim includes BOM but excludes U+0085, unlike Rust trim.
    let whitespace = |ch: char| ch == '\u{feff}' || (ch.is_whitespace() && ch != '\u{85}');
    if name.trim_matches(whitespace).is_empty()
        || name.encode_utf16().count() > 128
        || matches!(name.as_str(), "." | "..")
        || name
            .chars()
            .any(|ch| ch <= '\u{1f}' || ch == '\u{7f}' || matches!(ch, '/' | '\\'))
    {
        return Err(());
    }
    Ok(Some(name))
}

fn session_headers(headers: &hyper::HeaderMap) -> Option<(String, String, u64)> {
    let one = |name| {
        (headers.get_all(name).iter().count() == 1)
            .then(|| headers.get(name)?.to_str().ok())
            .flatten()
    };
    let id = one("x-emote-session-id")?;
    let stream = one("x-emote-stream-id")?;
    let sequence = one("x-emote-sequence")?;
    if !valid_id(id)
        || !valid_id(stream)
        || sequence.is_empty()
        || !sequence.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((id.into(), stream.into(), sequence.parse().ok()?))
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

fn decode(bytes: &[u8]) -> Option<Frame> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: DECODE_BYTES,
        },
    );
    // Ignore descriptive metadata rather than expanding compressed text/profiles.
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    // Inspect IHDR before read_info allocates dimension-dependent decoder buffers.
    let header = decoder.read_header_info().ok()?;
    if (header.width, header.height) != (64, 64) {
        return None;
    }
    let mut reader = decoder.read_info().ok()?;
    if reader.info().animation_control.is_some() {
        return None;
    }
    let size = reader.output_buffer_size()?;
    if size > RGBA_BYTES {
        return None;
    }
    let mut pixels = vec![0; size];
    let output = reader.next_frame(&mut pixels).ok()?;
    reader.finish().ok()?; // Require complete PNG chunks/checksums, not just IDAT pixels.
    let channels = output.color_type.samples();
    let mut rgba = Vec::with_capacity(RGBA_BYTES);
    for pixel in pixels[..output.buffer_size()].chunks_exact(channels) {
        match output.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(pixel),
            png::ColorType::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            png::ColorType::Grayscale => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255])
            }
            png::ColorType::GrayscaleAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            png::ColorType::Indexed => return None, // EXPAND normally removes palettes.
        }
    }
    (rgba.len() == RGBA_BYTES).then_some(Frame { rgba })
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use crate::sessions::CLOCK_SILENCE_TIMEOUT;

    fn latest_with_clock(observed: Instant) -> LatestFrame {
        let mut registry = SessionRegistry::default();
        registry
            .apply_named(
                "clock",
                "s",
                1,
                Event::Frame(
                    Arc::new(Frame {
                        rgba: vec![0; RGBA_BYTES],
                    }),
                    SessionState::Clock,
                ),
                Some("ProjectAlpha".into()),
                observed,
            )
            .unwrap();
        LatestFrame(Arc::new(Mutex::new(registry)))
    }

    #[test]
    fn display_snapshot_applies_the_clock_silence_rule_at_the_render_seam() {
        let observed = Instant::now();
        let latest = latest_with_clock(observed);
        let (tiles, show_clock) = latest.display_snapshot_at(observed + CLOCK_SILENCE_TIMEOUT);
        assert!(!show_clock);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].name.as_deref(), Some("ProjectAlpha"));
        assert!(!tiles[0].stale, "the rule is strictly more than 15 seconds");
        let (tiles, _) = latest.display_snapshot_at(observed + CLOCK_SILENCE_TIMEOUT * 2);
        assert!(tiles[0].stale);
        assert_eq!(
            tiles[0].name.as_deref(),
            Some("ProjectAlpha"),
            "silence is presentation-only and never rewrites metadata"
        );
        assert!(
            tiles[0].snapshot.active,
            "the 30-second lifecycle lease stays independent"
        );
        // Production reads the monotonic clock itself, so fresh contact is shown.
        let (tiles, _) = latest.display_snapshot();
        assert!(!tiles[0].stale);
    }
}
