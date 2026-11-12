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
change PATH, install system packages, or configure the sender globally.
Target `http://127.0.0.1:8001/emote`; see the [wire protocol](docs/protocol.md).
The local Gentle Emote fork has no sender CLI; a bounded integration used its
production `ExternalDisplay` API with an explicit per-run loopback destination.
Two distinct composed PNGs reached the production desktop receiver with HTTP 204.
See the [recorded evidence and manual checklist](docs/testing.md) for scope and
limits; no sender source or global destination was intentionally changed.

A busy port or native window/backend startup failure causes a nonzero exit;
there is no silent fallback port. Errors are reported on stderr where available.
The Windows executable uses the GUI subsystem, so double-clicking it does not
open a console window; startup errors are not visible in a companion console.
Exact OS error codes and text are host-specific.

| Interaction | Result |
| --- | --- |
| Start with no sender | Readable waiting message and control hints |
| Primary-button drag | Ask the OS to move the window |
| Right-click anywhere | Size menu: 128, 192 (initial), 256, or 384 logical pixels |
| Right-click → Transparent background | Toggle desktop transparency; off by default, for this session only |
| Right-click → Exit | Close the window and shut down the owned receiver |

Window sizes remain square; physical dimensions also follow the OS display
scale. The desktop adds black top padding equivalent to five source rows,
balancing the sender image's existing five-row black bottom gap. The complete
64×64 image is uniformly fitted within a virtual 64×69 composition, centered in
the window without cropping or changing the PNG. Nearest-neighbor filtering
keeps texel edges sharp. The background is **opaque black by default**. Enable
**Transparent background** in the right-click menu to make the added padding and
unused surface transparent and blend PNG alpha over the desktop, subject to
native compositor support. Original image pixels are not changed: opaque black
pixels (including the sender's bottom gap) stay black; this is not black-color
removal. On Windows, transparent mode also disables the native undecorated
window shadow and its thin perimeter; disabling the option restores black and
the opaque window's shadow policy. Native visual verification of shadow removal
is still required. The choice is session-only and
resets on restart. There is no click-through, persistent state, background
service, installer, or automatic sender configuration.

## Learn and verify

| Guide | Use it for |
| --- | --- |
| [Architecture and Rust study path](docs/architecture.md) | Data flow, ownership/Drop, snapshots, thread boundaries, texture reuse, and reading exercises |
| [Protocol and local trust boundary](docs/protocol.md) | Exact headers, PNG formats, rejections, limits, and local-process security assumptions |
| [Testing and evidence limits](docs/testing.md) | The 24-test suite, explicit native checks, TCP reset helper, and outstanding manual/sender/platform checks |

Quick developer checks:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

The default suite defines **24 tests** (14 receiver, 9 headless window, and
1 private shadow-policy unit test). The separate
macOS native startup test is ignored by default. The testing guide documents its
explicit invocation and the graphical `native_smoke` example.

## Platform status

Eframe/egui **0.37.0** uses the Glow renderer; Linux X11 and Wayland source
backends are enabled. macOS, Windows, and Linux are **source/build targets**, not
a claim of tested execution on all three. A graphical session and suitable
native graphics/toolchain libraries are required.

The native framebuffer smoke passed on the macOS development host using a
protocol-compatible test sender, including HTTP 204 and port release. This is
not human visual confirmation of topmost, drag, or menu behavior. Real sender
transport was separately exercised; per-real-frame framebuffer capture was not.
Human window checks and Windows/Linux execution remain outstanding. On Wayland,
topmost placement and window moves depend on compositor support/policy.
