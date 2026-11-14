# Gentle Emote Desktop

A small local Rust window for the existing Gentle Emote PNG sender. It listens on
**127.0.0.1:8001**, keeps the latest valid 64×64 frame per session, and displays
neighboring pixel-art avatars in one frameless, always-on-top window. The simulator
can keep port 8000.

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
| Right-click anywhere | Open a separate options window: per-avatar size 128, 192 (initial), 256, or 384 logical pixels |
| Right-click → Transparent background | Toggle desktop transparency; off by default, for this session only |
| Right-click → Exit | Close the window and shut down the owned receiver |

The options use an independent, opaque native window, so all controls remain
accessible even with a 128-pixel avatar; they are not clipped to the avatar's
surface. Selecting an option, pressing Escape, closing the options window, or
moving focus away after it has gained focus dismisses it. Exit closes the main
avatar window and its receiver, not just the options. Native popup positioning
and focus behavior depend on the window manager/compositor.

Each avatar occupies a square tile; the window is one horizontal row with width
`avatar count × chosen size` and height `chosen size`. With multiple avatars and
an opaque background, each tile uses the supplied [cyberpunk PNG frame](assets/README.md)
instead of a beveled border. The complete avatar and labels fit within its clear
opening; one embedded texture is reused across tiles with nearest-neighbor sampling.
At 128 points the avatar image is approximately 92 points wide, so the artwork
has more space than the old thin border. Transparent mode draws no frame and
keeps its previous inner margin of up to four logical pixels and image fit.
A single avatar keeps its original
borderless layout. Startup waits in one
192-pixel square. Physical dimensions also follow the OS display scale. Many
sessions can make the row wider than the monitor; scrolling, wrapping, and other
overflow policies are outside this change. The desktop adds black top padding
equivalent to five source rows,
balancing the sender image's existing five-row black bottom gap. The complete
64×64 image is uniformly fitted within a virtual 64×69 composition, centered in
each tile without cropping or changing the PNG. Nearest-neighbor filtering
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

## Session compatibility

Session-aware frames use `X-Emote-Session-Id` plus stream/sequence headers on the
existing version-1 PNG endpoint. Distinct sessions appear in stable first-seen
order; updates affect only their own tile. Closing or timing out a session removes
its tile while others remain. The last removed avatar retains its image until a
new or reactivated session replaces it. Senders heartbeat every 5 seconds; accepted
frames/heartbeats renew a 30-second lease. Heartbeats never upload image pixels.
Legacy frames without a session ID still update one nonexpiring singleton avatar,
which can coexist with session-aware tiles. See the [protocol](docs/protocol.md)
for exact headers, limits, finite replay protection, and lifecycle rules.

## Learn and verify

| Guide | Use it for |
| --- | --- |
| [Architecture and Rust study path](docs/architecture.md) | Data flow, ownership/Drop, snapshots, thread boundaries, texture reuse, and reading exercises |
| [Protocol and local trust boundary](docs/protocol.md) | Exact headers, PNG formats, rejections, limits, and local-process security assumptions |
| [Testing and evidence limits](docs/testing.md) | The headless suite, explicit native checks, TCP reset helper, and outstanding manual/sender/platform checks |

Quick developer checks:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

The default suite contains **46 tests** (19 receiver, 4 session lifecycle,
22 headless window, and 1 private shadow-policy unit test). The earlier popup
update passed its 14 window tests, the shadow policy, and all 4 lifecycle tests.
The frame update adds alpha/artwork checks and replaces the bevel geometry
checks with frame-opening, texture reuse, and transparent/single-avatar checks. The
full-suite attempt failed one receiver check with Windows `ConnectionAborted`
(OS 10053); it was not rerun. See the testing guide for the recorded limits.
The separate macOS native startup test is ignored by default. The testing guide documents its
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

## Runtime presentation controls

The runtime-only **No frame** option keeps the background setting independent,
omits the artwork and packs avatar images with five logical pixels of
transparent separation. Fully transparent backgrounds also use this compact
layout. The source PNGs and original artwork are unchanged.

**Vertical / Horizontal** provides exclusive stacking choices, initially
Horizontal. Each selection preserves the chosen per-avatar size, cached
textures, frame preference and clear five-point separators. The root window
resizes to the selected axis; there is no wrapping.

Unframed multi-avatar cards use an eight-point rounded contour shared by the
background and image mesh. Convex draw-time clipping prevents opaque lower
image corners from covering the cutouts. PNG pixels are not rewritten. This
does not correct the separately reported per-card top-padding inconsistency.

Background opacity adjusts only the background from 0 to 100 percent,
initially 100. PNG pixels, labels and the options menu are not faded.
Transparent background selects zero and restores the last nonzero percentage
when unchecked. Every percentage below 100 uses unframed packing and disables
the Windows native shadow. Settings reset on restart. The independent No frame
preference is preserved for fully opaque mode.
