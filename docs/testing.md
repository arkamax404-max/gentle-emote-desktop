# Verify behavior without overstating evidence

Start with the headless suite, then use native checks only in a suitable graphical
session. Socket tests, framebuffer capture, human window inspection, and a real
external sender each establish different things. The real sender transport check
below passed; it did not capture each real frame's framebuffer. Production macOS
WGPU transparency has since received local human acceptance after relaunch with
sender alpha preservation enabled. The controlled integration below passed Linux
headless/build checks; Windows/macOS were not rerun for this candidate;
remaining manual checks and historical platform evidence are distinguished below.

## Desktop focus-request header contract (T2, S2–S3)

The receiver emits only `X-Emote-Focus-Request-Id` as its optional focus header
on accepted current-stream frame/heartbeat POSTs, still returning 204/empty body.
Receiver and headless window helpers assert the obsolete `X-Emote-Action`,
`X-Emote-Action-Id` and response `X-Emote-Session-Id` names are absent. A dedicated
regression proves `X-Emote-Action-Ack` no longer clears pending focus, while
`X-Emote-Focus-Ack` does. Existing tests retain exact/old/malformed/unknown/duplicate
ACK, delivery suppression, pixel/notification and lifecycle coverage.

The sender ACKs only after confirmed Herdr focus (successful Promise, CLI exit 0,
JSON `agent_info` for the expected pane with `focused: true`). Without an ACK,
the same ID is redelivered on every accepted frame or heartbeat POST for that
current session stream until the 15-second TTL or lifecycle cleanup. The sender
deduplicates without retrying failed IDs. Desktop tests do not establish actual
sender focus confirmation; real sender/Herdr integration remains pending.

Test-first RED with the updated assertions, before receiver changes:
`cargo test --offline --config build.incremental=false --target-dir target/focus-check --test receiver focus_`
exited **101 (2 passed, 7 failed)** because the new response header was absent.
After the rename, that command exited **0 (9 passed)**. Using the same Cargo
prefix, `--lib focus_` passed **5**, `--test window focus_` passed **4**, and
`--test sessions` passed **10**, all exit 0. All runs used PowerShell.

The single full `--test receiver` run exited **101 (33 passed, 1 failed)** at
`tests/receiver.rs:61` in
`project_name_rejection_preserves_pixels_state_name_notifications_and_sequence`
with Windows ConnectionAborted / OS 10053, the reported pre-existing intermittent
failure. Its one authorized rerun with the same Cargo prefix and
`--test receiver project_name_rejection_preserves_pixels_state_name_notifications_and_sequence -- --exact`
exited **0 (1 passed)**. The failed full run is not claimed green. No app was
launched/closed and no full unfiltered suite or native Herdr focus check was run.
Rustfmt was limited to `src/receiver.rs`, `tests/receiver.rs` and `tests/window.rs`.

## Desktop focus-request handoff (T1, S1–S7; historical generic headers)

The `focus_*` checks use only ephemeral receivers and headless egui; no desktop
process is launched or closed, no real user configuration is used, and no sender,
terminal, emulator, LED or serial code is exercised. Run in the isolated target
because a running desktop can lock other build directories:

```sh
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test receiver focus_ -- --nocapture
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test window focus_ -- --nocapture
cargo test --offline --config build.incremental=false --target-dir target/focus-check --lib focus_ -- --nocapture
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test receiver
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test sessions
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test window menu
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test window primary_drag_requests_native_move -- --exact
cargo test --offline --config build.incremental=false --target-dir target/focus-check --test window card_
```

Observed initial RED (exit 101): receiver focus filter **0 passed / 6 failed** at
the queue scaffold, window focus filter **0 passed / 3 failed** (two absent focus
assertions and one test-helper consumption error), and registry focus filter
**0 passed / 2 failed** at the queue scaffold. The menu fixture was corrected by
rendering a new child pass before consuming its output again. These initial
failures establish missing queue/click behavior, not independent RED observations
of every downstream ACK/ID assertion. Additional actual boundary RED checks failed
one test each (exit 101): a press+release in one input batch did not queue, and
an accepted heartbeat just after lease expiry but before the timer tick still
returned the pending ID. Both were corrected and subsequently passed.

Final focused GREEN (exit 0): **8 receiver, 4 window and 5 injected-time registry**
tests. HTTP assertions cover all three optional headers, exact case-preserved
session identity, 204/empty body and existing Content-Type/Connection headers,
idle heartbeat redelivery, per-session isolation, exact ACK on frames/heartbeats,
old/malformed/unknown/duplicate ACK, unique bounded ASCII IDs across clicks and
receiver instances, error/replay/retired/legacy suppression, close/takeover and
shutdown. Delayed-body probes cover an old ACK surviving a newer click and an
old stream losing to takeover, while checking unchanged pixels/notification
counts for ignored requests. Time-injected registry boundary tests use the same
application seam as HTTP (not direct pending-storage inspection): delivery at
5/10 seconds and one nanosecond before 15 seconds, expiry exactly at 15 seconds,
lease expiry with and without a timer tick, inactive clicks, eviction/reintroduction
and shutdown. Public registry preservation also checks that clicking changes
neither pixels, lease nor sender sequence.

Headless click checks cover separate and batched press/release, no request on
press, new ID per click, normal and idle session cards, cross-card release,
separators, legacy/global fallback, preserved primary StartDrag, secondary menu
and menu-owned primary controls. The registry rejects inactive/elapsed leases;
no 30-second UI sleep is used. No native OS focus or actual sender receipt is
claimed. Cross-process restart uniqueness is provided by the time/process nonce
construction, not a process-restart integration test.

Preservation runs above all exited 0: **33 receiver**, **10 session**, **7 menu**,
**1 primary drag**, and **10 card-filter** tests (filters overlap). No full
unfiltered suite, native launch or lint run was attempted. The earlier full-suite
timeout recorded below was not retried. Rustfmt was limited to the six changed
Rust files; final whitespace/status checks are reported in the implementation
handoff. This change exceeds the approximate 400-authored-line planning heuristic
because it includes deterministic lifetime, public wire/UI tests and protocol docs;
that heuristic was not used to compress or omit coverage.
## Platform renderer checks

Production `native_options()` selects WGPU on macOS and Glow on other targets.
`native_renderer_matches_platform` asserts the target's compile-time selection:

```sh
cargo test --offline --test window native_renderer_matches_platform -- --exact
```

Run this on each target to verify its branch; a macOS pass is not Windows/Linux
compilation evidence. The `diagnostic-wgpu` feature remains available for the
optional transparency probe, without changing production selection. On the
reported Mac, the human confirmed desktop transparency with the WGPU diagnostic
but not Glow. Both diagnostic framebuffer captures had correct pre-swap alpha;
this observation does not establish a universal root cause or platform guarantee.
Production WGPU visual acceptance was subsequently confirmed after rebuilding
and relaunching the desktop with per-target sender `transparentBackground: true`:
the human reported “ok, va perfecto ahora”. The production macOS overlay disables
native shadows to avoid ghosting; this is not the sole opacity cure. The sender's
default `false` flattens PNG alpha to 255; the target opt-in preserves source RGBA,
including opaque black, and requires sender reload. It does not remove black or
make an opaque source PNG transparent. See the [per-target setup](../README.md#sender-transparency-optional-per-desktop-target).
No personal sender configuration is included in the repository changes.

### Post-v0.1.4 macOS verification and acceptance

The production WGPU build and native framebuffer smoke passed locally, including
HTTP 204, expected framebuffer RGB and clean close/port release. Format, all-target
Clippy with warnings denied, and build checks passed. The window suite reported
**54 passed, 5 failed**; it is not a green full-suite result. One color-test failure
was reproduced identically on the baseline; the other four failures have **not**
all been established as baseline failures. These results are recorded prior
verification, not checks rerun for the documentation/diagnostic cleanup.

The human confirmation above establishes locally tested macOS transparency with
the production renderer and the configured sender. It does not establish a
minimum macOS version, all-device/DPI compatibility, every manual interaction,
or Windows/Linux execution in this session. Earlier Windows evidence below
remains historical, not superseded by a claim that Windows has never run.

The transparency probe's held-window identifier occupies logical coordinates
(8, 8)–(164, 26). Its old top sample at (96, 24) could measure the opaque badge
instead of the background in CAPTURE runs. The sample now uses normalized
framebuffer coordinates `(7/8 width, 1/4 height)` — (168, 48) on the 192-point
surface — outside the badge at display scale and away from Waiting text. This
is a deterministic diagnostic-only correction, not a production rendering fix.
`--no-capture` still reports no framebuffer measurement and cannot claim a sample
PASS; native capture checks must be rerun separately to validate the new sample.

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

The current Linux run passed **134 tests**: 34 receiver, 66 headless
window, 10 session lifecycle, 3 background-color store, 9 idle-clock image, and
12 library unit tests (5 app policies, 1 receiver display, 6 session helpers).
Windows/macOS were not run for this candidate; no guarded target counts are inferred.
The macOS-only native startup test is ignored by default and counted separately;
on other platforms that startup test is excluded by a target guard.
A headless suite needs no running GUI, but still compiles native dependencies;
Linux builds need suitable graphics/toolchain libraries.

| Test file | What its passing tests establish |
| --- | --- |
| [`tests/receiver.rs`](../tests/receiver.rs), 34 | Real TCP/HTTP validation, exact status/body assertions, rejection preserving snapshot and notification count, byte/deadline/connection bounds, PNG normalization, publication ordering, snapshots, bind failures, and owned shutdown; includes three response-reader guards |
| [`tests/window.rs`](../tests/window.rs), 66 on Linux | Headless egui options, waiting/menu text, viewport commands, opaque-default/background-toggle round trip with the 45%-transparency toggle-on default and transparent renderer clear, original opaque black/partial-alpha/transparent texture pixels and nearest filtering, full-image padding geometry with whole-physical-pixel scales and snapped edges at every supported `pixels_per_point` (documented fractional fallback below 64×65 physical pixels), no redundant uploads (including after a background toggle), burst coalescing, and app-owned receiver shutdown |
| [`tests/sessions.rs`](../tests/sessions.rs), 10 | Deterministic registry lease/expiry, reactivation, isolation, retention, ordering, legacy compatibility, caps, and finite replay horizon |
| [`tests/background_color.rs`](../tests/background_color.rs), 3 | Strict lowercase `#rrggbb` persistence store with nested save/load round trips, black fallbacks, save errors and absolute-root policy |
| [`tests/idle_clock_image.rs`](../tests/idle_clock_image.rs), 9 | Canonical pixel-clock golden comparison and session-header renderer checks |
| [`src/app.rs`](../src/app.rs), 5 | Checkbox-driven shadow policy and explicit Linux/Windows/macOS menu-focus, button, resize and square-card eligibility/bounds checks; no native window or compositor is exercised |
| [`src/receiver.rs`](../src/receiver.rs), 1 | Clock-silence display rule applied at the render seam |
| [`src/sessions.rs`](../src/sessions.rs), 6 | Clock-title silence, focus lifetime and session-registry helpers |
| [`tests/startup.rs`](../tests/startup.rs), 1 ignored on macOS | An explicitly launched production process reports an occupied fixed port and terminates as expected on that host |

Receiver and headless window tests bind `127.0.0.1:0`: the OS chooses an
**ephemeral port**, and clients use `local_addr()` to discover it. They do not
need production port 8001 or an existing sender. Persistence tests create isolated
OS-temp directories and remove them in teardown; they never read/write real user
configuration or mutate process environment/cwd. Other socket/window checks need
no scratch directories. Window tests inspect egui outputs without a native event loop;
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

## Desktop background RGB selector and user-global persistence

**Background color** expands egui's opaque RGB picker inside the existing native
options window. The menu measures the expanded controls and shrinks when they
are collapsed; there is no nested color popup clipped to the compact viewport.
Black remains the fallback. Ordinary filesystem-free `EmoteApp::new` instances
still reset color; the production entry point explicitly enables the dedicated
user-global RGB store, so its selected color survives restarts and projects. The existing
transparency checkbox and slider apply independently to the selected RGB. This
setting affects only desktop underlays, not received pixels, sender settings or
HTML/serial LED displays.

Two `background_color_*` public-menu regressions edit the actual R/G/B numeric
controls to RGB(32,128,224). They inspect unmultiplied-to-premultiplied egui fill
colors at default/zero/half/full opacity, checkbox round trips, waiting, mixed
avatar/clock cards on both axes, a single session clock and the global fallback.
They check menu expansion/collapse and label bounds, unchanged root commands,
clear alpha, frames, image geometry and white mesh tint, cached texture reuse,
rounded individual cards and five-point clear separators. The existing upload
regression additionally changes RGB after uploading opaque black, partial-alpha
and transparent source pixels and requires the exact same image mesh with no
texture reupload. Existing default-black and native-shadow regressions remain.

The initial session-only selector milestone below is historical; persistence is
now opt-in at the app API and enabled by the production entry point. The new
`tests/background_color.rs` covers strict lowercase `#rrggbb` with optional single
LF, nested-directory save/load round trips, missing/malformed/oversized/unreadable
black fallbacks without repair, save errors and absolute-root policy. Two new
`background_color_persistence_*` public-menu tests cover immediate save before
Drop, reconstruction with only RGB restored and unrelated settings at defaults,
unchanged unrelated files, ordinary constructor isolation and nonfatal save failure.
The file overwrite is not claimed atomic, nor are native restart or all-platform
execution claimed by headless tests. No real user config is touched in tests.

Observed RED: `cargo test --offline --config build.incremental=false --test window
background_color -- --nocapture` exited 101 with both new tests failing at
`menu option must be painted: Background color`. After implementation it exited
0 with 2 passed, 0 failed. The strengthened final focused run again passed both;
`cargo test --offline --config build.incremental=false --test window
upload_only_on_new_snapshot_and_use_nearest_filter -- --exact` passed 1/1.
Disabling incremental compilation was necessary because ordinary `cargo test`
initially reused an outdated executable (0 matching tests, 26 filtered out)
instead of compiling the current source (44 window tests).

The combined focused-plus-full-suite attempt reached its 30-second foreground
limit during the window target. Before timeout, the suite reported 3 library,
9 clock-image, 25 receiver and 9 session tests passed; main/startup had no tests
on this Windows host. The window target began and reported passing tests but
no final count. This is **not a complete suite pass**; the timed-out command was
not retried. No background-job execution tool was available to this worker.
A subsequent targeted preservation check,
`cargo test --offline --config build.incremental=false --test window
small_surface_menu_has_independent_viewport_and_complete_controls -- --exact`,
exited 101 (0 passed, 1 failed) at `tests/window.rs:1650` with
`menu has unnecessary horizontal space`. Disabling the color header's
full-available-width frame in a scoped style was one correction attempt; the
same check still failed. That chained command stopped before rerunning the
RGB/upload checks.

The subsequently authorized bounded correction replaced the collapsing header
with a naturally sized **Background color** toggle and a session-local expanded
flag, initially false. Only the actual toggle and visible picker content now
contribute to measured width; no scoped header style override remains. The
unchanged compact-menu regression command above then exited 0 (1 passed,
0 failed). The final RGB command exited 0 (2 passed, 0 failed), and the final
upload regression command exited 0 (1 passed, 0 failed).
`rustfmt --edition 2024 --config skip_children=true src/app.rs tests/window.rs`
also exited 0, formatting only those two allowed files.

The earlier full-suite timeout remains a failed/incomplete verification result;
the suite was not retried or moved to background execution. Native human checks
of the picker, compositor colors, shadow policy and screen placement remain
outstanding, as do full-suite completion and lint verification.

## Session-local card order and clock dismissal

The `card_*` public HTTP/headless-egui regressions in `tests/window.rs` exercise
the actual secondary-click target and native child buttons. Both axes cover moves,
boundary no-ops, independently targeted identities, update/heartbeat stability and
new-card append. Clock cases cover ordinary-card close absence, preserving a normal
neighbor, repeated clock/heartbeat suppression, normal-image restoration, last-card
Waiting for session and legacy clocks, and global fallback suppression separately
from legacy None. Stale-menu clicks cannot close a returned avatar or move/close
a replacement. Separator and unused framed-margin clicks offer only global controls;
actual-card controls fit the compact native menu without weakening its width bound.
Existing primary drag, menu dismissal/Exit, image/clock geometry and RGB persistence
checks remain separate preservation evidence.

The initial focused run with the two new behavior tests exited 101: **Move up**
and **Close clock** were missing (2 failed, 2 existing contour tests passed).
After implementation that run passed 4/4. After adding the remaining rule-named
cases the focused filter passed 8/8 (six card tests and two existing contour tests).
A strengthened legacy-append check initially failed (7 passed, 1 failed): it sent
both new identities before the next UI observation, so their simultaneous discovery
correctly used registry snapshot order. Adding the intended UI discovery pass
between those arrivals corrected the fixture; the next run passed 8/8. The final
focused run, also asserting the exact 192×192 waiting resize, passed 8/8.
Changed-scope preservation runs passed compact-clock 3/3, session-clock 6/6,
background-color UI 4/4, menu 6/6, primary drag 1/1 and neighboring-cache lifecycle
1/1, plus library 3/3 and isolated background-color store 3/3. All used offline,
nonincremental Cargo and `--target-dir target/persistence-check`.
These are headless UI dispatch/layout checks, not native human visual acceptance.
The running production desktop was not closed or replaced. The earlier full-suite
timeout was not retried, and no native launch or real global configuration was used.

## Native acceptance and integration evidence

After the focused checks above, the parent built the current desktop successfully
with `cargo build --offline --config build.incremental=false --target-dir
 target/persistence-check` (exit 0), replaced the earlier test instance, and
confirmed the new process was responding. The user exercised the native window
and reported `va perfecto`, then authorized integration and pushing to main.
This provides human acceptance of the tested card controls and background-color
experience, not proof of every manual checklist item, cross-platform behavior,
or a complete test-suite pass. The independent verifier reported 15 passing
checks using copied focused test executables and independently compiled source
probes; it did not claim a fresh full Cargo rebuild or full-suite completion.
The earlier full-suite timeout remains outstanding historical verification.

## Independent options window and transparent frames

The reported 126 × 126 surface cannot contain the embedded menu. Options now
use a native immediate child viewport with its own content-sized surface and
eight logical pixels of outer padding. Tests drive egui's actual synchronous viewport-renderer hook on the
same Context, not copied application state. They check complete control-label
bounds at root sizes 126 and 128, a distinct child ID with ROOT parent, no root
resize just for opening, per-avatar sizing, unchanged-size suppression, and
Exit explicitly targeting ROOT. Escape and native child close dismiss only options.
**Close options** and focus-loss stability are Linux-only. Windows/macOS retain
upstream focus-gain → focus-loss dismissal, with no added button or menu-height
row. Initial lack of focus and unknown focus do not dismiss options. Existing action-triggered
closures are unchanged; sliders and the color picker stay open. Reopening
uses the same viewport. On Linux only, repeating a root right-click closes open
options on release; the next invocation reopens the same child, while Windows/macOS
keep opening/retargeting options. Wayland placement remains compositor-controlled, not
anchored beside the click; headless checks do not establish native usability.
Existing dragging, textures, background and image-fit regressions remain covered. Before the PNG replacement below, bevels became
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

## Independent No frame option

The exact **No frame** checkbox hides decorative PNG frames without changing the
background setting. It starts unchecked and resets on restart. With multiple
avatars it divides the surface into equal cards with exactly five logical
pixels between adjacent card contours. Each card independently fits the PNG
with the original top compensation and four-point inset. Separators always expose the
transparent renderer clear, even when the remaining background stays opaque
black. No PNG colors or alpha are rewritten. Window dimensions and single-avatar geometry are unchanged.
The gap is clamped only for transient surfaces too narrow to contain it. Transparent mode never paints frames, and switching
background modes does not reset the independent checkbox. Single-avatar geometry
and source textures are unchanged; restoring frames reuses the shared texture.
The popup includes the new row in its existing content-sizing calculation.

The public-interface regression drives the actual native-child UI checkbox,
checks frame meshes, background paint, image fit and texture reuse, crosses both
settings in both directions, and removes a session to check single-avatar fit.
The initial RED run failed at `checkbox must be painted: no frame` (one test
executed, one failed). After implementation, all 16 window tests and the
shadow-policy unit test passed, along with format, offline all-target Clippy
with warnings denied, and an offline native build. The earlier full-suite
OS 10053 receiver failure was not retried as part of this UI-only change.
Native visual acceptance of the new checkbox remains separate from these
headless dispatch and geometry assertions.

The follow-up spacing/capitalization RED ran two tests and failed both: the old
caption did not match `No frame`, and the original unframed geometry left
16.695648 points between images at size 128 instead of five. The spacing
regression covers two and three avatars, sizes 128/192/256/384, and both opaque
and transparent backgrounds, inspecting actual image rectangles. The first
post-implementation run passed 16/17 window tests but failed an overstrict exact
vertical-coordinate comparison (12.695656 versus 12.695659); the assertion now
uses the same 0.001-point floating tolerance as the gap and size checks. The
corrected run passed all 17 window tests, the shadow-policy unit test, format,
offline all-target Clippy with warnings denied, and the offline native build.
The receiver suite and unresolved native review were not retried.

The requested transparent-separator follow-up first failed because the five-pixel
strip still intersected opaque background paint. Splitting only that background
into segments passed both No frame regressions. Checks cover the complete gap
strip, transparent renderer clear, and preserved opaque underlays behind avatars.

### Vertical / Horizontal selector

Two exclusive radio choices select the stacking axis, initially Horizontal and
reset on restart. Selection closes only the menu, updates the root dimensions
once, and preserves size per avatar. Single-avatar geometry is unchanged.
No frame's five-pixel transparent separator follows the stacking axis: vertical
strips for a horizontal row, horizontal strips for a vertical column.

The initial public-menu RED failed at `checkbox must be painted: Vertical`.
The added regression checks default horizontal framing, vertical resize and
image order, two/three avatars at sizes 128/192/256/384 with clear separators and
opaque underlays, full transparency, restoration of frames, switching back to
Horizontal, and exact single-avatar restoration. Popup layout checks also include
the selector heading and both radio labels. Final verification passed all 18
window tests, the shadow-policy unit test, format, offline all-target Clippy with
warnings denied, and the offline native build. No receiver-suite or native-review
retry was performed; compositor-level visual acceptance remains separate.

### Rounded card corners beside separators

Human Image 4 showed square adjoining corners. The first underlay-only fix used
four logical pixels and passed 19 window tests, but Image 5 explicitly rejected
that result: rounding was smaller than the original and lower corners still
looked square. Those tests were insufficient evidence of the complete contour.

Two new RED failures reproduced the defects: background radius four instead of
eight, and an actual image mesh vertex at `[131.3, 124.0]` outside the desired
rounded card at size 128. No frame now uses an eight-logical-pixel outline for
both background and image. The rounded convex card is intersected with the image
rectangle; only the textured draw mesh is clipped, without rewriting cached PNG
pixels or changing its original coordinate/UV fit. Framed and single-avatar
paths keep their previous drawing behavior. No frame also applies the contour
when the remaining background is transparent.

Public-output regressions inspect every image vertex against the analytic
rounded-card mask, valid source UVs, nonempty meshes, all four background radii,
three cards, both orientations and sizes 128/192/256/384. They also retain checks
for image bounds, unchanged uploads, five-pixel clear separators and old controls.
Final verification passed all 20 window tests, the shadow-policy unit test,
format, offline all-target Clippy with warnings denied, and offline native build.
No full receiver suite or native review retry was performed. Human visual
acceptance of this corrected eight-pixel complete contour is still pending.

### Independent transparency control (current behavior)

**Transparent background** is unchecked by default. **Background transparency**
is disabled at 0% (opaque) until checked; checking initially selects 45%
(opacity 55, alpha 140). Checked-at-zero and partial values keep the checkbox checked.
Unchecking always resets to opaque 0% and disables the slider, not a remembered
partial percentage. Only background paint fades. Actual opacity controls frames
and native shadow; checked-at-zero may still show the frame when **No frame**
is unchecked, now including a single avatar (see the follow-up below).

`transparency_checkbox_gates_slider_and_restores_opaque_frame_choice` drives the
actual immediate native-child menu with two session avatars, including disabled
track clicks and real track values 0/50/100, checked-state paint, exact displayed
percentages, opaque menu paint, white avatar colors, texture reuse and both
No frame preferences. Existing tests retain contour, five-point separator,
orientation, dimensions, PNG and single-avatar coverage. The pre-fix public-UI
endpoint probe failed the newly required zero-transparency opaque-frame assertion
(0 frames versus 2). That RED established the old opacity/transparency mismatch,
not the exact numeric cause of the reported 2% floor. The corrected UI displays
exact 0/50/100% values after actual track clicks. Its first two post-change runs
failed because the fixture expected a combined percentage text shape; egui paints
the number and `%` separately. Correcting that observation passed the focused
regression and all 24 window tests, plus the shadow-policy test, formatting,
offline all-target Clippy with warnings denied and offline build. The reported
native 2% floor was not independently reproduced; passing headless endpoints do
not prove all native pointer/DPI behavior. Human compositor checks remain
outstanding. The historical receiver failure was not rerun.

The following section records the superseded opacity-slider milestone; its
remembered-percentage behavior is no longer current.

### Single-avatar frame activation (current follow-up)

The human explicitly superseded the earlier borderless-single-avatar rule.
An actual single avatar now uses the same decorative frame and clear-opening
composition as a multi-avatar tile whenever transparency is 0% and **No frame**
is unchecked, including at startup defaults. Checking **No frame** retains the
old full-surface single-avatar fit. Positive transparency omits the artwork;
unchecking transparency restores the frame unless **No frame** remains checked.
The empty waiting surface neither paints nor uploads the decorative frame.
Native root dimensions, opacity/shadow policy and source textures are unchanged.
Earlier milestone references to borderless single avatars are historical.

`single_avatar_frame_follows_opaque_no_frame_and_transparency_controls` drives
real child-menu checkboxes and exact displayed 0/50/100% slider values with both
No frame preferences, including disabled clicks after opaque reset. It checks
frame texture identity/nearest filtering, white mesh colors, unchanged uploads,
opaque/partial/clear paint, no root commands and the original No frame fit.
RED failed with zero frames instead of one before the count gate was changed;
GREEN then passed. Existing removal/texture-reuse tests now expect an opaque
single avatar's frame to survive removal of its neighbor. The original full-UV
composition test explicitly selects **No frame** instead of weakening its fit
assertions. Native visual acceptance remains separate from these headless checks.

### Automatic unframed transparency and background opacity (historical)

Any background percentage below 100 automatically omits decorative frames and
uses the compact five-pixel layout, regardless of the opaque-mode No frame
preference. Returning to 100 respects that preference and reuses the frame asset.
Narrow transient surfaces reserve positive image space while clamping gaps.

The background-only slider covers 0–100%, starts at 100%, and is runtime-only.
At 50%, black underlays use alpha 128; image-mesh vertex colors remain white,
source textures are not reuploaded, and the options surface stays opaque.
The full-transparency checkbox selects 0%, then restores the last nonzero slider
percentage on deselection. Windows' native shadow is enabled only at 100%.

New public-interface regressions cover automatic compact layout with both axes,
two/three avatars, sizes 128/192/256/384, narrow 16/4-point resizes, restored frame
preferences, slider values 0/50/100, unchanged avatar textures/colors, opaque menu
paint and remembered percentages. The existing UI-driven shadow-policy test also
covers partial opacity, restoring it through the checkbox, and returning to 100%.
Follow-up verification (job51): all 22 window tests and the one UI-driven
native-shadow policy test passed, followed by formatting, offline all-target
Clippy with warnings denied, and an offline native build. The initial GREEN49
fixture incorrectly rejected the third avatar's first texture upload; it now
asserts that upload before checking resize reuse. GREEN50 exposed a simulated
menu click against the preceding four-point viewport; an idle layout pass in
the input helper fixed the sequencing without changing application restoration.
At that milestone, uniform per-card top padding was not established. The later
Image6 correction below replaces global image packing with independent card
composition; the original five-pixel bitmap-edge rule is superseded by the
human-selected five-pixel card-contour rule. The receiver suite
was not rerun, and its historical connection-aborted failure and unresolved
native review are not superseded.

### Rounded intermediate-opacity single-avatar contour

The single-avatar fallback previously painted a square background at partial
opacity while Windows' native shadow was disabled. Its image quad could also
cover lower-corner cutouts. A shared eight-point background/image contour now
applies at intermediate percentages, without modifying the composition offset,
PNG pixels, stacked geometry or the fully opaque/fully clear single-avatar paths.

The public-UI regression selects 50%, checks all four underlay radii and every
image vertex against the rounded contour at sizes 128/192/256/384, and checks
white image colors, valid UVs and texture reuse. Returning to 100% or 0% retains
the original single-avatar quad. RED53 reproduced corner radii 0 instead of 8.
GREEN54 passed all 23 window tests and the one shadow-policy test, followed by
formatting, offline all-target Clippy with warnings denied, and an offline native
build. This is geometry/policy proof, not human visual or all-DPI compositor
acceptance. The historical receiver-suite failure and native-review limitation
remain unchanged.

### Uniform per-card composition after Image6

Image6 confirmed unequal first/last margins: background bounds followed packed
image edges instead of each card having its own padding. The human selected
correcting this before publication, with five clear pixels between card contours.
The surface extent and ordering stay unchanged. Card regions now have equal
sizes; each uses the original 64-by-65 fit and one-source-row top compensation
inside a four-point inset. This may slightly reduce vertical image size, rather
than increasing the native window dimensions. Embedded source pixels are unchanged.

RED59 reproduced the first horizontal image at x=14.2 instead of the independently
fitted x=7.1. GREEN60 passed all 24 window tests and one shadow-policy test,
formatting, offline all-target Clippy with warnings denied, and offline build.
The new public-interface regression covers both axes, two/three avatars,
128/192/256/384 surfaces, and 0/50/100% backgrounds; it checks each image's full
bounds, per-card footer balance, rounded backgrounds, clear separators and cache
reuse. Existing separation tests now inspect card contours rather than requiring
zero internal image margins. Framed and single-avatar paths retain their checks.
This is functional geometry proof, not human visual or all-DPI approval.

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
geometry, one-row padding, per-session texture identity and isolated pixel updates,
nearest filtering, heartbeat-only upload suppression, handle release after close,
last-image retention, replacement by a new session, and per-avatar size selection
from the second tile. Existing single-avatar geometry, alpha/background, native
shadow policy, menu/drag/exit, coalescing, and shutdown regressions still pass.
Expiry timing itself remains covered by receiver/registry tests; the GUI consumes
the same snapshots for expiry and close. These checks do not prove human native
visual behavior or replace an independent concurrency verifier. The existing
receiver transport helper can intermittently reject Windows ConnectionAborted
(OS 10053); that known base failure was not observed in this T3 full-suite run.

## Final-close and internal pixel clock regressions

`sole_avatar_close_immediately_shows_internal_pixel_clock_and_new_avatar_replaces_it`
checks initial Waiting, a live sole avatar, the immediate final-close internal
pixel clock (one shared `emote-idle-clock` texture, 64x64 NEAREST upload with
transparent gaps and opaque ink, no avatar/frame meshes, ≤500ms repaint
scheduling, whole-physical-pixel fit) and replacement by a new session through
HTTP and headless egui.
`per_session_clock_tiles_run_alongside_normal_avatars_without_hiding_them`
checks a session whose frame carries `X-Emote-State: clock`: it renders the
internal clock in its own tile with the normal tile frame next to an active
normal avatar, uploads no sender pixels while idle, and any ordinary image state
exits it. The neighboring session regression still checks unaffected remaining
avatars and cached texture ownership until replacement.

`tests/idle_clock_image.rs` compares the internal renderer against
`tests/fixtures/idle-clock-golden.json`, generated by executing the canonical
reference renderer (`gentle-emotex/src/idle_clock_image.ts`, canonical sprite in
`tests/fixtures/lemmings-walk.json`) at deterministic fixed-offset instants: it
pins the full pixel output (glyph typography, palette, path row, all sprite
math) plus the approved cadence — walker pose and movement advance every 500ms
while glyph rows stay byte-identical within one second and the seconds glyph
advances on the whole second. Deterministic registry checks cover final close,
remaining active sessions, timeout retention, delayed close after timeout,
reactivation, legacy suppression, state metadata through snapshots/lifecycle,
and unchanged compatibility snapshots/latest. Receiver HTTP checks cover the
`X-Emote-State: clock` mapping and unknown-value compatibility. These
regressions do not establish native visual acceptance. Current execution
results are reported separately; earlier milestone results above are historical.

### Session-header renderer and app wiring checks

`cargo test --offline --test idle_clock_image` runs the four unchanged canonical
checks plus three public-RGBA session-header checks. The new API is
`render_for_session(now: DateTime<FixedOffset>, session_id: &str) -> Frame`;
named app clock tiles supply project names through this legacy-compatible API. Assertions cover exact 4999/5000/9999/
10000ms boundaries, negative Unix buckets, byte-identical date phases and empty-ID
fallback, different session headers at the same instant, and identical pixels
outside rows 1–5. Static ellipsis checks cover 16/17/128-character IDs, the 63-pixel
fit, exact dot pixels, and header stability while the clock continues updating.
Every validated ASCII ID character has ink in its single centered cell, and
uppercase/lowercase ASCII pairs render identical pixels. ASCII normalization
happens before truncation; unsupported Unicode still occupies one blank cell
without transliteration or case expansion. A separate regression covers mixed
ASCII/Unicode names and truncation while retaining header-only changes. The canonical golden is unchanged.

The test-first baseline explicitly scaffolded the new API as delegation to
`render(now)`, not as a pre-existing API. All three new tests then failed actual
pixel assertions (phase, ellipsis, and charset placement); compilation alone was
not counted as RED. These pure checks do not establish app cache isolation.

Five `session_clock_*` window tests send valid PNGs through the public receiver
and draw with `EmoteApp::show_at` at fixed instants, inspecting egui texture deltas
and meshes. They cover independent identity keys with different display names
(including static long-name ellipsis), date at 0/10s and name at 5s, no upload within
an unchanged 500ms/name step, stable handles across phases,
unchanged normal-neighbor pixels/geometry/uploads, normal-state cache restoration,
clock texture release on removal, final-close date-only fallback despite retained
ID, replacement cleanup, and legacy clocks both with and without a name. The added
rename test keeps identical PNGs and one 500ms bucket, checking immediate title,
Unicode metadata, folder reset and missing-name updates without uploading the
neighbor's texture or changing geometry/handles/native-size commands. The date-only shared-texture
scaffold compiled, then failed three functional assertions (independent handles
and session RGBA); legacy and the existing mixed-tile regression passed. After
keyed-cache wiring all five focused tests passed. Logs are under
`target/compact-clock-session-header/session-cache-{red,green}.log`.
These header/cache checks do not establish compact clock layout or native visual acceptance.

### Avatar-sized clock geometry checks

Three `compact_clock_*` public HTTP/headless-egui tests now cover mixed-neighbor
mesh/UV/texture/background preservation, allocated-card underlays and complete
four-corner clock UVs (no clipped path pixels). Ordinary → mixed → all-clock
transitions retain the same padded canvas and background bounds; all-clock →
mixed → all-clock also emits no resize commands. The matrix exercises both orientations,
steps x1/x3/x5, DPI 1/1.25/1.61, and opaque No-frame/partial/zero backgrounds.
Single/global checks assert full allocated-surface backgrounds and preserve
date-only fallback, explicit framed-clock preference, root drag and menu access.
Expected card bounds come from ordinary-avatar outputs or supplied surfaces,
not a production clock-geometry helper. Logs use
`target/compact-clock-session-header/compact-*.log`. These tests inspect viewport
commands, not OS-applied sizing, compositor output or human visual acceptance.

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
and forwards its transparent renderer clear color. It uses the production
renderer (WGPU on macOS, Glow elsewhere), without a Glow override. The wrapper's observation
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

Historically, the Glow smoke passed on the macOS development host, as did the then
20 normal tests and the separately requested native startup test, format,
Clippy, and build checks. The post-v0.1.4 production WGPU smoke also passed as
recorded above; neither run alone establishes compositor transparency. The
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

## Avatar-size slider and frame-independent content sizing

The four per-avatar size buttons (128/192/256/384) were replaced by a five-step
egui slider with live x1–x5 value text. The correction round then replaced the
interim "64-point content side" canvas: the canvas now **derives from the
avatar's rendered image**, never the reverse. The step x1–x5 is the points per
source texel of the 64-point image; the composition adds the image's one
source separation row (64×step by 65×step points); a framed tile wraps the
composition so the frame's clear opening (851 of the 1100 artwork columns)
contains it plus three logical points of snapping slack (`(1100, 1092) ×
(65×step + 3) / 851`), absorbing the inward whole-physical-pixel fit and egui's
1/32-point viewport rounding (`GUI_ROUNDING`) so the framed image never loses
a level; unframed tiles shrink to exactly the composition; unframed
multi-avatar cards budget their real four-point insets and five-point gaps
around it; and the waiting surface keeps its native 192×192 square at every
step (the clock fallback uses the composition). Resize dedup, per-session
sizes, stacking, no-drag, and exit behavior are unchanged; `toggle_option` and
`select_menu_option` (which now returns the settle pass after a click, because
the immediate child renders after its root pass closure) tolerate exactly the
deliberate resize commands while still forbidding moves and closes.

First-round RED failed six public tests (slider caption absent, old button
labels, `count × tile` command expectations). Correction RED failed the two
new public regressions against the interim implementation: at x1 the framed
canvas commanded 82.7×82.7 (losing the separation row) instead of 87.9×87.3,
and the multi-avatar canvas 661.8×330.9 instead of 679.9×337.5 — the interim
square canvas made the inward-snapped fit lose a whole step from x2 upward
(x5 drew 256 instead of 320 points, confirmed in a native screenshot).
GREEN passed the full offline suite. The regressions drive all five steps
through `menu::slider_position` with real egui pointer events and assert the
**actual rendered image width and height equal 64×step exactly at 100% DPI**, framed and unframed, single and paired, both orientations, plus the exact
per-step canvas commands, separation-row reclaim on **No frame**, resize dedup,
count retention at the chosen step, and packed-card inset/gap budgeting. At
fractional scale factors (1.25/1.5/2.0) the rendered image keeps at least the
chosen step's whole physical level — never one less — under the existing
whole-physical-pixel policy (`assert_whole_physical_pixels`). Command values
themselves are exact f32 and unaffected by viewport rounding. Rendering, fit
helpers, receiving, and clock behavior are unchanged.

## Fixed root canvas constraints

On Linux only, the non-resizable root requests fixed minimum/maximum bounds and inner size
as one ordered content-size transaction. It first relaxes the old requested
bounds to the component-wise envelope of old and new sizes, then fixes both
bounds to the new size and requests that inner size. This avoids contradictory
intermediate hints when width shrinks while height grows (or vice versa).
The cache records requests, not compositor acknowledgements: actual 192×192
input after a 136×281 request does not cause retry/repaint loops. Options sizing
and native StartDrag handling are unchanged.

`root_fixed_size_transaction_survives_unacknowledged_square_and_restores_modes`
uses public HTTP publication and headless egui input. It checks the default pair's
exact ordered transaction from a still-square root, repeated refusal, accepted
128-point images, card insets/separator, fractional DPI, count changes with an
independent child open, opposite-axis changes, single-card restoration and the
192×192 waiting surface. Existing slider/frame/orientation tests now require the
Linux transaction, or the single upstream InnerSize on Windows/macOS; image and drag/focus assertions
remain intact. These checks establish command dispatch and layout, not OS sizing.

The prior Linux Wayland/Hyprland candidate received human acceptance:
«para mi ya seria una version valida para linux». That is not X11 acceptance
or approval of this exact compact-background candidate. The earlier controlled build's
native 136×281 pair was measured separately. Its later 112×281 width-only target
is historical, superseded by the square-card policy below. Native retesting of
square sizing remains pending. Also check orientation/frame restoration, drag
and options.
No app launch or GUI automation is part of the headless check.

## Controlled cross-platform integration

Production selects the Linux policy with `cfg!(target_os = "linux")`, not runtime
environment detection. The Linux gate includes X11 and Wayland, but native
acceptance is only for the prior Wayland/Hyprland candidate. Windows/macOS regain
the upstream focus latch, no **Close options** row, and one deduplicated ROOT
InnerSize request without min/max changes. Renderer, native shadow, primary
StartDrag, sender-focus, image and TMP-close logic are unchanged.

Test-first extraction preserved the pre-change universal Linux behavior before
`cargo test --offline --locked --lib platform_ -- --nocapture` failed all three
new policy checks (exit 101): missing non-Linux focus dismissal, unwanted button,
and five resize commands instead of one. After gating, the same command passed
3/3 (exit 0). Explicit literal expectations cover Linux, Windows and macOS on the
Linux host; public egui tests separately select literal platform expectations.
`cargo test --offline --locked --test window` passed 63/63 and
`cargo test --offline --locked` passed 130/130 (exit 0). `cargo fmt`,
`cargo clippy --all-targets -- -D warnings` and `cargo build --offline --locked`
also exited 0. These are not native
Windows/macOS execution or independent baseline differential evidence. No CLI
changed, so a CLI differential is inapplicable. Independent platform verification
and native retesting remain pending. The running accepted app was not relaunched
or automated. Historical macOS 54-pass/5-fail and release evidence above remains
separate and is not converted into a green suite result.

## Linux lateral background and repeated options invocation (width-only milestone)

The following width/height measurements and Y-anchor guarantees are historical;
the square-card successor below supersedes only that geometry policy.
Only Linux vertical unframed image/clock cards could narrow their background. The
policy measures the original composition before compacting; it never feeds a
new smaller fit back into its baseline. Two physical pixels of horizontal slack
protect inward pixel snapping. Every candidate must preserve the full quad's
physical dimensions and vertical bounds and keep it clear of rounded corners;
otherwise the original width is retained. Integer physical texel scales,
horizontal rows, frames, waiting surfaces and non-Linux policies are unchanged.
At effective 125% scale, x2 single/pair/triple requests are 104×130, 112×281 and
112×424 logical points. These are derived targets, not hard-coded widths.

`linux_side_compaction_keeps_full_images_and_vertical_geometry` first failed
(exit 101) because the original single-card request was 128×130, not 104×130.
The final focused run passes both side-compaction tests (exit 0). The matrix
covers steps 1–5, scales 1/1.25/1.5/2/1.61, aligned/off-grid origins and 1–3 cards,
including session and final-close fallback clocks. It checks exact physical
image extents/Y anchors, complete UV corners, unchanged backgrounds' heights,
five-point gaps and request/texture deduplication. Floating-point subtraction
and polygon clipping can differ by tiny fractions of a physical pixel: snapped
cases must still lie on the existing strict pixel grid and match integer physical
bounds exactly. Existing fractional fallback is classified from the original
inward-snapped available rectangle, not from the resulting image width; those
cases retain the original canvas and exact image geometry.

A repeated root right-click now closes options on Linux; a third invocation
reopens the same child. Its public-input regression observed RED (no child Close)
and GREEN, including press/release timing and no ROOT Close/drag/focus command.
Action fixtures explicitly dismiss stale options before opening a fresh target;
they do not hide toggle behavior. Windows/macOS retain original invocation policy.
The combined Linux suite passed 133/133, `cargo fmt --check`, all-target Clippy
with `-D warnings`, offline locked build and `git diff --check` (all exit 0).
Native candidate acceptance and Windows/macOS target execution remain separate.

## Linux square-card successor

Linux active vertical unframed cards now prioritize square contours, allowing a
minimal width increase and recentering the image within the resized card. The
original pre-square 64×65 layout remains the only image-scale reference. The
policy searches physical-pixel side increments from the complete composition's
lower bound until every card preserves that baseline scale and its full image
quad fits the existing eight-segment-per-quarter rounded polygon. It validates
requested and native floor/nearest/ceil-rounded root sizes, including their
subsequent egui UI-grid rounding. Packed cards retain four-point insets and
five-point clear gaps. Integer display scales also use square cards on Linux.
Global and session clocks use the same fixed baseline scale, with complete date,
header, time and path artwork. Rejected/transient undersized surfaces keep the
old contain fallback, and request deduplication prevents refusal retry loops.

The two `linux_square_*` public HTTP/headless-egui tests replace obsolete
width-only/Y-preservation expectations. RED exited 101 (0 passed, 2 failed): the
single request was still 104×130 rather than square; the matrix also initially
closed its session inside the native-rounding loop and thus changed texture
identity. That fixture now closes only after all rounding variants. GREEN passed
both tests. A strengthened composition oracle initially reconstructed padding
by subtracting logical f32 values; exact integer physical comparisons corrected
that oracle without changing source geometry or weakening image/grid guards.
The matrix covers steps 1–5, PPP 1/1.25/1.5/2/1.61, origins (0,0) and
(0.21875,0.34375), counts 1–3, avatars/session clocks/global fallback, and all
four root-size variants. It compares against the original inward-fit physical
extent, classifies fractional fallback from the original available rectangle,
checks full UV corners (1e-6 existing float tolerance), exact snapped physical
extents/grid, rounded-polygon containment, centered top-padding composition,
square contours within one physical pixel, gaps, ordering and request/texture
reuse. A policy unit test checks non-Linux exclusion, invalid inputs and the
1/64-card boundary. Other existing image/clock/menu/drag/focus assertions remain.

Observed aligned x2 requests at effective PPP 1.25 are **108×108** (single),
**112.8125×230.625** (pair), and **113.625×350.875** (triple), each retaining
**128×128 physical image pixels**. At PPP 1, the pair is **138×281**, while a
single card is **134×134**. These are computed headless results, not hard-coded
production sizes or native Wayland/X11 acceptance. The old width-only single
104×130 and pair 112×281 are not current promises. Horizontal/framed/waiting and
Windows/macOS geometry remain unchanged; native target runs are unavailable.
The historical macOS 54-pass/5-fail result and release evidence remain separate.

Final checks: `cargo test --offline --locked --test window` passed 66/66;
`cargo test --offline --locked` passed 134/134. Formatting, all-target Clippy with
warnings denied and offline locked build passed. No GUI/app operations, sender
configuration, dependencies, commits or release actions were performed.

## Manual verification — remaining scope

Local macOS production WGPU transparency with sender alpha preservation is
human-confirmed above; it is no longer pending acceptance. Use this checklist
for the remaining detailed checks, not as evidence that every item was exercised:

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
  Close or let one expire and check the other remains. Explicitly close the last
  active session with no legacy avatar and check an immediate internal Lemmings
  pixel clock (local date and 24h time, walking animation) with no avatar or
  decorative frame, advancing every 500ms. Check drag/options still work and a
  new session replaces the clock. Separately let the final session time out: its
  image must remain, even if a delayed close then arrives. Send a session frame
  with `X-Emote-State: clock` and check that session's tile renders the internal
  pixel clock beside active avatars; send an ordinary image and check it returns.
  Verify legacy singleton
  traffic still renders. Many sessions may exceed monitor width; no overflow
  handling is promised. These multi-avatar native checks have not been performed.
- [ ] With another ordinary window overlapping, visually check always-on-top
  behavior. Check primary drag, right-click avatar-size slider steps (x1–x5), and
  Exit. Confirm menu selection does not start a drag and exit releases port 8001.
- [ ] At a 128-pixel avatar size, open options and move the pointer into/out of the
  options window. On Linux, focus another window: options must stay visible;
  **Close options** is Linux-only. On Windows/macOS, gaining then losing focus
  must dismiss options without an extra button or height row. Confirm
  Escape and native close dismiss only options and allow
  reopening; existing checkbox/card/stacking actions still close options, while
  sliders and the color picker stay open. On Wayland record compositor-selected
  placement without requiring the window to appear beside the click. This is a
  pending native check, not established by headless input/viewport assertions.
- [ ] On a contrasting desktop background, confirm the initial window surface is
  black at 45% transparency. With **Transparent background** checked, check that unused space
  and the added one-source-row top padding reveal the desktop, fully transparent
  PNG pixels reveal it, and partial-alpha pixels blend with it. Confirm opaque
  black PNG pixels, including the sender's bottom gap, remain black; the full
  image and nearest-neighbor edges must remain unchanged.
- [ ] Open **Background color**, choose a nonblack RGB color, and collapse it.
  Confirm all picker controls fit the independent options window, which shrinks
  again after collapse. Check waiting, avatars, session clocks and the final-close
  fallback at opaque, partial and zero opacity; separators stay clear. Opaque
  black source pixels remain black, and source alpha/clock artwork stay unchanged.
  Restart (also from a different project directory) and confirm the selected RGB
  returns with the existing 45% transparency, x2, No frame and Vertical defaults.
  Do not change sender or HTML/serial LED configuration for this check.
- [ ] On Windows, inspect the transparent window against a contrasting desktop:
  confirm the native rounded shadow/thin perimeter is absent, then toggle back
  to opaque and confirm its previous shadow policy returns. This requires a
  compositor-level visual check; the framebuffer smoke cannot prove it.
- [ ] Toggle back to black, then enable transparency again without sending a new
  frame. Check there is no stale opaque surface, image loss, or unintended
  move/resize/close. Check menu readability and primary drag in both modes; the
  transparent surface must not become click-through. Restart and confirm the
  option resets to checked at 45% transparency.
- [ ] Repeat applicable build/run and human window checks on Windows and Linux;
  record OS, graphics backend, and display scale. On Wayland, record compositor
  support/policy for topmost placement, native moves, and desktop alpha rather
  than promising them. Record whether transparency is supported by each tested
  backend/compositor; headless checks cannot establish it.

Eframe/egui 0.37.0 uses WGPU on macOS and Glow on Windows/Linux; X11 and
Wayland source backends are enabled. macOS is locally tested and compatible;
Windows was not rerun for this candidate. Linux headless/build evidence is current;
prior native acceptance is limited to Wayland/Hyprland, not X11 or this exact candidate.
Historical Windows headless, build and native-smoke results are recorded above.
A framebuffer pixel proves
part of the native rendering pipeline, not topmost placement, successful OS
dragging, native menu usability, every display scale, or cross-platform parity.

Continue with the [architecture study guide](architecture.md) for ownership and
threading, or the [wire protocol](protocol.md) for exact input and trust limits.
