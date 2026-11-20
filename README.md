# Gentle Emote Desktop

A small local Rust window for the existing Gentle Emote PNG sender. It listens on
**127.0.0.1:8001**, keeps the latest valid 64×64 frame per session, and displays
neighboring pixel-art avatars in one frameless, always-on-top window. The simulator
can keep port 8000.

## Run

With Rust **1.95 or newer**, Cargo, a native compiler/linker, and a graphical
session available, run from this directory. The production renderer is WGPU on
macOS and Glow on Windows/Linux; no renderer flag is needed:

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

### Sender transparency (optional, per desktop target)

For senders supporting `transparentBackground`, add this field to the existing
HTTP desktop target for `http://127.0.0.1:8001/emote`:

```json
"transparentBackground": true
```

Reload the sender after changing its target configuration. The default is
`false`: the sender flattens PNG pixels to alpha 255. Opting in preserves the
source RGBA, including opaque black pixels; it does **not** turn an arbitrary
opaque PNG or its black background transparent. The desktop's background slider
only changes its own underlay, not incoming image pixels. This option belongs
to that desktop target, not a global transparency setting or an LED target.

A busy port or native window/backend startup failure causes a nonzero exit;
there is no silent fallback port. Errors are reported on stderr where available.
The Windows executable uses the GUI subsystem, so double-clicking it does not
open a console window; startup errors are not visible in a companion console.
Exact OS error codes and text are host-specific.

| Interaction | Result |
| --- | --- |
| Start with no sender | Readable waiting message and control hints |
| Primary click on an active session card | Queue a focus request for that session's Herdr TUI panel; a compatible sender receives it on its next accepted frame or idle heartbeat |
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
| Right-click a card titled exactly `TMP` → Close | Hide only this card (avatar or clock) until its session title changes from `TMP`; images and heartbeats do not reopen it |
| Right-click → Exit | Close the window and shut down the owned receiver |

A focus click requires press and release on the same active session card without
dragging; session clock cards count too. It queues on release, never on press.
Legacy cards without a session ID, global fallback clocks, inactive retained
cards, separators and unused framed margins do nothing. Each click replaces that
session's pending request with a new ID. The sole optional focus response header
is `X-Emote-Focus-Request-Id`. Without an ACK, the same ID is redelivered on every
accepted frame or heartbeat POST for that current session stream until 15 seconds
from the click; close, lease expiry, stream takeover, eviction and shutdown clear
it. The sender sends `X-Emote-Focus-Ack` only after confirmed Herdr focus (successful
Promise, CLI exit 0 and JSON `agent_info` for the expected pane with `focused: true`),
and deduplicates without retrying failed IDs. The desktop does not focus Windows Terminal or select
emulator tabs itself. Old senders may ignore the optional response headers;
HTTP success remains 204 with no body. See the [focus wire contract](docs/protocol.md#desktop-focus-requests-optional-response-headers).

Card order and clock dismissals last only while this desktop instance is open;
newly discovered cards append after existing cards, even when updates arrive.
Right-click targets the actual card under the pointer, not its index or name.
Clicks in separators, unused framed margins or the waiting surface show only
global controls. Global size, color, transparency and stacking controls still affect
all cards. Primary dragging still moves the window, never reorders cards.
Ordinary avatar cards cannot be closed, except when their sender-provided session
title is exactly `TMP` (case-sensitive, not an ID, prefix or substring). These
cards always offer **Close**, including in clock mode and for legacy senders
without an ID. Closing one hides only that identity locally while its observed
title remains `TMP`, across ordinary images, clock images and heartbeats. A title
change (including missing title) restores it, even in clock mode; restarting the
desktop resets the dismissal. The raw title metadata controls this rule, not the
clock's temporary stale-session display text.

Other clocks offer **Close clock** (session clocks, the legacy clock, or the
global fallback). Repeated clock images
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

## macOS app and ZIP

Local packaging is the default. Prerelease packaging adds clean-source and
validated dependency-notice gates; neither mode publishes or launches anything.

### Local build

On macOS, with Python 3, the Rust toolchain above, and Apple's command-line tools
(`ditto`, `file`, `lipo`, `otool`, `codesign`) installed, run from this repository:

```sh
python3 scripts/package_macos.py
# Optional bundle-version override; does not change Cargo.toml:
python3 scripts/package_macos.py --package-version 0.1.4
```

This builds with `cargo build --release --locked` for the **current process's
native architecture** (arm64 or x86_64, verified with `file` and `lipo`), not a
universal binary or cross-build by default. An x86_64 Python under Rosetta
produces an x86_64 artifact. Cargo is
resolved from PATH, then `~/.cargo/bin/cargo`. Build intermediates stay under
`target/macos-package/build/`.

### Universal2 (opt-in)

With both `x86_64-apple-darwin` and `aarch64-apple-darwin` Rust targets already
installed and a suitable Apple SDK, build both locked release targets and merge
them with `lipo`:

```sh
python3 scripts/package_macos.py --universal --package-version 0.1.4
```

The packager never installs targets or tools. It inspects each thin Mach-O before
merging, verifies exactly the two expected slices independently of slice order,
and checks dependencies separately for each slice of the merged executable.
Universal2 outputs use `universal2` instead of the native architecture in their
names, so they do not overwrite native artifacts. Bundle Info.plist metadata is
the same as native packaging.

Merging may invalidate thin signatures. The merged executable's actual signature
is inspected and verified across both architectures. An unsigned or invalid merged
signature stops packaging with an actionable error, without exporting an app or
ZIP. Apple Silicon needs valid code integrity. **Only after explicit approval**,
opt in to ad-hoc signing of the **complete newly generated app**, after its
Info.plist, receipt, LICENSE and other resources are assembled:

```sh
python3 scripts/package_macos.py --universal --ad-hoc-sign --package-version 0.1.4
```

`--ad-hoc-sign` requires `--universal`; it invokes `codesign --force --sign -`
only on the complete tool-generated app, never on a thin input, an existing
bundle, or a running app. It verifies the app and all executable architectures
before archiving and again after export. Nothing inside the app is modified
after signing. It is **not Developer ID signing or notarization**.
It is never automatic. A failure still reserves its output directory; a retry
with the same source/version needs a fresh reservation (existing output paths are
not reused or removed by the script).

Each local run reserves a new directory under
`target/macos-package/gentle-emote-desktop-<version>-<arch>-local-<revision>-<dirty|clean>/`
containing `Gentle Emote Desktop.app`, its attribute-preserving ZIP, a
`.zip.sha256` checksum sidecar and an external `signature-verification.json`. Existing directories are never overwritten or
automatically deleted. A failed run may leave an incomplete reserved directory;
only a successful exit reports final artifacts. The script neither opens the app
nor changes an already running instance. Close any other listener on port 8001
before manually opening the bundle.

**These are local, not official release artifacts.** Cargo currently declares
`0.1.0`; repository release labels such as `v0.1.4` do not silently change the
bundle version. `--package-version` accepts one to three numeric components
(major 0–9999, minor/patch 0–99), and changes only packaging metadata. The receipt
in `Contents/Resources/packaging-receipt.json` records both versions, source SHA,
dirty state (including untracked files), architecture, dependencies and
**pre-signing binary evidence**. It also records native/Universal2 mode, all
architectures and Rust targets, signing intent, and the static-only validation
scope. Universal receipts include thin inspections and the merged signature
before requested signing. The `file` fields contain portable Mach-O type and
validated architecture labels, never raw `file` diagnostics (which may include
host-local slice paths). Final observed signature evidence is external:
embedding it after signing would invalidate the bundle's resource seal. Dependencies outside Apple's `/System/Library/` and `/usr/lib/`
are rejected, including unresolved `@rpath` references.

No Developer ID signing, notarization, publication or installation is performed.
The native executable may retain a linker-generated **ad-hoc signature**; the
Universal2 app may be explicitly ad-hoc signed as above. Neither is
Developer ID signing. Gatekeeper may block downloaded copies; this script does
not bypass security controls. The bundle includes the root LICENSE and a short
local-build notice, **not a complete third-party license audit in local mode**.
Prerelease mode below requires validated dependency notices. No custom icon,
minimum macOS version guarantee, or universal runtime compatibility is claimed.
Universal2 describes CPU slices, not a minimum supported OS or GPU guarantee.
Packaging and CLI tests do not launch the app or validate ARM runtime behavior;
manual checks on Intel and Apple Silicon remain separate from static inspection.
The renderer and Windows packaging/build behavior are unchanged.

### Clean-source Universal2 prerelease

Use a **new clean committed worktree** of the intended source revision, with both
Darwin Rust targets installed. Generate the audit into a new directory outside
the source tree (its parent must already exist), then package from that worktree.
Before packaging, explicitly select `RUSTC_WRAPPER`: export the intended wrapper
executable path, or export an empty value to choose no wrapper. This choice
explicitly overrides Cargo's configured wrapper. Do not select an empty value
unless disabling any configured wrapper is intended.

```sh
python3 -B scripts/audit_macos_licenses.py --output /tmp/emote-v0.1.5-rc.1-notices
python3 -B scripts/package_macos.py --universal --ad-hoc-sign \
  --release-tag v0.1.5-rc.1 --license-audit /tmp/emote-v0.1.5-rc.1-notices
```

`--release-tag v0.1.5-rc.1` maps to numeric bundle version **0.1.5** without
changing Cargo's version. A conflicting `--package-version` is rejected.
Release mode requires Universal2, explicit ad-hoc signing and a validated audit;
`--license-audit DIR` is not accepted in local mode. HEAD supplies the source SHA,
not a caller-provided revision. The packager requires clean tracked **and
untracked** source at entry and exit (including `.pi/` if present); it never
silently ignores development state. It checks HEAD and Cargo.toml/Cargo.lock
hashes again before export and at completion. A failure at any stage blocks
release use, even if an incomplete output reservation remains.

Prerelease builds remap compiler source paths: Cargo home to `/cargo`, the frozen
workspace to `/source`, and build intermediates/system temporary paths to `/build`.
This is rustc `--remap-path-prefix`, not post-build binary stripping or a change
to dependency files or license payloads. With `RUSTC_WRAPPER` unset, release
packaging fails before tool calls or output reservation: a config-only wrapper
cannot be transparently preserved. A temporary compiler shim chains the exact
explicitly selected wrapper executable (including paths with spaces), or no
wrapper when explicitly empty, and appends remaps to Cargo's resolved arguments.
`RUSTC`, `RUSTC_WORKSPACE_WRAPPER`, `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS`
(including empty values) remain unchanged; Cargo's compiler-flag resolution is
not replaced. Local builds neither require this selection nor enable these
remaps. Fresh release builds and separate artifact privacy inspection remain
required: fake-toolchain tests do not prove a real binary contains no host paths.

The audit validator binds notices to the source commit, both resolved Darwin
closures, Cargo manifest/lock hashes and exact notice contents. Notices are
copied to `Contents/Resources/ThirdPartyNotices` and validated again there before
signing. The embedded prerelease receipt records the exact source commit,
`source_dirty: false` and a portable manifest hash/package/file-count summary.
Only successful destination validation establishes `notice_inventory_complete`;
it does **not** establish a complete legal-license audit. The receipt and external
`signature-verification.json` preserve the validator's exception records and
separate inventory/legal status. A copied summary must match the original,
including exceptions, before the app is signed.

**Public RC license exception — dispatch 0.2.0:** its exact upstream Cargo.toml
declares MIT, but no original copyright/license notice is available. The human
approved this prerelease with that explicit exception. The notice inventory
includes the original declaration and a clearly labeled standard MIT reference
with exact provenance; the reference is **not dispatch's original notice**, and
no copyright holder or year is invented. The bundle's README.txt and embedded
receipt disclose the exception before final signing; the external verification
receipt records the same exception after signing. Both
`legal_audit_complete` and `third_party_license_audit_complete` remain **false**;
validated notice coverage is not a legal guarantee.
**Public RC historical MIT exceptions — ten current crates:** block2 0.6.2,
objc2 0.6.5, objc2-encode 4.1.0, dispatch2 0.3.1, and
objc2-{app-kit,core-foundation,core-graphics,io-surface,metal,quartz-core} 0.3.2
retain their exact upstream link-only declarations alongside the authentic
[historical MIT notice](https://raw.githubusercontent.com/madsmtm/objc2/757cd841d0293341dbefd9b99d7548dec244ffde/LICENSE.txt).
Original bytes and `Copyright (c) Steven Sheldon` attribution are unchanged.
The human accepted prerelease publication **with uncertain applicability to these
current crates**, not complete legal clearance. MIT is explicitly selected as a
declared alternative only within these validated exceptions; it is not asserted
to cover Zlib. Historical provenance remains separate from exact-current
provenance, and applicability verification remains **false**. The bundled README,
notice manifest and embedded/external receipts disclose all eleven exceptions
(including the separate dispatch exception); legal/full-audit flags remain false.
See the [supplement contract](assets/macos-license-supplements/README.md).

Outputs use
`target/macos-package/gentle-emote-desktop-v0.1.5-rc.1-universal2-<revision>/`,
not the local naming scheme, and never overwrite earlier outputs.

**Release verification remains separate:** verify both the exported app and a
fresh ZIP extraction using `codesign --verify --strict --all-architectures APP`
and the same command on `APP/Contents/MacOS/gentle-emote-desktop`. The external
signature receipt covers export only, not ZIP extraction. Intel runtime smoke
and native Apple Silicon runtime/permissions checks are separate manual steps;
**ARM runtime remains untested** (`arm_runtime_validated: false`). No minimum
macOS version or GPU compatibility is promised. Downloaded ad-hoc apps can be
blocked by Gatekeeper: users must assess trust and use macOS's manual
Privacy & Security approval flow if appropriate. No quarantine removal or other
security bypass is automated or recommended here. Publication, Developer ID
signing and notarization are not performed. Windows releases are unchanged.

CLI and fake-toolchain checks (no real Cargo build, signing or app launch):

```sh
python3 -B -m unittest discover -s tests -p 'test_package_macos.py'
```

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

The default suite contains **120 tests** on Windows: 34 receiver, 56 headless
window, 10 session lifecycle, 3 background-color store, 9 idle-clock image, and
8 library unit tests (1 shadow-policy, 1 receiver display, 6 session helpers).
The macOS-only native startup test is ignored by default and counted separately.
See the [testing guide](docs/testing.md) for the exact breakdown, recorded
limits, and the graphical `native_smoke` example.

## Platform status

Eframe/egui **0.37.0** uses WGPU on macOS and Glow on Windows/Linux;
Linux X11 and Wayland source backends are enabled. WGPU is enabled by default
only for the macOS target. The optional `diagnostic-wgpu` feature still enables
WGPU for the transparency probe on other targets; it does not change the
production renderer selection. A graphical session and suitable native
graphics/toolchain libraries are required.

| Platform | Compatibility and evidence |
| --- | --- |
| **macOS — locally tested and compatible** | Production WGPU build and native framebuffer smoke passed (HTTP 204, expected RGB, clean close/port release). After relaunch with per-target sender `transparentBackground: true`, the human confirmed visible transparency: “ok, va perfecto ahora”. Native overlay shadows are disabled to avoid ghosting; that alone does not fix opaque sender pixels. |
| Windows | Existing Glow source/build target, with historical Windows build, headless-test and native-smoke evidence in the testing guide; not executed in this session. |
| Linux | Existing Glow source/build target with X11/Wayland enabled; not executed in this session. On Wayland, topmost placement and window moves depend on compositor support/policy. |

Local macOS acceptance is not a universal minimum macOS version, graphics-device
or display-scale guarantee, nor proof of every manual window check. The earlier
Glow smoke and real sender transport checks remain separate historical evidence;
framebuffer alpha alone is not proof of visible compositor transparency. See the
[testing guide](docs/testing.md) for results and remaining checks.
