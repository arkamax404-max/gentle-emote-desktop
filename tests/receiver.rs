use gentle_emote_desktop::receiver::{BODY_TIMEOUT, MAX_BODY_BYTES, Receiver};
use gentle_emote_desktop::sessions::SessionState;
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
        // reset or abort *after* the entire HTTP response. Never hide a partial
        // response or another I/O error. Callers still assert exact status/body.
        let complete = complete_length_framed_response(&text);
        let transport_only = expect_transport_close && text.is_empty();
        let teardown = matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
        );
        if !teardown || !(complete || transport_only) {
            return Err(error);
        }
    }
    Ok(text)
}

// Narrow test-only framing guard: the receiver's error replies have one explicit
// Content-Length, never Transfer-Encoding. Do not infer completion from teardown.
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
// Only this helper permits an *empty* reset/abort, never a partial HTTP reply.
fn transport_response(socket: TcpStream) -> String {
    read_wire(socket, true).unwrap()
}

// A deterministic read error avoids depending on the OS scheduling the early 413
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
fn response_helper_accepts_aborted_only_after_complete_framed_response() {
    let reader = std::io::Cursor::new(COMPLETE_413.as_bytes())
        .chain(ReadError(std::io::ErrorKind::ConnectionAborted));
    assert_response(&read_response(reader).unwrap(), 413, "body too large\n");
    assert_eq!(
        read_wire(ReadError(std::io::ErrorKind::ConnectionAborted), true).unwrap(),
        ""
    );
}

#[test]
fn response_helper_rejects_incomplete_or_ambiguous_reset_and_other_errors() {
    for kind in [
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionAborted,
    ] {
        assert_rejected_teardown(kind);
    }
    for kind in [
        std::io::ErrorKind::TimedOut,
        std::io::ErrorKind::UnexpectedEof,
    ] {
        for expect_transport_close in [false, true] {
            let reader = std::io::Cursor::new(COMPLETE_413.as_bytes()).chain(ReadError(kind));
            assert_eq!(
                read_wire(reader, expect_transport_close)
                    .unwrap_err()
                    .kind(),
                kind
            );
            assert_eq!(
                read_wire(ReadError(kind), expect_transport_close)
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }
}

fn assert_rejected_teardown(kind: std::io::ErrorKind) {
    // Every truncated prefix, including empty input and a complete header with
    // only part of the body, must remain a read failure.
    for end in 0..COMPLETE_413.len() {
        let reader = std::io::Cursor::new(&COMPLETE_413.as_bytes()[..end]).chain(ReadError(kind));
        assert_eq!(read_response(reader).unwrap_err().kind(), kind);
        if end > 0 {
            let reader =
                std::io::Cursor::new(&COMPLETE_413.as_bytes()[..end]).chain(ReadError(kind));
            assert_eq!(read_wire(reader, true).unwrap_err().kind(), kind);
        }
    }
    assert_eq!(read_wire(ReadError(kind), true).unwrap(), "");
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
        for expect_transport_close in [false, true] {
            let reader = std::io::Cursor::new(wire.as_bytes()).chain(ReadError(kind));
            assert_eq!(
                read_wire(reader, expect_transport_close)
                    .unwrap_err()
                    .kind(),
                kind,
                "accepted {wire:?}"
            );
        }
    }
}

#[test]
fn response_helper_does_not_bypass_exact_payload_assertions() {
    // Correct framing alone cannot turn the wrong payload into a passing test.
    let wire = COMPLETE_413.replace("body too large", "body too small");
    for kind in [
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionAborted,
    ] {
        let reader = std::io::Cursor::new(wire.as_bytes()).chain(ReadError(kind));
        let result = read_response(reader).expect("complete framing must be readable");
        assert!(
            std::panic::catch_unwind(|| {
                assert_response(&result, 413, "body too large\n");
            })
            .is_err()
        );
    }
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

fn project_headers(session: &str, stream: &str, sequence: &str, encoded: &str) -> String {
    format!(
        "{}X-Emote-Project-Name: {encoded}\r\n",
        session_state_headers(session, stream, sequence, "clock")
    )
}

#[test]
fn project_name_decodes_existing_producer_names_and_preserves_legacy() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let latest = receiver.latest();
    let cases = [
        ("Mi%20t%C3%ADtulo".into(), "Mi título".into()),
        (
            "carpeta-%C3%A9%20%2B%20demo".into(),
            "carpeta-é + demo".into(),
        ),
        ("a+b".into(), "a+b".into()),
        ("%252E".into(), "%2E".into()),
        ("%20folder%20".into(), " folder ".into()),
        ("%C2%85".into(), "\u{85}".into()),
        ("x".repeat(128), "x".repeat(128)),
        ("%E7%95%8C".repeat(128), "界".repeat(128)),
        ("%F0%9F%9A%80".repeat(64), "🚀".repeat(64)),
    ];
    for (index, (encoded, expected)) in cases.iter().enumerate() {
        let sequence = (index + 1).to_string();
        assert_response(
            &request(
                address,
                "POST",
                "/emote",
                &project_headers("a", "s", &sequence, encoded),
                &png(64, 64, index as u8),
            ),
            204,
            "",
        );
        let (snapshot, name) = latest.snapshots_with_names().remove(0);
        assert_eq!(name.as_deref(), Some(expected.as_str()));
        assert_eq!(snapshot.state, SessionState::Clock);
        assert!(Arc::ptr_eq(&snapshot.frame, &latest.get().unwrap()));
        assert_eq!(count.load(Ordering::SeqCst), index + 1);
    }
    // Missing metadata replaces the old name with None, never an opaque ID.
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &session_headers("a", "s", "100"),
            &png(64, 64, 90),
        ),
        204,
        "",
    );
    assert!(latest.snapshots_with_names()[0].1.is_none());
    let legacy = format!("{HEADERS}X-Emote-Project-Name: Legacy%20folder\r\n");
    assert_response(
        &request(address, "POST", "/emote", &legacy, &png(64, 64, 91)),
        204,
        "",
    );
    let (snapshot, name) = latest.snapshots_with_names().remove(0);
    assert!(snapshot.session_id.is_none());
    assert_eq!(name.as_deref(), Some("Legacy folder"));
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 92)),
        204,
        "",
    );
    assert!(latest.snapshots_with_names()[0].1.is_none());
    assert_eq!(count.load(Ordering::SeqCst), cases.len() + 3);
}

#[test]
fn project_name_rejection_preserves_pixels_state_name_notifications_and_sequence() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let latest = receiver.latest();
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &project_headers("a", "s", "1", "before"),
            &png(64, 64, 9),
        ),
        204,
        "",
    );
    let saved = latest.get().unwrap();
    let mut invalid: Vec<String> = [
        "",
        "%",
        "%0",
        "%GG",
        "%FF",
        "%C3%28",
        "%ED%A0%80",
        "%00",
        "%1F",
        "%7F",
        "a%2Fb",
        "a%5Cb",
        ".",
        "..",
        "%20%C2%A0%EF%BB%BF",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    invalid.extend([
        "x".repeat(129),
        "%E7%95%8C".repeat(129),
        "%F0%9F%9A%80".repeat(65),
    ]);
    let mut headers: Vec<String> = invalid
        .iter()
        .map(|name| project_headers("a", "s", "2", name))
        .collect();
    headers.push(format!(
        "{}X-Emote-Project-Name: second\r\n",
        project_headers("a", "s", "2", "first")
    ));
    for header in headers {
        assert_response(
            &request(address, "POST", "/emote", &header, &png(64, 64, 99)),
            400,
            "invalid project name\n",
        );
        let (snapshot, name) = latest.snapshots_with_names().remove(0);
        assert!(Arc::ptr_eq(&saved, &snapshot.frame));
        assert_eq!(snapshot.state, SessionState::Clock);
        assert!(snapshot.active);
        assert_eq!(name.as_deref(), Some("before"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    // Existing Origin and identity policy still precede optional-name parsing.
    for (header, status, message) in [
        (
            format!("{}Origin: null\r\n", project_headers("a", "s", "2", "%")),
            403,
            "Origin is not allowed\n",
        ),
        (
            project_headers("bad id", "s", "2", "%"),
            400,
            "invalid session headers\n",
        ),
    ] {
        assert_response(
            &request(address, "POST", "/emote", &header, &png(64, 64, 99)),
            status,
            message,
        );
    }
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &project_headers("a", "s", "2", "after"),
            b"bad",
        ),
        422,
        "expected a 64x64 PNG\n",
    );
    assert!(Arc::ptr_eq(&saved, &latest.get().unwrap()));
    assert_eq!(
        latest.snapshots_with_names()[0].1.as_deref(),
        Some("before")
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &project_headers("a", "s", "2", "after"),
            &png(64, 64, 10),
        ),
        204,
        "",
    );
    assert_eq!(latest.snapshots_with_names()[0].1.as_deref(), Some("after"));
    assert_eq!(latest.get().unwrap().rgba[0], 10);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn project_name_delayed_body_cannot_rename_after_newer_lifecycle_or_takeover() {
    for (path, stream, sequence, body) in [
        ("/emote/heartbeat", "old", "3", vec![]),
        ("/emote/close", "old", "3", vec![]),
        ("/emote", "new", "0", png(64, 64, 7)),
    ] {
        let (receiver, count) = start();
        let address = receiver.local_addr();
        let latest = receiver.latest();
        assert_response(
            &request(
                address,
                "POST",
                "/emote",
                &project_headers("a", "old", "1", "original"),
                &png(64, 64, 9),
            ),
            204,
            "",
        );
        let bytes = png(64, 64, 99);
        let mut slow = connect(address);
        write!(
            slow,
            "POST /emote HTTP/1.1\r\nHost: localhost\r\n{}Content-Length: {}\r\n\r\n",
            project_headers("a", "old", "2", "outdated"),
            bytes.len()
        )
        .unwrap();
        slow.write_all(&bytes[..1]).unwrap();
        let name = if path == "/emote" { "current" } else { "%ZZ" };
        assert_response(
            &request(
                address,
                "POST",
                path,
                &project_headers("a", stream, sequence, name),
                &body,
            ),
            204,
            "",
        );
        let saved = latest.snapshots_with_names().remove(0);
        assert_eq!(
            saved.1.as_deref(),
            Some(if path == "/emote" {
                "current"
            } else {
                "original"
            })
        );
        let before = count.load(Ordering::SeqCst);
        slow.write_all(&bytes[1..]).unwrap();
        assert_response(&response(slow), 204, "");
        let after = latest.snapshots_with_names().remove(0);
        assert!(Arc::ptr_eq(&saved.0.frame, &after.0.frame));
        assert_eq!(saved.0.active, after.0.active);
        assert_eq!(saved.0.state, after.0.state);
        assert_eq!(saved.1, after.1);
        assert_eq!(count.load(Ordering::SeqCst), before);
    }
}

#[test]
fn project_name_capacity_rejection_and_unknown_heartbeat_leave_names_unchanged() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let latest = receiver.latest();
    assert_response(
        &request(
            address,
            "POST",
            "/emote/heartbeat",
            &project_headers("unknown", "s", "0", "ignored"),
            &[],
        ),
        204,
        "",
    );
    assert!(latest.snapshots_with_names().is_empty());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    for i in 0..32 {
        let id = i.to_string();
        assert_response(
            &request(
                address,
                "POST",
                "/emote",
                &project_headers(&id, "s", "0", &format!("folder{i}")),
                &png(64, 64, i),
            ),
            204,
            "",
        );
    }
    let saved = latest.snapshots_with_names();
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &project_headers("extra", "s", "0", "extra"),
            &png(64, 64, 99),
        ),
        429,
        "session limit reached\n",
    );
    for (i, (snapshot, name)) in latest.snapshots_with_names().iter().enumerate() {
        assert!(Arc::ptr_eq(&saved[i].0.frame, &snapshot.frame));
        assert_eq!(name.as_deref(), Some(format!("folder{i}").as_str()));
    }
    assert_eq!(count.load(Ordering::SeqCst), 32);
    assert_response(
        &request(
            address,
            "POST",
            "/emote/close",
            &session_headers("0", "s", "1"),
            &[],
        ),
        204,
        "",
    );
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &project_headers("extra", "s", "0", "extra"),
            &png(64, 64, 99),
        ),
        204,
        "",
    );
    assert_eq!(
        latest.snapshots_with_names().last().unwrap().1.as_deref(),
        Some("extra")
    );
    assert_eq!(count.load(Ordering::SeqCst), 34);
}

fn session_headers(session: &str, stream: &str, sequence: &str) -> String {
    format!(
        "{HEADERS}X-Emote-Session-Id: {session}\r\nX-Emote-Stream-Id: {stream}\r\nX-Emote-Sequence: {sequence}\r\n"
    )
}

fn session_state_headers(session: &str, stream: &str, sequence: &str, state: &str) -> String {
    format!(
        "{}X-Emote-State: {state}\r\n",
        session_headers(session, stream, sequence)
    )
}

#[test]
fn session_http_order_close_takeover_and_legacy_compatibility() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let send = |path: &str, stream: &str, sequence: &str, body: &[u8]| {
        request(
            address,
            "POST",
            path,
            &session_headers("session", stream, sequence),
            body,
        )
    };
    assert_response(&send("/emote/heartbeat", "a", "0", &[]), 204, "");
    assert!(receiver.latest().get().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_response(&send("/emote", "a", "1", &png(64, 64, 1)), 204, "");
    let saved = receiver.latest().get().unwrap();
    assert_response(&send("/emote/heartbeat", "a", "3", &[]), 204, "");
    assert_response(&send("/emote", "a", "2", &png(64, 64, 2)), 204, "");
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_response(&send("/emote/close", "a", "4", &[]), 204, "");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_response(&send("/emote", "a", "5", &png(64, 64, 5)), 204, "");
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_response(&send("/emote", "b", "0", &png(64, 64, 6)), 204, "");
    assert_response(&send("/emote", "a", "99", &png(64, 64, 7)), 204, "");
    assert_eq!(receiver.latest().get().unwrap().rgba[0], 6);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_response(
        &request(address, "POST", "/emote", HEADERS, &png(64, 64, 8)),
        204,
        "",
    );
    assert_eq!(receiver.latest().get().unwrap().rgba[0], 8);
    assert_eq!(count.load(Ordering::SeqCst), 4);
}

#[test]
fn session_http_invalid_input_never_changes_image_or_notifications() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    let valid = session_headers("ok", "stream", "0");
    assert_response(
        &request(address, "POST", "/emote", &valid, &png(64, 64, 9)),
        204,
        "",
    );
    let saved = receiver.latest().get().unwrap();
    let mut cases = Vec::new();
    for id in ["", "has space", "bad/id", "é", &"a".repeat(129)] {
        cases.push((
            "/emote",
            session_headers(id, "stream", "1"),
            png(64, 64, 1),
            "invalid session headers\n",
        ));
        cases.push((
            "/emote",
            session_headers("ok", id, "1"),
            png(64, 64, 1),
            "invalid session headers\n",
        ));
    }
    for sequence in ["", "-1", "+1", "1.0", "18446744073709551616"] {
        cases.push((
            "/emote",
            session_headers("ok", "stream", sequence),
            png(64, 64, 1),
            "invalid session headers\n",
        ));
    }
    for name in [
        "X-Emote-Session-Id",
        "X-Emote-Stream-Id",
        "X-Emote-Sequence",
    ] {
        cases.push((
            "/emote",
            format!("{valid}{name}: x\r\n"),
            png(64, 64, 1),
            "invalid session headers\n",
        ));
    }
    for missing in ["X-Emote-Stream-Id: stream\r\n", "X-Emote-Sequence: 0\r\n"] {
        cases.push((
            "/emote",
            valid.replace(missing, ""),
            png(64, 64, 1),
            "invalid session headers\n",
        ));
    }
    cases.push((
        "/emote/heartbeat",
        HEADERS.into(),
        vec![],
        "invalid session headers\n",
    ));
    cases.push((
        "/emote/close",
        valid.clone(),
        vec![1],
        "expected empty body\n",
    ));
    for (path, headers, body, message) in cases {
        assert_response(
            &request(address, "POST", path, &headers, &body),
            400,
            message,
        );
        assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    // Invalid PNGs must not consume the sequence that a later valid frame uses.
    let headers = session_headers("ok", "stream", "18446744073709551615");
    assert_response(
        &request(address, "POST", "/emote", &headers, b"bad"),
        422,
        "expected a 64x64 PNG\n",
    );
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_response(
        &request(address, "POST", "/emote", &headers, &png(64, 64, 10)),
        204,
        "",
    );
    assert_eq!(receiver.latest().get().unwrap().rgba[0], 10);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn session_http_multiple_sessions_and_delayed_publication() {
    let (receiver, count) = start();
    let addr = receiver.local_addr();
    for (id, value) in [("b", 1), ("a", 2)] {
        assert_response(
            &request(
                addr,
                "POST",
                "/emote",
                &session_headers(id, "old", "1"),
                &png(64, 64, value),
            ),
            204,
            "",
        );
    }
    let latest = receiver.latest();
    let b = latest.snapshots()[0].frame.clone();
    assert_eq!(
        latest
            .snapshots()
            .iter()
            .map(|s| s.session_id.clone().unwrap())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    // Hold an older frame body open while newer lifecycle requests are accepted.
    for (path, stream, sequence, body) in [
        ("/emote/heartbeat", "old", "3", vec![]),
        ("/emote/close", "old", "4", vec![]),
        ("/emote", "new", "0", png(64, 64, 7)),
    ] {
        let bytes = png(64, 64, 99);
        let mut slow = connect(addr);
        write!(
            slow,
            "POST /emote HTTP/1.1\r\nHost: localhost\r\n{}Content-Length: {}\r\n\r\n",
            session_headers("a", "old", "2"),
            bytes.len()
        )
        .unwrap();
        slow.write_all(&bytes[..1]).unwrap();
        assert_response(
            &request(
                addr,
                "POST",
                path,
                &session_headers("a", stream, sequence),
                &body,
            ),
            204,
            "",
        );
        let before = count.load(Ordering::SeqCst);
        let saved = latest.get().unwrap();
        slow.write_all(&bytes[1..]).unwrap();
        assert_response(&response(slow), 204, "");
        assert_eq!(count.load(Ordering::SeqCst), before);
        assert!(Arc::ptr_eq(&saved, &latest.get().unwrap()));
        assert!(Arc::ptr_eq(&b, &latest.snapshots()[0].frame));
    }
    assert_eq!(latest.snapshots().len(), 2);
    assert_eq!(latest.snapshots()[1].frame.rgba[0], 7);
    assert_eq!(count.load(Ordering::SeqCst), 4);
    // Bodyless lifecycle traffic does not require image media type.
    let headers = session_headers("a", "new", "1").replace("Content-Type: image/png\r\n", "");
    assert_response(
        &request(addr, "POST", "/emote/close", &headers, &[]),
        204,
        "",
    );
    assert_eq!(latest.snapshots().len(), 1);
    assert_eq!(latest.snapshots()[0].session_id.as_deref(), Some("b"));
    assert_response(
        &request(
            addr,
            "POST",
            "/emote/close",
            &session_headers("b", "old", "2"),
            &[],
        ),
        204,
        "",
    );
    assert_eq!(latest.snapshots().len(), 1);
    assert!(!latest.snapshots()[0].active);
    assert_eq!(latest.snapshots()[0].session_id.as_deref(), Some("b"));
}

#[test]
fn session_http_active_cap_and_identifier_boundaries() {
    use gentle_emote_desktop::sessions::MAX_ACTIVE_SESSIONS;
    let (receiver, count) = start();
    let addr = receiver.local_addr();
    let id = "a".repeat(128);
    let stream = format!("{}._:-", "Z".repeat(124));
    assert_response(
        &request(
            addr,
            "POST",
            "/emote",
            &session_headers(&id, &stream, "0"),
            &png(64, 64, 1),
        ),
        204,
        "",
    );
    for n in 1..MAX_ACTIVE_SESSIONS {
        assert_response(
            &request(
                addr,
                "POST",
                "/emote",
                &session_headers(&n.to_string(), "s", "0"),
                &png(64, 64, 2),
            ),
            204,
            "",
        );
    }
    let saved = receiver.latest().get().unwrap();
    assert_response(
        &request(
            addr,
            "POST",
            "/emote",
            &session_headers("overflow", "s", "0"),
            &png(64, 64, 3),
        ),
        429,
        "session limit reached\n",
    );
    assert_eq!(receiver.latest().snapshots().len(), MAX_ACTIVE_SESSIONS);
    assert!(Arc::ptr_eq(&saved, &receiver.latest().get().unwrap()));
    assert_eq!(count.load(Ordering::SeqCst), MAX_ACTIVE_SESSIONS);
    assert_response(
        &request(
            addr,
            "POST",
            "/emote/close",
            &session_headers(&id, &stream, "1"),
            &[],
        ),
        204,
        "",
    );
    // The rejected activation must not have consumed sequence zero.
    assert_response(
        &request(
            addr,
            "POST",
            "/emote",
            &session_headers("overflow", "s", "0"),
            &png(64, 64, 4),
        ),
        204,
        "",
    );
    assert_eq!(receiver.latest().snapshots().len(), MAX_ACTIVE_SESSIONS);
    assert_eq!(receiver.latest().get().unwrap().rgba[0], 4);
    assert_eq!(count.load(Ordering::SeqCst), MAX_ACTIVE_SESSIONS + 2);
}

#[test]
fn session_http_repaint_can_read_snapshots_after_publication() {
    use gentle_emote_desktop::receiver::LatestFrame;
    use std::sync::{OnceLock, mpsc};
    let handle = Arc::new(OnceLock::<LatestFrame>::new());
    let callback_handle = handle.clone();
    let (tx, rx) = mpsc::channel();
    let receiver = Receiver::bind("127.0.0.1:0".parse().unwrap(), move || {
        let latest = callback_handle.get().unwrap();
        tx.send((latest.get().unwrap().rgba[0], latest.snapshots()[0].active))
            .unwrap();
    })
    .unwrap();
    assert!(handle.set(receiver.latest()).is_ok());
    for (path, sequence, body, active) in [
        ("/emote", "0", png(64, 64, 8), true),
        ("/emote/close", "1", vec![], false),
    ] {
        assert_response(
            &request(
                receiver.local_addr(),
                "POST",
                path,
                &session_headers("session", "s", sequence),
                &body,
            ),
            204,
            "",
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (8, active)
        );
    }
    receiver.shutdown();
    assert_eq!(handle.get().unwrap().snapshots()[0].frame.rgba[0], 8);
}

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
fn x_emote_state_clock_maps_to_internal_state_and_other_values_stay_normal() {
    let (receiver, count) = start();
    let address = receiver.local_addr();
    // A legacy frame declaring the clock state renders internally per session
    // semantics but keeps the ordinary legacy singleton slot.
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            "Content-Type: image/png\r\nX-Emote-Version: 1\r\nX-Emote-State: clock\r\n",
            &png(64, 64, 1),
        ),
        204,
        "",
    );
    let legacy = receiver.latest().snapshots();
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].session_id, None);
    assert_eq!(legacy[0].state, SessionState::Clock);
    assert!(legacy[0].active);
    // Unknown state metadata stays accepted-and-ignored (normal avatar).
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &session_state_headers("s1", "st", "0", "thinking"),
            &png(64, 64, 2),
        ),
        204,
        "",
    );
    // The documented clock value maps to the internal clock state.
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &session_state_headers("s2", "st", "0", "clock"),
            &png(64, 64, 3),
        ),
        204,
        "",
    );
    // An absent header stays normal.
    assert_response(
        &request(
            address,
            "POST",
            "/emote",
            &session_headers("s3", "st", "0"),
            &png(64, 64, 4),
        ),
        204,
        "",
    );
    let snapshots = receiver.latest().snapshots();
    assert_eq!(snapshots.len(), 4);
    assert_eq!(snapshots[1].session_id.as_deref(), Some("s1"));
    assert_eq!(snapshots[1].state, SessionState::Normal);
    assert_eq!(snapshots[2].session_id.as_deref(), Some("s2"));
    assert_eq!(snapshots[2].state, SessionState::Clock);
    assert_eq!(snapshots[3].session_id.as_deref(), Some("s3"));
    assert_eq!(snapshots[3].state, SessionState::Normal);
    // Accepted metadata frames notify exactly like ordinary frames.
    assert_eq!(count.load(Ordering::SeqCst), 4);
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
