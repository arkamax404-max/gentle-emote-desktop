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
Legacy requests still replace one latest-frame slot, never an image queue.
Session-aware requests also retain bounded per-session frames (see below).
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
Most additional state/frame/set/activity/stream metadata is accepted and ignored:
visual labels are already in the pixels. `X-Emote-Project-Name` optionally supplies
the resolved title/folder name (see below). `X-Emote-State`'s documented value
`clock` marks the sender's presentation as the idle clock,
which the desktop renders internally (see below); every other value, including
absent or unknown ones, stays an ordinary image state. The frame body is always
validated and stored exactly like any other image. Session lifecycle headers
described below are interpreted only when a session ID is present (or on
lifecycle endpoints). No metadata or payload is logged or persisted.

The PNG must be exactly 64x64. Standard grayscale, RGB, palette, and alpha PNGs
are normalized to RGBA8; 16-bit samples are reduced to 8 bits. Alpha is preserved,
not converted into window transparency. Animated PNGs are rejected. Complete
chunks/checksums through IEND are required. Trailing bytes after a complete PNG
are tolerated, but still count toward the body limit. Descriptive text and ICC
profiles are ignored rather than decompressed. There is no color management.

Successful requests return **204 No Content**, empty body. For legacy requests,
the most recently published valid image wins, with no sender-order guarantee.
Session-aware publication additionally enforces stream sequence order. There is
no animation queue.

### Optional project name

Images may carry one `X-Emote-Project-Name: Mi%20t%C3%ADtulo` header, using the
existing sender's `encodeURIComponent` UTF-8 encoding. Decode exactly once;
`+` is literal, not a space. The raw resolved name is the `/emote title` override
or otherwise the captured working-directory basename, never a full path.

The decoded name must be nonblank, at most 128 UTF-16 units (the sender's existing
bound), without C0/DEL controls, path separators, or exact `.`/`..`. Preserve
nonblank folder whitespace and Unicode; limits are 384 decoded UTF-8 bytes and
1152 encoded ASCII bytes. Duplicate headers, malformed escapes/UTF-8, or invalid
names return 400 `invalid project name\n` without publishing or consuming sequence.
An absent header clears the name on an accepted image. Heartbeat/close ignore
this optional header and retain the accepted frame's name.

`latest().snapshots_with_names()` atomically pairs each unchanged `SessionSnapshot`
with its `Option<String>` name, including legacy and retained snapshots. Names
commit under the same post-decode stream/sequence/capacity guards as pixels and
state; rejected or delayed stale requests cannot rename a newer frame. The old
`snapshots()`, `get()`, `SessionSnapshot` and `Event` APIs remain unchanged.

### Session avatars and liveness

Add these headers to `/emote` to address an independent avatar:

```http
X-Emote-Session-Id: stable-session-id
X-Emote-Stream-Id: sender-stream-id
X-Emote-Sequence: 0
```

Each header must occur exactly once. Both IDs contain 1–128 ASCII characters from
`[A-Za-z0-9._:-]`. Sequence contains decimal digits only and fits an unsigned u64
(0 through 18446744073709551615). Without a session ID, `/emote` retains its original
single-slot semantics and ignores stream/sequence metadata.

Send bodyless `POST /emote/heartbeat` or `POST /emote/close` with the same version,
session, stream and sequence headers. These endpoints do not require Content-Type.
Send a heartbeat every **5 seconds**, including while the image is unchanged.
They never create image placeholders. Only a valid decoded frame can introduce a
session or take over an existing stable ID with a new stream.

| Event | Result |
| --- | --- |
| Valid frame | Updates only that session, renews its 30-second lease |
| Higher-sequence heartbeat for a known, nonclosed current stream | Renews lease; reactivates a timed-out avatar using its retained frame |
| Unknown heartbeat/close, stale sequence, closed or retired stream | 204, ignored; no lease, sequence, pixel or notification changes |
| Higher-sequence close for current stream | Closes that stream; it cannot reactivate within the replay horizon |
| New stream's valid frame for the same session | Takes over its existing ordered slot; retires the old stream |
| 30 seconds without accepted frame/heartbeat | Deactivates avatar; independent worker timer checks every 100 ms |

Sequence is shared across frame, heartbeat and close for each session's destination
stream. Publication rechecks sequence and stream state **after** PNG decoding under
the same mutex as lifecycle changes. Rejected requests never consume sequence.
Invalid bodies/PNGs still return errors even when their sequence would be stale.

`receiver.latest().snapshots()` returns `Vec<SessionSnapshot>` in stable first-seen
order. Each snapshot has `session_id: Option<String>`, `frame: Arc<Frame>`,
`active: bool` and `state: SessionState` (the sender-declared presentation of the
latest accepted frame: `Normal` or `Clock`). Active sessions appear together. If none remain, the final removed
avatar stays visible with `active == false`; a reactivated or new session replaces
that retained display. Legacy traffic has its own singleton key `None`, always
active once populated, ordered before session keys, and carries its own state
metadata. It coexists with session
avatars; it does not expire. Existing `latest().get()` consumers continue to see
the last accepted frame (legacy or session-aware), never lifecycle metadata, and
remain unchanged by close or timeout. The desktop uses `snapshots()` to render
neighboring avatars with independent textures; see the [architecture guide](architecture.md).

Repaint callbacks run outside the mutex. Accepted frames and visible lifecycle
changes notify; ordinary heartbeats and ignored requests do not. Snapshot pixel
Arcs remain valid after replacement or shutdown.

#### Sender-declared clock state

A session (or the legacy singleton) whose latest accepted frame carries
`X-Emote-State: clock` keeps its tile, order, lease and stream guards, but the
desktop paints its own internally generated 64x64 Lemmings pixel clock there
instead of the stored pixels: local date and 24h time glyphs plus a walking
Lemmings animation that advances every 500ms while time glyphs remain
second-stable. Any ordinary image state exits the clock for that session only.
The final-close fallback clock (below) renders the same artwork full-surface
without a decorative frame. Senders need no new endpoint or body change; unknown
state values keep working as ordinary images.

#### Bounded replay protection

At most **32 session-aware avatars** may be active. A request that would activate
a 33rd returns 429 without mutation. At most **64 known session records**, each
with one last frame, are retained. When full, admitting a new session evicts the
least recently communicating inactive record (ties use first-seen order); active
records are never evicted. Each record remembers the **16 most recently retired
stream IDs**, plus its current stream and sequence/closed state. Replacement of
an evicted identity or a stream older than that finite window can be accepted
again. There is no permanent ID ban or durable replay protection across restart.
Senders should use fresh stream IDs on restart. Inactive image retention is bounded
by the same 64-record cap, not an unbounded session history.

### Rejections

Application responses have `Connection: close` and
`Content-Type: text/plain; charset=utf-8`. Error bodies below end in one newline.
Policy precedence remains Origin, path, method, media type (frames only), version,
length, then required session headers, optional project name (images only), body,
PNG/empty-body validation, publication.

| Condition | Status | Exact body (before newline) |
| --- | --- | --- |
| Any Origin header, including `null` or local origins | 403 | `Origin is not allowed` |
| Not exactly `/emote`, `/emote/heartbeat`, or `/emote/close`, including query strings | 404 | `not found` |
| Method other than POST | 405 | `method not allowed` |
| Missing, duplicate, or wrong Content-Type | 415 | `expected image/png` |
| Missing, duplicate, or unsupported version | 400 | `expected X-Emote-Version: 1` |
| Declared or streamed body exceeds the limit | 413 | `body too large` |
| Body cannot finish within the deadline | 408 | `body timed out` |
| Malformed HTTP body framing surfaced to the handler | 400 | `malformed body` |
| Empty, invalid, truncated, animated, wrong-size, or decode-limit PNG | 422 | `expected a 64x64 PNG` |
| Missing, duplicate, or invalid required session/stream/sequence headers | 400 | `invalid session headers` |
| Invalid or duplicate image project-name metadata | 400 | `invalid project name` |
| Nonempty heartbeat/close body | 400 | `expected empty body` |
| Activation exceeds the active-session cap | 429 | `session limit reached` |

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
| Optional project name | 128 UTF-16 units, 384 decoded UTF-8 / 1152 encoded ASCII bytes | Matches the existing sender; at most one name per bounded record |
| Encoded body | 65,536 bytes, inclusive | Plenty for a raw 64x64 RGBA PNG; rejects large uploads, even chunked |
| Image dimensions | Exactly 64x64, checked immediately after IHDR | Rejects dimension bombs before dimension-dependent decoder allocations |
| PNG decoder budget | 256 KiB | Explicit `png::Limits` budget for decoder-managed allocations |
| Decoded output | At most 16,384 bytes, then exactly 16,384 RGBA bytes | Bounds caller-owned decode/output buffers separately |
| Headers | 32 fields, 8 KiB | Bounds HTTP parser/header storage |
| Concurrent connections/tasks | 8 | Excess accepted sockets are immediately closed; no application wait queue |
| Complete headers | 1 second | Bounds slow header writers |
| Complete body | 1 second from handler body-read start | Absolute deadline, not renewed for each chunk |
| Entire connection | 3 seconds from accept | Also bounds slow response readers |
| Stored session frames | At most 64 | One per bounded known-session record |
| Other frame references | Latest, legacy, and retained final display | At most three additional frame allocations, often sharing session pixels |
| Active sessions | 32, plus optional legacy singleton | Bounded avatar set |
| Retired streams | 16 per known session | Finite replay horizon |

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
