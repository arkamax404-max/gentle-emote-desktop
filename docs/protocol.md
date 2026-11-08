# Local emote receiver

This Rust library is the network half of the desktop application. It accepts the
existing sender's PNG protocol and exposes the latest decoded frame to the native
egui/eframe window. Run and interaction instructions are in the [README](../README.md).

## Use from a window

The package is `gentle-emote-desktop`; the Rust library name is
`gentle_emote_desktop`. The executable uses eframe/egui with the Glow renderer.

```rust
use gentle_emote_desktop::receiver::Receiver;

// In eframe, clone the egui Context into this closure and call
// context.request_repaint(). Never do window work directly on this thread.
let receiver = Receiver::start(|| { /* notify GUI to repaint */ })?;
let latest = receiver.latest();

// In the GUI update callback:
if let Some(frame) = latest.get() {
    // frame.rgba is exactly 64 * 64 * 4 bytes, RGBA8, row-major,
    // unpremultiplied. Upload it only when the Arc differs from the previous
    // frame (Arc::ptr_eq), then render with nearest-neighbor filtering.
}

// Keep `receiver` owned by the app. Exit explicitly or simply drop the app.
receiver.shutdown();
Ok::<(), std::io::Error>(())
```

`Receiver::start(repaint) -> io::Result<Receiver>` binds **127.0.0.1:8001**.
The simulator keeps port 8000. Bind failures propagate; there is no fallback to
another port or interface. `Receiver::bind(SocketAddr, repaint)` permits alternate
ports, including port 0 for tests, but only on exactly 127.0.0.1 (IPv4).
`local_addr()` reports the actual bound address.

`latest() -> LatestFrame` returns a cheap cloneable handle.
`LatestFrame::get() -> Option<Arc<Frame>>` initially returns `None`; once populated,
it returns a snapshot that stays valid across replacement and shutdown. `Frame`
contains public `rgba: Vec<u8>`; dimensions are always 64x64. Treat it as immutable.
The receiver stores only **one** frame, never a history or a pending-frame queue.
External consumers can retain snapshots, so their memory use is their own concern.

Publication swaps a complete image under a short mutex lock, releases the lock,
then calls the repaint closure on the network thread. The closure must be quick,
nonblocking, and nonpanicking; `egui::Context::request_repaint` is the intended
integration. It may safely read the latest slot. Notifications do not contain
image payloads. The GUI should coalesce repaint requests, not enqueue every frame.
Rejected requests neither replace the slot nor invoke the callback.

`shutdown(self)` and `Drop` both signal cancellation, abort all connection tasks,
and synchronously join the owned network thread. They do not wait for partial
clients or body deadlines. Retain the receiver on the GUI thread; do not arrange
for its own repaint callback to drop it or block waiting for that GUI thread.

## Wire protocol

Send one HTTP/1.1 request with a binary PNG body (not JSON, base64, or multipart):

```http
POST /emote HTTP/1.1
Host: 127.0.0.1:8001
Content-Type: image/png
X-Emote-Version: 1
Content-Length: <PNG byte length>

<binary 64x64 PNG>
```

Both Content-Length and HTTP chunked bodies work. Each connection handles one
request and closes; keepalive/pipelining are not supported. Content-Type is
case-insensitive but must be exactly `image/png` without parameters. Exactly one
Content-Type and one X-Emote-Version header are required. Version must be `1`.
The sender's additional state/frame/set/activity/stream headers are accepted and
ignored: visual labels are already in the pixels. No metadata or payload is
logged or persisted.

The PNG must be exactly 64x64. Standard grayscale, RGB, palette, and alpha PNGs
are normalized to RGBA8; 16-bit samples are reduced to 8 bits. Alpha is preserved,
not converted into window transparency. Animated PNGs are rejected. Complete
chunks/checksums through IEND are required. Trailing bytes after a complete PNG
are tolerated, but still count toward the body limit. Descriptive text and ICC
profiles are ignored rather than decompressed. There is no color management.

Successful requests return **204 No Content**, empty body. The most recently
published valid image wins. Simultaneous clients have no sender-order guarantee;
publication order on the single worker is authoritative. Old images are not
replayed and there is no animation queue.

### Rejections

Application responses have `Connection: close` and
`Content-Type: text/plain; charset=utf-8`. Error bodies below end in one newline.
Policy precedence is Origin, path, method, media type, version, length, body, PNG.

| Condition | Status | Exact body (before newline) |
| --- | --- | --- |
| Any Origin header, including `null` or local origins | 403 | `Origin is not allowed` |
| Not exactly `/emote`, including a query string | 404 | `not found` |
| Method other than POST | 405 | `method not allowed` |
| Missing, duplicate, or wrong Content-Type | 415 | `expected image/png` |
| Missing, duplicate, or unsupported version | 400 | `expected X-Emote-Version: 1` |
| Declared or streamed body exceeds the limit | 413 | `body too large` |
| Body cannot finish within the deadline | 408 | `body timed out` |
| Malformed HTTP body framing surfaced to the handler | 400 | `malformed body` |
| Empty, invalid, truncated, animated, wrong-size, or decode-limit PNG | 422 | `expected a 64x64 PNG` |

Hyper itself rejects malformed HTTP syntax/headers (normally 400 with empty body)
and oversized headers. Incomplete headers are closed at the header deadline.
Excess connections, client disconnects, the outer connection deadline, and server
shutdown can close sockets without an HTTP response. Clients must handle these
transport failures as well as application statuses. They never publish a partial
image. A response-reader timeout can occur after an already valid publication;
HTTP acknowledgement is not an exactly-once delivery guarantee.

## Limits and local trust boundary

| Resource | Limit | Why |
| --- | --- | --- |
| Encoded body | 65,536 bytes, inclusive | Plenty for a raw 64x64 RGBA PNG; rejects large uploads, even chunked |
| Image dimensions | Exactly 64x64, checked immediately after IHDR | Rejects dimension bombs before dimension-dependent decoder allocations |
| PNG decoder budget | 256 KiB | Explicit `png::Limits` budget for decoder-managed allocations |
| Decoded output | At most 16,384 bytes, then exactly 16,384 RGBA bytes | Bounds caller-owned decode/output buffers separately |
| Headers | 32 fields, 8 KiB | Bounds HTTP parser/header storage |
| Concurrent connections/tasks | 8 | Excess accepted sockets are immediately closed; no application wait queue |
| Complete headers | 1 second | Bounds slow header writers |
| Complete body | 1 second from handler body-read start | Absolute deadline, not renewed for each chunk |
| Entire connection | 3 seconds from accept | Also bounds slow response readers |
| Stored frames | 1 | Last valid image wins |

The PNG library's allocation budget is documented upstream as best-effort, not a
process-wide allocator cap. The independent encoded-byte, early-dimension,
metadata-skip, and output-size limits remain in force; ordinary runtime, parser,
stack, and allocator overhead is additional. Decode work runs synchronously on
the single worker with these small bounded inputs, not on an unbounded blocking
pool. The OS TCP listen backlog is separate from the application task limit.

This is a **local trusted-process MVP**, not authentication. Binding exactly
127.0.0.1 avoids LAN exposure. Requiring PNG media/version headers, rejecting
*all* Origin-bearing requests, and never emitting CORS headers prevents normal
browser cross-origin submissions (including preflights). Origin checks apply to
OPTIONS too. A nonbrowser local process can forge or omit Origin and replace the
image or consume available connections. No auth key, remote-access mode, TLS,
platform installer, or global sender configuration change is part of this unit.

## Build and verify

With Rust stable and Cargo available, run from the repository root:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Receiver tests use real TCP/HTTP requests on ephemeral 127.0.0.1 ports, with no
system packages or running GUI required. They assert status/body and retained
image plus notification count on rejection, inclusive byte bounds, chunked transfers, body
and header deadlines, recovery, replacement, concurrent publication, bind errors,
and both shutdown paths. There are no test scratch directories. The source is
intended for macOS, Windows, and Linux; passing on one host is not execution proof
on the other platforms. The README describes the separate native GUI smoke and
its limits; headless tests alone do not establish OS window behavior.
