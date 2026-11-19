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

If Cargo is not on PATH on the development host, use the full path to the
installed `cargo` binary in place of `cargo` (for rustup installations this is
typically `~/.cargo/bin/cargo`). This project does not
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
| Right-click anywhere | Open a separate options window: avatar-size slider with five steps x1–x5 (the avatar's 64-point image renders at that many points per source texel, x2 initial) |
| Right-click → Transparent background | Checked initially at 45% transparency (opacity 55), with the slider enabled; uncheck to reset to opaque (0%) and disable the slider |
| Right-click → Background transparency | While checked, adjust background-only transparency: 0% opaque, 100% invisible; for this session only |
| Right-click → Background color | Expand the RGB picker to choose the background color (black unless saved); independent of transparency, saved globally for this desktop user |
| Right-click → No frame | Hide decorative frames and leave transparent 5 px separators; preserve the remaining background; checked by default, for this session only |
| Right-click → Vertical / Horizontal | Select Vertical or Horizontal stacking; Vertical is initial, for this session only |
| Right-click a card → Move up / Move down | Move that card one visible position in Vertical stacking; end positions are disabled |
| Right-click a card → Move left / Move right | Move that card one visible position in Horizontal stacking; end positions are disabled |
| Right-click a clock card → Close clock | Hide only this clock until its identity receives an ordinary image; the desktop keeps receiving |
| Right-click → Exit | Close the window and shut down the owned receiver |

Card order and clock dismissals last only while this desktop instance is open;
newly discovered cards append after existing cards, even when updates arrive.
Right-click targets the actual card under the pointer, not its index or name.
Clicks in separators, unused framed margins or the waiting surface show only
global controls. Global size, color, transparency and stacking controls still affect
all cards. Primary dragging still moves the window, never reorders cards.
Only clocks can be closed this way (session clocks, the legacy clock, or the
global fallback); ordinary avatar cards cannot be closed. Repeated clock images
and heartbeats do not reopen a dismissed clock. An ordinary image restores that
identity in its session-local order. Closing the last visible clock shows the
initial 192×192 waiting surface, without exiting or immediately restoring a retained
avatar/fallback. A dismissed global fallback stays hidden until that underlying
fallback ends; a new avatar is shown normally and a later fallback can appear again.
These controls do not close sender sessions or change the receiver protocol.

The options use an independent, opaque native window, so all controls remain
accessible even with a 128-pixel avatar; they are not clipped to the avatar's
surface. Selecting an option, pressing Escape, closing the options window, or
moving focus away after it has gained focus dismisses it. Exit closes the main
avatar window and its receiver, not just the options. Native popup positioning
and focus behavior depend on the window manager/compositor.

The canvas derives from the avatar's rendered image, never the reverse: the
slider step **x1–x5** is the points per source texel of the 64-point image
(x2 = 128, the initial step), the composition adds the image's one source
separation row (so the composition is 64×step wide and 65×step tall), and the
window adds only the margins that composition and the visible decorations
actually need. **Horizontal** rows stack `avatar count` tiles; **Vertical**
columns stack them along the other axis. With one or more avatars, an opaque
background and **No frame** unchecked, each tile is wrapped in the PNG
[cyberpunk frame](assets/README.md): its clear opening (851 of the 1100
artwork columns) contains the whole composition plus three logical points of
snapping slack, so the framed tile is `(1100, 1092) × (65×step + 3) / 851` and
the avatar still renders at the whole chosen multiplier. Checking **No frame**
(or any nonzero transparency) reclaims the frame margins and shrinks each tile
to exactly the composition. Unframed multi-avatar cards budget their real
four-point insets and five-point gaps around the composition instead of
deducting them from the image. At 100% display scale the avatar draws exactly
`64×step` square pixels in every mode; at fractional scale factors the
existing whole-physical-pixel policy applies (at least the chosen step's
physical level, never one less). One embedded texture is
reused across tiles with nearest-neighbor sampling. Transparent mode draws no frame and
automatically uses the compact unframed layout instead of retaining extra tile spacing.
Checking **No frame** reclaims the unframed image fit while keeping an opaque
background opaque. The available surface is divided into equal cards with
**5 logical pixels between adjacent card contours** along the selected axis.
Each PNG is fitted and top-compensated inside its own four-point-inset card;
the internal margin is not part of the transparent separator. The
separator strips are **always transparent**, even with an opaque background
behind the avatars. Cards use **8 logical pixel rounded corners**, including
the edges next to separators. The background and image draw mesh share that
outline, so opaque PNG corners cannot cover it. Cached PNG pixels and label
layout are unchanged. Window dimensions follow the framing choice: framed tiles
add the frame margins around the composition, unframed tiles shrink to the
composition. A framed single avatar
uses the same clear-opening inset as a framed multi-avatar tile; **No frame**
retains its borderless fit. Nonzero transparency hides frames, independently of that
checkbox; changing transparency does not reset the **No frame** selection.
The checkboxes, background transparency and stacking selection reset on restart.
Only the background RGB color persists across restarts and projects for this desktop user.
Avatars start at **x2**, with **45% background transparency**, **No frame**
checked and **Vertical** stacking; no decorative frame is shown by default. The waiting surface has no decorative frame before an image arrives and keeps
its native 192×192 initial square at every slider step; only actual avatars or
the clock fallback derive computed content sizes (the clock draws the
composition of the chosen step). Physical dimensions also follow the OS display scale. Many
sessions can exceed the monitor's width or height; scrolling, wrapping, and other
overflow policies are outside this change. The desktop adds one source row of
background-colored top padding above the complete image. The complete 64×64 image is
uniformly fitted within a virtual 64×65 composition, centered in each tile
without cropping or changing the PNG, using the largest whole number of
physical pixels per source pixel at the display's current scale factor, with
every edge snapped to the physical pixel grid: pixels always stay square, and
the image may use larger margins rather than a fractional scale. Only surfaces
smaller than 64×65 physical pixels fall back to a fractional contain fit.
Nearest-neighbor filtering keeps texel edges sharp. The background is
**black at 45% transparency by default** (opacity 55), with
**Transparent background** checked and its slider enabled.
After unchecking it, checking it again enables **Background transparency** at 45% (opacity 55).
**Background color** expands a native RGB picker inside the options window;
collapse it again to return to the compact menu. The selected color applies to
the waiting surface, avatar cards, session clocks and global fallback clock.
The existing transparency slider applies independently to that color, including
when transparency is disabled (fully opaque). Black is used when no valid saved color
is available. The selection is saved immediately when RGB changes; save failures
retain the live color without interrupting the app. Both controls change only the background;
received image pixels and their alpha, embedded labels, clock artwork and the
opaque options menu are unchanged. This desktop-only setting does not configure
the sender or any HTML/serial LED display. Intermediate transparency retains the 8-pixel rounded background
and image contour, including with a single avatar. The checkbox stays checked
at 0% and partial values. Unchecking resets to 0% transparency (fully opaque),
not a remembered partial value. Any opacity below 100%
automatically omits the frame and uses the compact layout with 5-pixel clear
gaps, without changing the saved **No frame** preference for fully opaque mode.
PNG alpha blends over the desktop subject to native compositor support. Original image pixels are not changed: opaque black
pixels (including the sender's bottom gap) stay black; this is not black-color
removal. On Windows, any background opacity below 100% disables the native undecorated
window shadow and its thin perimeter; only 100% restores the opaque shadow
policy. Native visual verification of shadow removal
is still required. The choice is session-only and
resets on restart. There is no click-through, general window/settings persistence, background
service, installer, or automatic sender configuration.

The dedicated color file is `%APPDATA%/gentle-emote-desktop/background-color.txt`
on Windows, `$XDG_CONFIG_HOME/gentle-emote-desktop/background-color.txt` on Linux
(or `$HOME/.config/gentle-emote-desktop/background-color.txt` when XDG is unset),
and `$HOME/Library/Application Support/gentle-emote-desktop/background-color.txt`
on macOS. Roots must be absolute and nonempty; an invalid configured root disables
persistence, never falling back to the current project. The strict format is lowercase
`#rrggbb` with an optional single LF newline; saves include that newline. Startup
reads at most nine bytes and uses black for missing, malformed, oversized or
unreadable files, without repairing them. Only an actual RGB change creates the
parent directory and writes this file. Writes are simple overwrites, not guaranteed
atomic; simultaneous desktop instances use last-writer-wins. No other settings or
sender/LED files are read or saved. Library/embedding `EmoteApp::new(receiver)`
remains filesystem-free; only the native entry point opts in by supplying the path.

## Session compatibility

Session-aware frames use `X-Emote-Session-Id` plus stream/sequence headers on the
existing version-1 PNG endpoint. Distinct sessions initially appear in first-seen
order, then follow any session-local card moves; updates affect only their own tile. Closing or timing out a session removes
its tile while others remain. Explicitly closing the last active session, with
no legacy avatar present, immediately replaces its display with the internally
rendered Lemmings pixel clock: the same 64x64 artwork as the sender display
(local `DD/MM/YYYY` and 24h `HH:MM:SS` glyphs, walking Lemmings on a green path).
The desktop renders that clock internally, so senders activate it per session
with `X-Emote-State: clock` on the ordinary image endpoint; any ordinary image
state (or absent/unknown state value) restores the received pixels. An idle
session's clock tile never hides active neighbors, honors the selected frame,
and alternates its header every five seconds between the local date and its
sender-resolved `/emote title` override or folder basename, via the existing
`X-Emote-Project-Name` header. Display names are normalized to ASCII uppercase before counting/truncation;
metadata and session identity keep their original case. Names longer than 16 characters show the first 13
plus three static dots, never scrolling. IDs remain identity only; missing names
and the global fallback keep the date. Renames refresh within the same animation
tick without replacing texture handles or changing avatar layout. The compact
bitmap alphabet is unchanged: unsupported characters reserve blank cells.

Unframed clocks use the same allocated card width and height as equivalent
avatars, including the four-point insets in multi-tile groups and five-point
separators. Backgrounds fill those cards, not just the snapped composition;
rounded card backgrounds retain the existing opacity policy, with no underlay
at zero. Clock artwork uses a complete image quad over the background: every
source pixel, including the bottom path ends, remains visible without corner
cropping. Avatar masking is unchanged. Ordinary, mixed and all-clock groups use
the same canvas, insets and pixel multiplier. At slider step `s`, `n > 1`
unframed tiles request `(n×(64s+8)+(n−1)×5, 65s+8)` horizontally or
`(64s+8, n×(65s+8)+(n−1)×5)` vertically. Single unframed clocks and the global
fallback use the full allocated surface for the background and the same 64×65
image fit as a single unframed avatar. The selected scale never changes. Opaque clocks still use the selected decorative
frame unless **No frame** is checked; the global fallback remains unframed.

The surface repaints every 500ms while any clock is visible (walkers advance
each 500ms tick; time glyphs stay second-stable). The final-close fallback clock
renders without a frame, as before. A new or reactivated session replaces
either clock; drag and options remain available. Startup still shows Waiting, and
timeout of the last session still retains its image (a delayed close after timeout
does not enable the clock; timeout alone never starts the fallback clock). Senders
heartbeat every 5 seconds; accepted frames/heartbeats renew a 30-second lease.
Heartbeats never upload image pixels. Legacy frames without a session ID still
update one nonexpiring singleton avatar, which can coexist with session-aware
tiles and may also declare `X-Emote-State: clock`. See the [protocol](docs/protocol.md)
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

The default suite contains **49 tests** (19 receiver, 4 session lifecycle,
25 headless window, and 1 private shadow-policy unit test). The earlier popup
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
