# Verify behavior without overstating evidence

Start with the headless suite, then use native checks only in a suitable graphical
session. Socket tests, framebuffer capture, human window inspection, and a real
external sender each establish different things. The real sender transport check
below passed; it did not capture each real frame's framebuffer. Windows/Linux
execution and human visual OS-behavior checks remain outstanding.

## Default developer checks

Run from the repository root with Rust 1.95 or newer and a native compiler/linker:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

If Cargo is not on PATH on the development host, substitute
`/Users/santiagoperez/.cargo/bin/cargo` for `cargo`. These instructions do not
install tools, modify PATH, or change sender configuration.

The current suite contains **44 non-ignored tests: 19 receiver, 4 session lifecycle,
20 window, and 1 private shadow-policy unit test**. The frame/compact-popup checks
passed all 15 window tests and the shadow policy; the earlier lifecycle check
passed 4/4. The most recent full-suite attempt failed one receiver test with
Windows OS 10053; it was not rerun for this UI update. See the evidence below.
On macOS, Cargo also lists one ignored native startup test; it is not part of
those 44. On other platforms,
that startup test is excluded by a target guard.
A headless suite needs no running GUI, but still compiles native dependencies;
Linux builds need suitable graphics/toolchain libraries.

| Test file | What its passing tests establish |
| --- | --- |
| [`tests/receiver.rs`](../tests/receiver.rs), 19 | Real TCP/HTTP validation, exact status/body assertions, rejection preserving snapshot and notification count, byte/deadline/connection bounds, PNG normalization, publication ordering, snapshots, bind failures, and owned shutdown; includes three response-reader guards |
| [`tests/window.rs`](../tests/window.rs), 20 | Headless egui options, waiting/menu text, viewport commands, opaque-default/background-toggle round trip and transparent renderer clear, original opaque black/partial-alpha/transparent texture pixels and nearest filtering, full-image padding geometry, no redundant uploads (including after a background toggle), burst coalescing, and app-owned receiver shutdown |
| [`tests/sessions.rs`](../tests/sessions.rs), 4 | Deterministic registry lease/expiry, reactivation, isolation, retention, ordering, legacy compatibility, caps, and finite replay horizon |
| [`src/app.rs`](../src/app.rs), 1 | Checkbox-driven opaque → transparent → opaque shadow policy; no native window or compositor is exercised |
| [`tests/startup.rs`](../tests/startup.rs), 1 ignored on macOS | An explicitly launched production process reports an occupied fixed port and terminates as expected on that host |

Receiver and headless window tests bind `127.0.0.1:0`: the OS chooses an
**ephemeral port**, and clients use `local_addr()` to discover it. They do not
need production port 8001 or an existing sender. They create no scratch
directories. Window tests inspect egui outputs without a native event loop;
asserting `StartDrag` or `AlwaysOnTop` does not prove the OS honored the request.
Likewise, transparent viewport options, renderer clear values, and omitted black
paint do not prove native desktop compositing works. The private shadow-policy
test uses the same policy that native `App::ui` applies after showing the UI.
On Windows, the app disables the undecorated shadow in transparent mode and
restores it in opaque mode, applying once initially and then only when the mode
changes. A missing headless native window is a no-op. Policy assertions do not
prove the Windows API was applied or that the visible perimeter disappeared.
The parent observed the new focused test's initial RED run fail with three
E0599 errors for the missing policy method. After implementation, the focused
Windows check compiled the native API successfully but exited 101 (0 passed,
1 failed) at `transparency checkbox must be painted`. Unlike the existing
integration round trip, the new unit test had no idle UI pass between toggles;
its helper now includes that pass before reopening the menu. The corrected
focused test then passed (1 passed, 0 failed). The following full-suite attempt
exited 101 before tests because Windows denied replacement of the running
`target/debug/gentle-emote-desktop.exe` (OS error 5). The first independent source review found no concrete defect but ran zero tests
because its permitted shell executor was unavailable. After the user closed the
utility and authorized local Git Bash selection plus a new bounded verification,
the parent passed the 23-test suite, corrected a format-check failure, and passed
format, offline all-target check, and offline build. A new independent verifier
copied source, Cargo cache, and bounded dependency artifacts into isolated state:
all 23 tests, format, all-target check, the native framebuffer smoke, and an
independent actual-App::ui missing-window probe passed. That probe initially
failed twice because of overconstrained expectations about intermediate UI
frames; its corrected assertions passed without changing production code.
The missing-window regression was then added to `tests/window.rs` using existing
helpers; the parent passed the final 24-test suite, format, and diff checks.
Native smoke printed its documented PASS line on Windows, including HTTP 204,
framebuffer RGB(42,99,151), and receiver port release. The independent run did
not compare an earlier Git baseline. Actual desktop-composited shadow/perimeter
removal, opaque restoration, and human drag/menu checks remain pending.

For the transparency update, Cargo was found at
`C:\Users\sperezm\.cargo\bin\cargo.exe` after initial attempts failed before
running tests (Cargo absent from PATH; `/bin/bash` unavailable). Using that
explicit path from PowerShell, all 8 focused window tests and the complete
22-test suite passed. The first format check failed; `cargo fmt` corrected the
formatting and the next `cargo fmt --check` passed. Clippy did not complete:
`cargo clippy --all-targets -- -D warnings` timed out after 30 seconds while
checking dependencies; it was not retried. The standalone Windows build and
native framebuffer smoke subsequently passed for the shadow update as recorded
above; compositor-level visual checks remain unverified. No RED run was observed
before the original transparency implementation.

For focused investigation, the existing test targets can be run separately:

```sh
cargo test --test receiver
cargo test --test window
cargo test --lib app::tests::native_shadow_policy_follows_background_toggle_round_trip -- --exact
```

## Independent options window and transparent frames

The reported 126 × 126 surface cannot contain the embedded menu. Options now
use a native immediate child viewport with its own content-sized surface and
eight logical pixels of outer padding. Tests drive egui's actual synchronous viewport-renderer hook on the
same Context, not copied application state. They check complete control-label
bounds at root sizes 126 and 128, a distinct child ID with ROOT parent, no root
resize just for opening, per-avatar sizing, unchanged-size suppression, and
Exit explicitly targeting ROOT. Escape, native child close and focus gain/loss
dismiss only the popup; initial lack of focus does not self-dismiss it. Reopening
uses the same viewport. Existing dragging, textures, background and image-fit
regressions remain covered. Before the PNG replacement below, bevels became
opaque-only; transparent geometry stayed unchanged, and returning to opaque
restored all eight edges for two tiles. The frame update now replaces those
edge assertions with artwork/clear-opening checks.

Initial test compilation failed on two incorrect egui API assumptions (parent
is not optional; ViewportClass has no Debug). After fixing the fixture, actual
RED failed both requested rules: missing independent viewport and visible
transparent-mode bevels. First GREEN passed 13/14 window checks; the remaining
fixture expected exported ViewportOutput.class to be Immediate. Public egui
0.37 source leaves that metadata at its default even on the immediate path.
The corrected assertion observes the native immediate-renderer hook, child ID,
parent, separate surface, no deferred callback, and actual child control bounds;
no dependency or captured output was patched. All 14 window tests then passed.

The single full-suite attempt passed the shadow-policy test but failed
`session_http_invalid_input_never_changes_image_or_notifications` at
`tests/receiver.rs:48`: Windows ConnectionAborted (OS 10053), 18/19 receiver tests
passed. This error has historical baseline precedents, but this attempt did not
reproduce a baseline or establish a fresh cause. Receiver source/tests are
unchanged. The suite stopped before later integration targets and was not
retried to obtain a green result. The previously unexecuted lifecycle target
subsequently passed 4/4; fmt, all-target Clippy with warnings denied, and offline
build each passed. macOS startup is not applicable on this Windows host.

Headless outputs establish dispatch, layout and settings behavior, not real OS
focus, DPI, screen-edge placement or compositor visuals. Manual native popup
and transparent-border inspection remain pending. No current full-suite green,
remote CI, native review approval or publication is claimed for this update.

## User-supplied PNG frame and compact popup

The user's RGB source image contains a baked checkerboard, not alpha. Preparation
preserved its SHA-256 and generated a cropped 1100 × 1092 RGBA derivative with a
clear 851-point-source square; see [the exact recipe and provenance](../assets/README.md).
Pixel checks assert the clear opening and exterior alpha, plus retained green
and cyan opaque artwork. The renderer uploads one shared nearest-neighbor frame
texture only for opaque multiple avatars. Tests independently inspect public
texture metadata to distinguish frame and avatar meshes, and check every frame
pixel under each complete avatar rectangle remains transparent at tile sizes
128, 192, 256 and 384. They also check proportional frame fitting, useful image
size at 128, unchanged transparent geometry, toggle restoration, texture reuse,
narrow widths 16/4, and exact single-avatar restoration after removal.

Actual RED expected two PNG frame meshes and received zero from the old bevel.
After integration all 15 window tests and the shadow policy passed. Initial
Clippy rejected two new test-only constant `chunks_exact` calls; the fixture
uses `as_chunks::<4>()` instead, without suppressing warnings. Final verification
passed the 15 window tests, shadow-policy test, format check, all-target Clippy
with warnings denied, and offline build. The compact menu
feedback also has actual RED for excess horizontal whitespace, then a passing
right/bottom padding check with dimensions measured from the controls. The
previous oversized popup was visually confirmed by the user. The user then
accepted the new compact popup and PNG frames with a screenshot showing two
avatars: "ha quedado bastante bien, lo subimos y lo dejamos aqui por el momento".
This is native visual acceptance of the displayed result, not independent
measurement of logical size/DPI or a claim that every compositor/focus behavior
was exercised. Transparent-mode omission remains covered by the automated tests.

The earlier full-suite OS 10053 receiver failure remains recorded, not retried
or declared green. No receiver, protocol, dependency, source PNG or sender
behavior changed. No new review approval, commit or publication is asserted.

## Session-avatar rendering evidence

The T3 test-first run of `cargo test --offline --test window` exited 101: the
neighbor test saw one mesh instead of two, the size menu requested 256×256 instead
of 512×256 for two avatars, and reselecting 192 emitted an unnecessary resize.
After implementation the same command passed all 11 tests; the full offline suite
then passed all 35. The new public HTTP/headless-egui tests check stable neighboring
geometry, five-row padding, per-session texture identity and isolated pixel updates,
nearest filtering, heartbeat-only upload suppression, handle release after close,
last-image retention, replacement by a new session, and per-avatar size selection
from the second tile. Existing single-avatar geometry, alpha/background, native
shadow policy, menu/drag/exit, coalescing, and shutdown regressions still pass.
Expiry timing itself remains covered by receiver/registry tests; the GUI consumes
the same snapshots for expiry and close. These checks do not prove human native
visual behavior or replace an independent concurrency verifier. The existing
receiver transport helper can intermittently reject Windows ConnectionAborted
(OS 10053); that known base failure was not observed in this T3 full-suite run.

## Understand the test-only TCP reset correction

An early rejection (for example, an oversized upload returning 413) can close a
socket while request bytes remain unread. On some schedules/hosts, a client sees
`ConnectionReset` **after it has received the entire HTTP response**. Treating
that reset alone as a failed response read would reject valid evidence.

In [`tests/receiver.rs`](../tests/receiver.rs), `read_response` delegates to
`read_wire`. The helper tolerates a reset only after a complete, unambiguous
HTTP/1.1 response with one explicit Content-Length, no Transfer-Encoding, valid
status/header syntax, and an exactly matching body byte length. Truncated or
ambiguous responses and other I/O errors still fail. Exact status/body assertions
run afterward; complete framing cannot make an incorrect payload pass.

Only the separate `transport_response` path also allows an **empty** reset for
probes that explicitly expect a transport-only close (header/connection limits).
It never accepts a partial reply. Three deterministic `response_helper_*` tests
pin these rules, including ordinary EOF behavior and wrong-payload rejection.
This was a test-reader correction, not a relaxation of production validation or
a guarantee that every closed socket returns an HTTP error. See the
[protocol's rejection and transport rules](protocol.md).

## Explicit macOS production startup regression

Use a **macOS graphical session**, with **127.0.0.1:8001 free initially**:

```sh
cargo test --test startup -- --ignored
```

`production_busy_port_exits_with_error` first occupies that port itself, then
launches the actual production executable. It fails instead of skipping if the
port was already busy. It bounds the child's lifetime to 20 seconds, kills and
reaps the child on failure, and verifies release of the test listener afterward.
It is ignored by default so ordinary headless CI never implicitly opens a native
window.

On the recorded macOS host, the assertion is exit code **1**, empty stdout, and
this exact stderr line (including its trailing newline):

```text
Error: AppCreation(Os { code: 48, kind: AddrInUse, message: "Address already in use" })
```

The numeric OS code and wording are **host-specific**, not a portable error
contract. Production propagates bind and native-backend failures with a nonzero
exit and stderr, without silently choosing another port. The library-level
occupied-port test uses an ephemeral port and checks for an error, not macOS's
exact process output. A backend failure before binding is different from proof
of occupied-port handling.

## Native framebuffer smoke

In a graphical session, on the native main thread:

```sh
cargo run --example native_smoke
```

[`examples/native_smoke.rs`](../examples/native_smoke.rs) wraps the reusable
`EmoteApp` and `native_options`, delegates to the product's native `App::ui`,
and forwards its transparent renderer clear color. The wrapper's observation
and deadline are not product behavior. Its sequence is:

1. Bind a receiver on an ephemeral loopback port and share its address with an
   in-process protocol-compatible test sender thread.
2. Send a generated opaque 64×64 RGBA PNG, with every pixel `[42, 99, 151, 255]`,
   over real TCP/HTTP and require HTTP 204.
3. Observe an allocated `emote` texture with 64×64 dimensions and `NEAREST`
   filtering, then request eframe's own framebuffer capture after painting.
4. Require the captured center pixel to equal RGB(42,99,151), close the window,
   join the sender/watchdog on normal completion, and rebind the port to prove
   receiver shutdown released it.

This is not an OS screenshot API and needs no screen-recording permission.
A **20-second process watchdog** covers stuck startup/rendering and exits
nonzero on timeout. Normal completion prints:

```text
PASS: native startup, HTTP 204, nearest 64x64 texture, framebuffer RGB(42,99,151), clean close/port release
```

Historically, the smoke passed on the macOS development host, as did the then
20 normal tests and the separately requested native startup test, format,
Clippy, and build checks. These are recorded implementation results, not a claim
that the transparency update passed the native smoke. The
smoke uses its own test sender, **not the actual external Gentle Emote sender**,
and does not use production port 8001.

## Recorded real sender transport integration

A separate bounded check used the existing production desktop executable and the
local Gentle Emote fork's production `ExternalDisplay` API (version 0.1.4,
commit `3bc14addc36a557ae30f5feb040c5d2a55dfae88`, pre-existing dirty files retained).
Node 26.3.1 imported only the single-target sender/rendering path, not extension
startup or layered configuration. An explicit per-run destination and guarded
real fetch restricted all requests to `http://127.0.0.1:8001/emote`, rejecting
redirects. There is no documented sender CLI; no CLI flag is implied by this test.

Before sending, the port was free; `lsof` then confirmed that the newly launched,
owned production process was its loopback listener. One sender instance delivered
two distinct composed PNGs sequentially:

| Asset | Composed bytes | HTTP response | Stream sequence |
| --- | ---: | --- | ---: |
| `gentleman/idle/idle.png` | 591 | 204, empty body | 1 |
| `gentleman/idle/idle_blink.png` | 582 | 204, empty body | 2 |

Both had valid PNG signatures and 64×64 IHDR dimensions, protocol version 1,
`Content-Type: image/png`, correct idle/gentleman/frame metadata, and the same
stream identifier. The payload digests differed; neither delivery was overwritten
by a pending selection. Node exited 0 with empty stderr and no error callbacks.
The desktop stayed alive, with empty stdout/stderr after both deliveries.

Cleanup intentionally terminated only the owned app with SIGTERM (exit −15).
This is not graceful `Drop` evidence; the earlier native smoke provides that
separately. The orchestration exited 1 at an immediate plain Python socket rebind
(`EADDRINUSE`). A subsequent bounded cleanup observation found no listener and
successfully bound/listened with `SO_REUSEADDR`. No integration rerun occurred;
the plain rebind was not shown to be equivalent to the production listener.

Preservation checks found desktop source unchanged and the sender's three
pre-existing dirty status entries unchanged under the same user Git configuration.
Four `.DS_Store` entries visible without global Git configuration were not proof
of writes: their timestamps predated the probe, and `git check-ignore` traced all
four to the existing global `.DS_Store` rule. No files were removed or normalized.
A planned optional byte-hash comparison was interrupted and its preflight hashes
were not persisted, so byte-identical before/after contents are not claimed.
No sender source/global configuration write was issued or intended.

This establishes real composition/delivery to the actual receiver, not human
visual behavior or framebuffer capture of these two real frames. The generic
native framebuffer smoke above remains separate evidence.

## Final manual verification — still outstanding

Use this checklist to gather missing evidence, not as a record of completed work:

- [ ] Launch the production app with `cargo run` in a graphical session, with
  port 8001 free. Confirm the waiting message and controls are readable.
- [ ] Using **only the external sender's documented options**, configure that
  run to target `http://127.0.0.1:8001/emote` (localhost, port 8001). Do not invent
  sender CLI flags or a configuration schema; do not write global configuration.
  If no documented per-run override exists, stop and resolve that separately.
- [ ] Send actual sender updates and verify the visible emote changes, pixel-art
  edges stay sharp, and labels already embedded in the PNG are not duplicated.
  Record sender version, supported per-run option used, and observed outcome.
- [ ] Send two session-aware avatars and confirm neighboring tiles stay in stable
  order, update independently, and resize together at each per-avatar menu size.
  Close or let one expire and check the other remains; close the last and check
  its image remains until a new session replaces it. Verify legacy singleton
  traffic still renders. Many sessions may exceed monitor width; no overflow
  handling is promised. These multi-avatar native checks have not been performed.
- [ ] With another ordinary window overlapping, visually check always-on-top
  behavior. Check primary drag, right-click size choices (128/192/256/384), and
  Exit. Confirm menu selection does not start a drag and exit releases port 8001.
- [ ] On a contrasting desktop background, confirm the initial window surface is
  opaque black. Right-click → **Transparent background**: check that unused space
  and the added five-source-row top padding reveal the desktop, fully transparent
  PNG pixels reveal it, and partial-alpha pixels blend with it. Confirm opaque
  black PNG pixels, including the sender's bottom gap, remain black; the full
  image and nearest-neighbor edges must remain unchanged.
- [ ] On Windows, inspect the transparent window against a contrasting desktop:
  confirm the native rounded shadow/thin perimeter is absent, then toggle back
  to opaque and confirm its previous shadow policy returns. This requires a
  compositor-level visual check; the framebuffer smoke cannot prove it.
- [ ] Toggle back to black, then enable transparency again without sending a new
  frame. Check there is no stale opaque surface, image loss, or unintended
  move/resize/close. Check menu readability and primary drag in both modes; the
  transparent surface must not become click-through. Restart and confirm the
  option resets to off with an opaque black surface.
- [ ] Repeat applicable build/run and human window checks on Windows and Linux;
  record OS, graphics backend, and display scale. On Wayland, record compositor
  support/policy for topmost placement, native moves, and desktop alpha rather
  than promising them. Record whether transparency is supported by each tested
  backend/compositor; headless checks cannot establish it.

Eframe/egui 0.37.0 with Glow is the implementation choice; X11 and Wayland source
backends are enabled. macOS, Windows, and Linux are **source/build targets**.
Native graphical execution evidence is recorded only for macOS; the transparency
update's headless tests also passed on Windows. A framebuffer pixel proves
part of the native rendering pipeline, not topmost placement, successful OS
dragging, native menu usability, every display scale, or cross-platform parity.

Continue with the [architecture study guide](architecture.md) for ownership and
threading, or the [wire protocol](protocol.md) for exact input and trust limits.

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

Publication-slice validation runs the public window tests and the native-shadow policy test in an isolated copy. Historical receiver-suite failure and unresolved native review remain unchanged; inventory is not a full-suite pass claim.
