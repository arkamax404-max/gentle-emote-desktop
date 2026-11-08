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

A busy port or native window/backend startup failure is reported on stderr with
a nonzero exit; there is no silent fallback port. Exact OS error codes and text
are host-specific.

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

## Learn and verify

| Guide | Use it for |
| --- | --- |
| [Architecture and Rust study path](docs/architecture.md) | Data flow, ownership/Drop, snapshots, thread boundaries, texture reuse, and reading exercises |
| [Protocol and local trust boundary](docs/protocol.md) | Exact headers, PNG formats, rejections, limits, and local-process security assumptions |
| [Testing and evidence limits](docs/testing.md) | The 20-test suite, explicit native checks, TCP reset helper, and outstanding manual/sender/platform checks |

Quick developer checks:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

The default suite has **20 tests** (14 receiver, 6 headless window). The separate
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
