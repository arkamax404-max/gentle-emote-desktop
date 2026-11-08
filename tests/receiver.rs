use gentle_emote_desktop::receiver::{BODY_TIMEOUT, MAX_BODY_BYTES, Receiver};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![value; (width * height * 4) as usize])
            .unwrap();
    }
    bytes
}

fn start() -> (Receiver, Arc<AtomicUsize>) {
    let notifications = Arc::new(AtomicUsize::new(0));
    let count = notifications.clone();
    let receiver = Receiver::bind("127.0.0.1:0".parse().unwrap(), move || {
        count.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    (receiver, notifications)
}

fn connect(address: SocketAddr) -> TcpStream {
    let socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
}

fn response(socket: TcpStream) -> String {
    read_response(socket).unwrap()
}

fn read_response(reader: impl Read) -> std::io::Result<String> {
    read_wire(reader, false)
}

fn read_wire(mut reader: impl Read, expect_transport_close: bool) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    let read = reader.read_to_end(&mut bytes);
    let text = String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if let Err(error) = read {
        // An early rejection can close with unread inbound bytes, producing a
        // reset *after* the entire HTTP response. Never hide a partial response
        // or a different I/O error. Callers still assert exact status and body.
        let complete = complete_length_framed_response(&text);
        let transport_only = expect_transport_close && text.is_empty();
        if error.kind() != std::io::ErrorKind::ConnectionReset || !(complete || transport_only) {
            return Err(error);
        }
    }
    Ok(text)
}

// Narrow test-only framing guard: the receiver's error replies have one explicit
// Content-Length, never Transfer-Encoding. Do not infer completion from EOF/reset.
fn complete_length_framed_response(text: &str) -> bool {
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let mut lines = head.split("\r\n");
    let mut status = lines.next().unwrap_or_default().splitn(3, ' ');
    if status.next() != Some("HTTP/1.1") {
        return false;
    }
    let code = status.next().unwrap_or_default();
    if code.len() != 3
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || !code
            .parse::<u16>()
            .is_ok_and(|code| (200..600).contains(&code))
        || !status.next().is_some_and(|reason| {
            !reason.is_empty()
                && reason
                    .bytes()
                    .all(|b| b.is_ascii() && !b.is_ascii_control())
        })
    {
        return false;
    }
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || !value
                .bytes()
                .all(|byte| byte == b'\t' || (byte.is_ascii() && !byte.is_ascii_control()))
        {
            return false;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return false;
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
            let Ok(parsed) = value.parse::<usize>() else {
                return false;
            };
            length = Some(parsed);
        }
    }
    length == Some(body.len())
}

// The bounds probes explicitly expect a transport-only close for some inputs.
// Only this separate helper permits an *empty* reset, never a partial HTTP reply.
fn transport_response(socket: TcpStream) -> String {
    read_wire(socket, true).unwrap()
}

// A deterministic reset avoids depending on the OS scheduling the early 413
// before or after all request bytes have reached the server.
struct ReadError(std::io::ErrorKind);
impl Read for ReadError {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(self.0))
    }
}

const COMPLETE_413: &str = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 15\r\nConnection: close\r\n\r\nbody too large\n";

#[test]
fn response_helper_accepts_reset_only_after_complete_framed_response() {
    let reader = std::io::Cursor::new(COMPLETE_413.as_bytes())
        .chain(ReadError(std::io::ErrorKind::ConnectionReset));
    assert_response(&read_response(reader).unwrap(), 413, "body too large\n");
    // Ordinary EOF-delimited reads retain their previous behavior.
    assert_response(
        &read_response(std::io::Cursor::new(COMPLETE_413)).unwrap(),
        413,
        "body too large\n",
    );
}

#[test]
fn response_helper_rejects_incomplete_or_ambiguous_reset_and_other_errors() {
    // Every truncated prefix, including empty input and a complete header with
    // only part of the body, must remain a read failure.
    for end in 0..COMPLETE_413.len() {
        let reader = std::io::Cursor::new(&COMPLETE_413.as_bytes()[..end])
            .chain(ReadError(std::io::ErrorKind::ConnectionReset));
        assert!(read_response(reader).is_err(), "accepted prefix {end}");
        if end > 0 {
            let reader = std::io::Cursor::new(&COMPLETE_413.as_bytes()[..end])
                .chain(ReadError(std::io::ErrorKind::ConnectionReset));
            assert!(
                read_wire(reader, true).is_err(),
                "transport path accepted prefix {end}"
            );
        }
    }
    assert_eq!(
        read_wire(ReadError(std::io::ErrorKind::ConnectionReset), true).unwrap(),
        ""
    );
    for wire in [
        COMPLETE_413.replace("Content-Length: 15", "Content-Length: 14"),
        COMPLETE_413.replace("Content-Length: 15", "Content-Length: 16"),
        COMPLETE_413.replace("Content-Length: 15\r\n", ""),
        COMPLETE_413.replace(
            "Content-Length: 15",
            "Content-Length: 15\r\nContent-Length: 15",
        ),
        COMPLETE_413.replace(
            "Content-Length: 15",
            "Content-Length: 15\r\nTransfer-Encoding: chunked",
        ),
        COMPLETE_413.replace("Content-Length: 15", "Content-Length: +15"),
        COMPLETE_413.replace(
            "Content-Length: 15",
            "Content-Length: 99999999999999999999999999999",
        ),
        COMPLETE_413.replace("HTTP/1.1 413", "HTTP/1.1 xyz"),
        COMPLETE_413.replace("Connection: close", "not a header"),
    ] {
        let reader = std::io::Cursor::new(wire.as_bytes())
            .chain(ReadError(std::io::ErrorKind::ConnectionReset));
        assert!(read_response(reader).is_err(), "accepted {wire:?}");
    }
    for kind in [
        std::io::ErrorKind::TimedOut,
        std::io::ErrorKind::UnexpectedEof,
    ] {
        let reader = std::io::Cursor::new(COMPLETE_413.as_bytes()).chain(ReadError(kind));
        assert_eq!(read_response(reader).unwrap_err().kind(), kind);
    }
}

#[test]
fn response_helper_does_not_bypass_exact_payload_assertions() {
    // Correct framing alone cannot turn the wrong payload into a passing test.
    let wire = COMPLETE_413.replace("body too large", "body too small");
    let reader =
        std::io::Cursor::new(wire.as_bytes()).chain(ReadError(std::io::ErrorKind::ConnectionReset));
    let result = read_response(reader).expect("complete framing must be readable");
    assert!(
        std::panic::catch_unwind(|| {
            assert_response(&result, 413, "body too large\n");
        })
        .is_err()
    );
}

fn request(address: SocketAddr, method: &str, path: &str, headers: &str, body: &[u8]) -> String {
    let mut socket = connect(address);
    write!(
        socket,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n{headers}\r\n",
        body.len()
    )
    .unwrap();
    socket.write_all(body).unwrap();
    response(socket)
}

const HEADERS: &str = "Content-Type: image/png\r\nX-Emote-Version: 1\r\n";

fn assert_response(response: &str, code: u16, body: &str) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {code} ")),
        "{response}"
    );
    assert_eq!(response.split_once("\r\n\r\n").unwrap().1, body);
    assert!(!response.to_ascii_lowercase().contains("access-control-"));
}

#[test]
fn accepts_replaces_and_notifies_without_a_frame_queue() {
    let (receiver, count) = start();
    let latest = receiver.latest();
    assert!(latest.get().is_none());
    for value in [17, 42] {
        let result = request(
            receiver.local_addr(),
            "POST",
            "/emote",
            HEADERS,
            &png(64, 64, value),
        );
        assert_response(&result, 204, "");
        assert_eq!(latest.get().unwrap().rgba, vec![value; 64 * 64 * 4]);
    }
    assert_eq!(count.load(Ordering::SeqCst), 2);
    receiver.shutdown();
    // The GUI's clone remains readable after the owned server has stopped.
    assert_eq!(latest.get().unwrap().rgba[0], 42);
}

#[test]
fn rejections_preserve_frame_and_notification_count_and_allow_recovery() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 7)),
        204,
        "",
    );
    let latest = receiver.latest();
    let saved = latest.get().unwrap();
    let mut truncated = png(64, 64, 1);
    truncated.truncate(truncated.len() - 12); // Remove the complete IEND chunk.
    let mut bad_crc = png(64, 64, 1);
    bad_crc[29] ^= 1; // Corrupt the IHDR checksum, not its dimensions.
    let cases = [
        (
            "GET",
            "/emote",
            HEADERS.to_owned(),
            png(64, 64, 1),
            405,
            "method not allowed\n",
        ),
        (
            "POST",
            "/other",
            HEADERS.to_owned(),
            vec![],
            404,
            "not found\n",
        ),
        (
            "POST",
            "/emote?x=1",
            HEADERS.to_owned(),
            vec![],
            404,
            "not found\n",
        ),
        (
            "POST",
            "/emote",
            "X-Emote-Version: 1\r\n".to_owned(),
            vec![],
            415,
            "expected image/png\n",
        ),
        (
            "POST",
            "/emote",
            "Content-Type: text/plain\r\nX-Emote-Version: 1\r\n".to_owned(),
            vec![],
            415,
            "expected image/png\n",
        ),
        (
            "POST",
            "/emote",
            "Content-Type: image/png\r\n".to_owned(),
            vec![],
            400,
            "expected X-Emote-Version: 1\n",
        ),
        (
            "POST",
            "/emote",
            "Content-Type: image/png\r\nX-Emote-Version: 2\r\n".to_owned(),
            vec![],
            400,
            "expected X-Emote-Version: 1\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}X-Emote-Version: 1\r\n"),
            vec![],
            400,
            "expected X-Emote-Version: 1\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}Content-Type: image/png\r\n"),
            vec![],
            415,
            "expected image/png\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}Origin: https://example.com\r\n"),
            vec![],
            403,
            "Origin is not allowed\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}Origin: null\r\n"),
            vec![],
            403,
            "Origin is not allowed\n",
        ),
        (
            "OPTIONS",
            "/emote",
            format!("{HEADERS}Origin: http://localhost\r\n"),
            vec![],
            403,
            "Origin is not allowed\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            vec![],
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            b"not PNG".to_vec(),
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            truncated,
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            bad_crc,
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            png(63, 64, 1),
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            png(64, 65, 1),
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            png(2048, 1, 1),
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            vec![0; MAX_BODY_BYTES + 1],
            413,
            "body too large\n",
        ),
    ];
    for (method, path, headers, body, code, message) in cases {
        assert_response(
            &request(address, method, path, &headers, &body),
            code,
            message,
        );
        assert!(Arc::ptr_eq(&saved, &latest.get().unwrap()));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    // The byte boundary is inclusive. PNG trailing padding is tolerated by the decoder.
    let mut boundary = png(64, 64, 8);
    boundary.resize(MAX_BODY_BYTES, 0);
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &boundary),
        204,
        "",
    );
    assert_eq!(latest.get().unwrap().rgba[0], 8);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn chunked_limit_and_body_deadline_preserve_previous_frame() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 3)),
        204,
        "",
    );
    let saved = receiver.latest().get().unwrap();
    let mut socket = connect(address);
    write!(socket, "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n", MAX_BODY_BYTES + 1).unwrap();
    socket.write_all(&vec![0; MAX_BODY_BYTES + 1]).unwrap();
    socket.write_all(b"\r\n0\r\n\r\n").unwrap();
    assert_response(&response(socket), 413, "body too large\n");
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);

    let mut socket = connect(address);
    write!(
        socket,
        "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Content-Length: 100\r\n\r\nx"
    )
    .unwrap();
    let before = Instant::now();
    assert_response(&response(socket), 408, "body timed out\n");
    assert!(before.elapsed() < BODY_TIMEOUT + Duration::from_secs(2));
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 9)),
        204,
        "",
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_public_requests_publish_whole_frames_and_shutdown_releases_port() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let threads: Vec<_> = (1..=4)
        .map(|value| {
            std::thread::spawn(move || {
                assert_response(
                    &request(address, "POST", "/emote", HEADERS, &png(64, 64, value)),
                    204,
                    "",
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let latest = receiver.latest().get().unwrap();
    assert!((1..=4).contains(&latest.rgba[0]));
    assert!(latest.rgba.iter().all(|byte| *byte == latest.rgba[0]));
    assert_eq!(count.load(Ordering::SeqCst), 4);
    let mut stalled = connect(address);
    stalled.write_all(b"POST /emote HTTP/1.1\r\n").unwrap();
    let before = Instant::now();
    receiver.shutdown();
    assert!(before.elapsed() < Duration::from_secs(1));
    let replacement = Receiver::bind(address, || {}).unwrap();
    drop(replacement); // Drop is also an owned, synchronous shutdown.
    assert!(TcpStream::connect(address).is_err());
}

#[test]
fn chunked_rgb_and_sender_metadata_are_supported() {
    let (receiver, count) = start();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![12; 64 * 64 * 3])
            .unwrap();
    }
    let mut socket = connect(receiver.local_addr());
    write!(socket, "POST /emote HTTP/1.1\r\nHost: localhost\r\nContent-Type: IMAGE/PNG\r\nX-Emote-Version: 1\r\nX-Emote-State: thinking\r\nX-Emote-Frame: 2\r\nX-Emote-Set: default\r\nX-Emote-Activity: coding\r\nX-Emote-Stream: test\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", bytes.len()).unwrap();
    socket.write_all(&bytes).unwrap();
    socket.write_all(b"\r\n0\r\n\r\n").unwrap();
    assert_response(&response(socket), 204, "");
    let frame = receiver.latest().get().unwrap();
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [12, 12, 12, 255])
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_http_and_stalled_headers_never_publish() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 2)),
        204,
        "",
    );
    let saved = receiver.latest().get().unwrap();
    let mut malformed = connect(address);
    malformed
        .write_all(b"POST /emote HTTP/1.1\r\nHost: localhost\r\nBad Header: x\r\n\r\n")
        .unwrap();
    assert_response(&response(malformed), 400, "");
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let mut stalled = connect(address);
    stalled.write_all(b"POST /emote HTTP/1.1\r\n").unwrap();
    let before = Instant::now();
    // Hyper closes an incomplete header block at its header deadline.
    assert_eq!(response(stalled), "");
    assert!(before.elapsed() < BODY_TIMEOUT + Duration::from_secs(2));
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let mut stalled_body = connect(address);
    write!(
        stalled_body,
        "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Content-Length: 100\r\n\r\nx"
    )
    .unwrap();
    let before = Instant::now();
    receiver.shutdown();
    assert!(before.elapsed() < Duration::from_secs(1));
}

#[test]
fn rejects_non_local_bind_and_reports_occupied_port() {
    assert!(Receiver::bind("0.0.0.0:0".parse().unwrap(), || {}).is_err());
    assert!(Receiver::bind("127.0.0.2:0".parse().unwrap(), || {}).is_err());
    let (receiver, _) = start();
    assert!(Receiver::bind(receiver.local_addr(), || {}).is_err());
}

// Durable independent-verifier probes for S1/S2. HTTP replies use the same
// guarded reader as the original tests; explicit no-response closes are separate.
#[test]
fn verify_rejection_matrix_and_inclusive_chunked_limit() {
    let (receiver, count) = start();
    let addr = receiver.local_addr();
    assert_response(
        &request(addr, "POST", "/emote", HEADERS, &png(64, 64, 9)),
        204,
        "",
    );
    let saved = receiver.latest().get().unwrap();
    let cases = [
        (
            "GET",
            "/emote",
            HEADERS.to_owned(),
            png(64, 64, 1),
            405,
            "method not allowed\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}Origin: http://127.0.0.1:8001\r\n"),
            png(64, 64, 1),
            403,
            "Origin is not allowed\n",
        ),
        (
            "POST",
            "/emote",
            format!("{HEADERS}Origin:\r\n"),
            vec![],
            403,
            "Origin is not allowed\n",
        ),
        (
            "POST",
            "/emote",
            "Content-Type: image/png; charset=utf-8\r\nX-Emote-Version: 1\r\n".into(),
            vec![],
            415,
            "expected image/png\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            png(1, 64, 1),
            422,
            "expected a 64x64 PNG\n",
        ),
        (
            "POST",
            "/emote",
            HEADERS.to_owned(),
            vec![0; MAX_BODY_BYTES + 1],
            413,
            "body too large\n",
        ),
    ];
    for (method, path, headers, body, code, message) in cases {
        let mut socket = connect(addr);
        write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n{headers}\r\n",
            body.len()
        )
        .unwrap();
        // Early rejection may also interrupt the write; the complete response
        // and exact rejection/state assertions below remain mandatory.
        let _ = socket.write_all(&body);
        assert_response(&response(socket), code, message);
        assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    let mut bytes = png(64, 64, 23);
    bytes.resize(MAX_BODY_BYTES, 0);
    let mut socket = connect(addr);
    write!(socket,"POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n",bytes.len()).unwrap();
    socket.write_all(&bytes).unwrap();
    socket.write_all(b"\r\n0\r\n\r\n").unwrap();
    assert_response(&response(socket), 204, "");
    assert_eq!(receiver.latest().get().unwrap().rgba, vec![23; 16384]);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn verify_completion_order_snapshot_and_active_shutdown() {
    let (receiver, count) = start();
    let addr = receiver.local_addr();
    let bytes = png(64, 64, 11);
    let mut slow = connect(addr);
    write!(
        slow,
        "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Content-Length: {}\r\n\r\n",
        bytes.len()
    )
    .unwrap();
    slow.write_all(&bytes[..1]).unwrap();
    assert_response(
        &request(addr, "POST", "/emote", HEADERS, &png(64, 64, 22)),
        204,
        "",
    );
    let retained = receiver.latest().get().unwrap();
    slow.write_all(&bytes[1..]).unwrap();
    assert_response(&response(slow), 204, "");
    assert_eq!(receiver.latest().get().unwrap().rgba, vec![11; 16384]);
    assert_eq!(retained.rgba, vec![22; 16384]);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let mut active = connect(addr);
    write!(
        active,
        "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}Content-Length: 100\r\n\r\nx"
    )
    .unwrap();
    let now = Instant::now();
    drop(receiver);
    assert!(now.elapsed() < Duration::from_secs(1));
    assert_eq!(retained.rgba, vec![22; 16384]);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    drop(Receiver::bind(addr, || {}).unwrap());
}

#[test]
fn verify_header_and_connection_bounds_preserve_state() {
    let (receiver, count) = start();
    let addr = receiver.local_addr();
    assert_response(
        &request(addr, "POST", "/emote", HEADERS, &png(64, 64, 31)),
        204,
        "",
    );
    let saved = receiver.latest().get().unwrap();
    for headers in [
        "X-Pad: ".to_owned() + &"a".repeat(8192) + "\r\n",
        (0..33).map(|n| format!("X-{n}: a\r\n")).collect::<String>(),
    ] {
        let mut socket = connect(addr);
        let wire = format!(
            "POST /emote HTTP/1.1\r\nHost: localhost\r\n{HEADERS}{headers}Content-Length: 0\r\n\r\n"
        );
        let _ = socket.write_all(wire.as_bytes());
        let reply = transport_response(socket);
        assert!(
            reply.is_empty() || reply.starts_with("HTTP/1.1 431 "),
            "{reply}"
        );
        assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    let mut held = Vec::new();
    for _ in 0..8 {
        let mut socket = connect(addr);
        socket.write_all(b"POST /emote HTTP/1.1\r\n").unwrap();
        held.push(socket);
    }
    std::thread::sleep(Duration::from_millis(50));
    let mut excess = connect(addr);
    let _ = excess.write_all(b"POST /emote HTTP/1.1\r\n");
    assert_eq!(transport_response(excess), "");
    for socket in held {
        assert_eq!(transport_response(socket), "");
    }
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_response(
        &request(addr, "POST", "/emote", HEADERS, &png(64, 64, 32)),
        204,
        "",
    );
}

#[test]
fn verify_normalized_png_formats_and_animation_rejection() {
    let (receiver, count) = start();
    for (color, depth, raw, expected) in [
        (
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            vec![7; 4096],
            [7, 7, 7, 255],
        ),
        (
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            [8, 90].repeat(4096),
            [8, 8, 8, 90],
        ),
        (
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0].repeat(4096),
            [0x12, 0x56, 0x9a, 0xde],
        ),
        (
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            vec![0; 4096],
            [1, 2, 3, 4],
        ),
    ] {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
            encoder.set_color(color);
            encoder.set_depth(depth);
            if color == png::ColorType::Indexed {
                encoder.set_palette(vec![1, 2, 3]);
                encoder.set_trns(vec![4]);
            }
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&raw)
                .unwrap();
        }
        assert_response(
            &request(receiver.local_addr(), "POST", "/emote", HEADERS, &bytes),
            204,
            "",
        );
        assert_eq!(receiver.latest().get().unwrap().rgba, expected.repeat(4096));
    }
    let saved = receiver.latest().get().unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_animated(1, 0).unwrap();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![1; 16384])
            .unwrap();
    }
    assert_response(
        &request(receiver.local_addr(), "POST", "/emote", HEADERS, &bytes),
        422,
        "expected a 64x64 PNG\n",
    );
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 4);
}
