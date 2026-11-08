# Gentle Emote Desktop

A small local Rust window for the existing Gentle Emote PNG sender. It listens on
**127.0.0.1:8001**, keeps only the latest valid 64×64 frame, and displays pixel art
in a frameless, always-on-top square. The simulator can keep port 8000.

## Run

With Rust **1.95 or newer**, Cargo, a native compiler/linker, and a graphical
session available, run from this directory:

```sh
cargo run
```

If Cargo is not on PATH on the development host, use
`/Users/santiagoperez/.cargo/bin/cargo` in place of `cargo`. This project does not
change PATH, install system packages, or configure the sender globally. Configure
an existing sender explicitly to POST to `http://127.0.0.1:8001/emote`; see the
[wire protocol and local trust boundary](docs/protocol.md). A busy port or native
window/backend startup failure is reported on stderr with a nonzero exit; there
is no silent fallback port.

| Interaction | Result |
| --- | --- |
| Start with no sender | Readable waiting message and control hints |
| Primary-button drag | Ask the OS to move the window |
| Right-click anywhere | Size menu: 128, 192 (initial), 256, or 384 logical pixels |
| Right-click → Exit | Close the window and shut down the owned receiver |

Sizes are integer multiples of the 64-pixel image; physical dimensions also
follow the OS display scale. Nearest-neighbor filtering keeps texel edges sharp.
PNG alpha is blended inside an **opaque window**, not converted into desktop
transparency. There is no click-through, theme/settings UI, persistent state,
background service, installer, or automatic sender configuration.

## Study the ownership path

1. [`src/main.rs`](src/main.rs) creates the window and binds the fixed local port.
   Its network callback holds only an egui context handle and requests repaint.
2. [`src/receiver.rs`](src/receiver.rs) validates bounded HTTP/PNG input on one
   owned worker. It alone writes a single latest-frame slot, then notifies egui
   outside the mutex. Rejections preserve the last valid image.
3. [`src/app.rs`](src/app.rs) reads `Option<Arc<Frame>>` snapshots. `None` means
   waiting; `Arc` shares immutable pixels cheaply. Pointer identity detects a new
   publication, so unchanged frames never trigger another texture upload. The
   app retains the receiver for clean shutdown and owns its reusable GPU texture.

There is no GUI polling timer or pending-frame queue. Eframe redraws on input,
window events, or receiver notifications; a burst may coalesce into one redraw.
Teaching comments explain the conversion from unpremultiplied RGBA, nearest
filtering, snapshot lifetimes, and native viewport commands.

## Verify and platform limits

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo run --example native_smoke
```

Headless tests exercise real ephemeral-loopback HTTP, native option values,
painted waiting/menu text, resize/exit/drag commands, and exact texture upload
pixels/filtering/replacement, burst coalescing, and app-owned receiver shutdown
with an incomplete client. They do **not** prove OS window behavior.

On a **macOS graphical host only**, with **127.0.0.1:8001 free initially**, also run:

```sh
cargo test --test startup -- --ignored
```

This explicitly ignored native test occupies port 8001, launches the production
executable, and checks exit code 1, empty stdout, and the exact macOS address-in-use
stderr. It fails rather than skipping if the port is already occupied, bounds the
child's lifetime to 20 seconds, kills/reaps it on failure, and checks port release.
It is ignored by default because ordinary headless CI must not launch a window;
its exact OS error assertion is macOS-only.

The smoke example launches the reusable real app on the main thread, sends one
known PNG over TCP to an ephemeral port, and checks the rendered framebuffer's
center pixel via eframe's own capture (not an OS screenshot/permission API). It
then closes and checks port release. A 20-second process watchdog fails nonzero
if native startup or rendering stalls; normal completion joins its threads. This
is native pipeline evidence, not visual confirmation of topmost or drag behavior.

Eframe/egui **0.37.0** with the Glow renderer is used (no default persistence
features); Linux X11 and Wayland source backends are enabled. macOS, Windows, and
Linux are **source/build targets**, not a claim of tested execution on all three.
The native smoke passed on the macOS development host, including HTTP 204,
nearest-texture metadata, framebuffer RGB(42,99,151), and port release. This is
not a human visual inspection. Windows/Linux execution and manual cross-platform
window-behavior checks remain outstanding. A graphical session
and suitable native graphics/toolchain libraries are required. On Wayland,
always-on-top placement and moving windows depend on compositor support/policy.
