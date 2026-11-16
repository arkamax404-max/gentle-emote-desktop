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

The current suite contains **49 non-ignored tests: 19 receiver, 4 session lifecycle,
25 window, and 1 private shadow-policy unit test**. The earlier frame/compact-popup checks
passed all 15 then-existing window tests and the shadow policy; the earlier lifecycle check
passed 4/4. The most recent full-suite attempt failed one receiver test with
Windows OS 10053; it was not rerun for this UI update. See the evidence below.
On macOS, Cargo also lists one ignored native startup test; it is not part of
those 49. On other platforms,
that startup test is excluded by a target guard.
A headless suite needs no running GUI, but still compiles native dependencies;
Linux builds need suitable graphics/toolchain libraries.

| Test file | What its passing tests establish |
| --- | --- |
| [`tests/receiver.rs`](../tests/receiver.rs), 19 | Real TCP/HTTP validation, exact status/body assertions, rejection preserving snapshot and notification count, byte/deadline/connection bounds, PNG normalization, publication ordering, snapshots, bind failures, and owned shutdown; includes three response-reader guards |
| [`tests/window.rs`](../tests/window.rs), 33 | Headless egui options, waiting/menu text, viewport commands, opaque-default/background-toggle round trip with the 45%-transparency toggle-on default and transparent renderer clear, original opaque black/partial-alpha/transparent texture pixels and nearest filtering, full-image padding geometry with whole-physical-pixel scales and snapped edges at every supported `pixels_per_point` (documented fractional fallback below 64×65 physical pixels), no redundant uploads (including after a background toggle), burst coalescing, and app-owned receiver shutdown |
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
- [ ] On a contrasting desktop background, confirm the initial window surface is
  opaque black. Right-click → **Transparent background**: check that unused space
  and the added one-source-row top padding reveal the desktop, fully transparent
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
