# Study the receiver-to-window path

The application turns bounded local HTTP PNG uploads into reusable per-session GPU
textures and neighboring tiles in one widening window. This guide follows the
ownership and synchronization choices in the commented Rust source; it is not a
proposal for a larger service.

## Start with the data flow

```text
Native sender: POST /emote with a 64×64 PNG
    │ TCP on 127.0.0.1:8001
    ▼
Receiver worker: Hyper → request policy → bounded body → PNG decode
    │ publish a complete Frame; rejected requests leave the registry unchanged
    ▼
LatestFrame: shared mutex-protected bounded SessionRegistry
    │ release lock, then request_repaint() for visible changes
    ▼
Main/native event loop: EmoteApp::show → atomic display_snapshot()
    │ per-key new Arc identity? upload/update texture; otherwise reuse it
    ▼
egui + Glow → frameless native window; opaque black by default, optional transparency
```

There is no pending-frame queue, avatar polling timer, persistence layer, or second
metadata display. Sender labels are already part of the pixels. Only clock mode
schedules a one-second GUI repaint, reading `chrono::Local::now()` once for local
`HH:MM:SS` and `DD/MM/YYYY`, followed by `Waiting Agents...`. Chrono uses the
system time zone, including daylight-saving rules. The
[protocol guide](protocol.md) is the reference for headers, response bodies,
validation order, resource limits, and the local trust boundary.

## Read the source in this order

| Read | Question to follow |
| --- | --- |
| [`src/main.rs`](../src/main.rs) | How does app creation connect receiver notifications to egui? |
| [`src/receiver.rs`](../src/receiver.rs): `Frame`, `LatestFrame`, `Receiver` | Who owns pixels, the listener, and shutdown? |
| Same file: `serve`, `handle`, `decode` | When is a request allowed to become visible? |
| [`src/app.rs`](../src/app.rs): `EmoteApp::new`, `show` | How does a snapshot become a texture without repeated uploads? |
| Same file: `native_options` and viewport commands | Which behaviors are requested from the OS rather than drawn by egui? |
| [`tests/window.rs`](../tests/window.rs) and [testing guide](testing.md) | Which properties can be checked without opening a window? |

[`src/lib.rs`](../src/lib.rs) exposes the receiver and app modules so the binary,
integration tests, and smoke example use the same implementation.

## Ownership makes shutdown explicit

`Receiver::start` uses the fixed production address. `Receiver::bind` also permits
an ephemeral port (`127.0.0.1:0`) for tests and embedding. Binding happens before
the worker starts, so the caller receives bind errors directly; there is no
fallback address.

`Receiver` owns the cancellation sender and the worker's `JoinHandle`. The worker
owns a current-thread Tokio runtime, listener, and connection tasks. `EmoteApp`
retains the receiver in `_receiver`: the underscore suppresses an unused-field
warning, **not** destruction. The field exists to keep the service alive for the
app's lifetime.

On app destruction, `Receiver::drop` takes and sends the one-shot stop signal,
then takes and joins the worker. `serve` prioritizes cancellation, aborts active
connections, and drains their tasks. It does not wait for incomplete clients to
finish their requests. `shutdown(self)` simply consumes and drops the receiver,
using the same path rather than maintaining another shutdown implementation.

The `Option` fields for the stop sender and worker allow `take()` to move those
owned values out of `&mut self` exactly once. Send/join errors are deliberately
ignored in `Drop`; shutdown is not an error-reporting API. A retained latest-frame
handle or snapshot remains readable after the listener has stopped.

## Keep native work on the main thread

| Main/native event loop | Owned network worker |
| --- | --- |
| Runs eframe and `EmoteApp::show` | Runs one current-thread Tokio runtime |
| Reads a short-lived snapshot | Accepts bounded concurrent connection tasks |
| Owns and updates per-key GPU textures | Parses HTTP, validates policy, decodes PNG |
| Paints image/placeholder and a separate immediate options viewport | Publishes complete immutable snapshots |
| Sends resize, close, and drag viewport commands | Calls the repaint notification after publication |
| Drops the app and joins the receiver | Cancels connection tasks when stopped |

Tokio tasks overlap socket waits; they are not a thread per client. PNG decoding
is synchronous on this worker with small bounded inputs. The repaint closure in
`main` captures a cloned egui context, a cheap thread-safe handle. It wakes the
native loop; it does not render, touch the GPU, or wait for the GUI to respond.

The callback must be quick, nonblocking, and nonpanicking. It must not drop its
own receiver or wait for the GUI thread: the GUI may itself be waiting in the
receiver's synchronous join during shutdown.

## Unpack the shared snapshots

`LatestFrame` shares an `Arc<Mutex<SessionRegistry>>`. The registry serializes
frame publication, stream ordering, heartbeat leases, close, and expiry. Its
`snapshots()` method clones an ordered `Vec<SessionSnapshot>` under one short lock:

| Field | Purpose |
| --- | --- |
| `session_id: Option<String>` | Stable texture-cache key; `None` is the legacy singleton |
| `frame: Arc<Frame>` | Immutable shared 64 × 64 × 4 unpremultiplied RGBA8 pixels |
| `active: bool` | Active avatar, or the inactive final image retained when none remain |

Active avatars appear in stable first-seen order. When none remain, the final
removed avatar remains in compatibility snapshots. Timeout keeps it visible;
explicit close of the final active session instead displays the local clock when
no legacy avatar remains. Initial Waiting and a delayed close after timeout do
not enable the clock. A new or reactivated session replaces either presentation,
rather than appending to a dead placeholder. Legacy traffic is a nonexpiring singleton
ordered before session keys. The GUI uses registry ordering, never HashMap order.

`LatestFrame::get()` remains a compatibility API returning the last accepted image
from any session (initially `None`), unaffected by lifecycle-only traffic. Both
APIs release the mutex before the caller converts pixels, uploads, or paints.
The app's crate-private `display_snapshot()` reads snapshots and the registry's
`show_clock()` flag together under that same lock. Clock mode retains cached
textures but paints neither avatar nor frame; drag and options stay available.
Pixel Arcs remain readable after replacement and shutdown.

Repaint callbacks run outside the lock after image publication or visible lifecycle
changes. Heartbeats for already-live sessions only renew leases; they neither
replace image Arcs nor request a repaint. Rejected input changes neither data nor notifications.

Registry lock sites use `expect("frame lock poisoned")`. Rust poisons a mutex when a
thread panics while holding its guard; this implementation treats that as an
invariant failure, not a recoverable input error. The tiny locked sections contain
no decoding or user callback. There is no poison-recovery mechanism or guarantee
of continuing after arbitrary panics.

## Latest wins; notification is not a queue

Suppose valid frames A, B, and C finish before the next GUI pass. The slot ends
with C, and the GUI uploads C once. It does not drain A and B later. Concurrent
legacy senders have no request-arrival ordering guarantee: **publication order
wins**. An older-started legacy request can finish last and replace a newer-started
request. Session-aware traffic instead rechecks stream and sequence atomically at
publication; latest wins independently for each session.

The app retains the last uploaded `Arc` in each keyed texture-cache entry.
`Arc::ptr_eq` asks whether the current snapshot is the same allocation, not whether all pixel bytes compare
equal. An unchanged snapshot causes no upload. A newly accepted frame with
identical pixels is still a new allocation and can cause an upload.

On each key's first image, `load_texture` creates a texture. Later images use
`TextureHandle::set` on that reusable handle. Missing snapshot keys are removed
from the cache, dropping their GPU handles. A retained final avatar keeps the same
handle and Arc without another upload. Legacy texture name `emote` remains stable;
session textures use `emote-session:<id>`. Repaint requests may coalesce and carry
no pixels. Other library consumers retaining Arcs own their memory use. Registry
caps and finite replay protection are documented in the [protocol](protocol.md).

## Pixels and native window policy are separate

PNG decoding normalizes supported inputs to RGBA8 while preserving alpha.
`ColorImage::from_rgba_unmultiplied` tells egui that RGB has **not** already been
multiplied by alpha, so egui performs the blending conversion correctly. Passing
these bytes as premultiplied data would misinterpret partially transparent colors.

`TextureOptions::NEAREST` prevents interpolation between adjacent pixel-art
texels. Menu sizes (128, 192, 256, 384 logical pixels) are integer multiples of
64; physical dimensions also depend on the display's scale factor.

`native_options` requests a transparency-capable, fixed, frameless, always-on-top
initial square with Glow rendering. It grows into a row of square avatar tiles
by default. **Vertical / Horizontal** selects either a column or a row using
exclusive radio controls; the private orientation flag starts horizontal and
resets on restart. The size menu chooses each tile's side length. Native size is
`count × tile size` by `tile size` horizontally, or `tile size` by
`count × tile size` vertically (one tile while waiting). Resize commands are
sent only when count, chosen size or orientation changes, not on image updates
or heartbeats.
The options use `show_viewport_immediate` with a stable child viewport ID and an
independent surface fitted to its controls with eight logical pixels of outer
padding, rather than a fixed oversized rectangle. Rendering is synchronous on the UI
thread: the child borrows the app's settings without a channel or shared mutable
state. Exit explicitly targets ROOT; Escape, native close, selection, and focus
loss after initial focus dismiss only the child. Where available, the root's
monitor-space inner origin plus the click position places the child; unsupported
placement falls back to the native window manager. It is not an embedded egui
popup clipped to a small avatar. Native focus/placement still needs OS checks.

The row or column may exceed monitor dimensions; no wrapping, scrolling, or overflow policy is
provided. `EmoteApp::clear_color` clears to transparent RGBA;
`show` paints an opaque black surface by default. A private integer percentage
starts at 100 (opaque). The independent **Transparent background** flag starts
false, disabling **Background transparency** at 0%. Checking it initially selects
100% transparency; the enabled slider converts transparency to opacity as
`100 - transparency`, without tinting image meshes or changing cached pixels.
Black underlay alpha is opacity scaled and rounded to 0–255. Zero or partial
transparency does not uncheck the flag. Unchecking forces opacity to 100 and
disables the slider at 0% transparency; no previous percentage is restored.
Both values live only in the app instance and reset on restart. Frame and native
shadow policy use actual opacity, not the flag, so checked-at-zero remains opaque.
The options viewport remains opaque. At intermediate percentages, a single
avatar's underlay and image mesh share the existing eight-point rounded contour,
without relying on the Windows shadow. Fully clear single avatars and opaque
single avatars with **No frame** checked retain their original fit; framed single
avatars use the existing artwork opening. Stacked-card contours and spacing are unchanged.
No click-through is enabled.

Both modes use the same texture and full image UVs. The complete 64×64 image is
uniformly fitted within each tile into a centered virtual 64×65 composition, with
one source row of padding above it. That added padding becomes transparent when enabled; opaque
black pixels already in the PNG, including the sender's five-row bottom gap,
remain black. No color-keying, cropping, or pixel rewriting occurs, and toggling
the background does not require re-uploading an unchanged frame. All opaque
avatar tiles, including a single avatar, use the embedded `assets/avatar-frame.png`
unless **No frame** is checked.
The fit uses the largest whole number of physical pixels per source pixel for the
display's current `pixels_per_point` (including fractional scale factors such as
1.25 or 1.5) and snaps every image edge onto an integer physical pixel, so
`NEAREST` always renders square, unskewed pixels. Faithful source pixels take
priority over filling the tile: the image may be smaller with larger margins
than a fractional fit. Only when the available area is smaller than 64×65
physical pixels does the fit degrade to the fractional contain scale, still
without overflowing. The decorative frame keeps its fractional artwork fit.
One lazily loaded shared texture holds the cropped RGBA artwork. Its 851 × 851
clear opening contains the complete virtual composition; the frame is painted
above the avatar without covering labels. Uniform fitting preserves the frame's
aspect ratio even during narrow transient resizes. Any background opacity below
100 automatically uses the compact unframed layout, without modifying the
independent **No frame** preference for opaque mode. The frame is loaded only
when an actual avatar needs it, never for the empty waiting surface. A single
avatar uses the same frame/opening composition as each multi-avatar tile; checking
**No frame** restores its original borderless fit. See the
[asset preparation contract](../assets/README.md).

The independent **No frame** checkbox starts unchecked and resets on restart.
When checked, or whenever background opacity is below 100%, all avatars
omit the artwork. For multiple avatars, five logical pixels are reserved between card contours, then
the remaining extent is divided into equal card regions along the selected axis.
Each card independently fits the original 64-by-65 composition inside a four-point
inset, including the one-source-row top compensation; there is no global image
group offset. Margins are therefore consistent across first, middle and last
cards. Bitmap-to-bitmap spacing includes these internal margins; the clear
separator itself is exactly five points. Vertical image size may shrink slightly
to retain the original native window extent while reserving separators.
Horizontal image rows leave vertical strips; vertical columns leave horizontal
strips. Separator strips are left
unpainted over the transparent renderer clear, even with the remaining background
opaque; no PNG pixels are rewritten. Cards use an eight-logical-pixel rounded
outline for both background and image. The textured image mesh is intersected
with the rounded convex card outline and its original image rectangle; UVs are
derived from that unchanged rectangle. Rounding only the background is not
sufficient: opaque image corners can otherwise paint over the lower cutouts.
The draw mesh, not the cached texture, is clipped in both orientations, including
when the remaining background is fully transparent. It does not change the
background setting, root window dimensions or the unframed single-avatar fit. During transient
surfaces too narrow for the gaps, spacing is clamped rather than creating
negative image sizes. Toggling either presentation option reuses cached textures.

Drag, resize, and close are viewport commands asking the OS to act, not promises
that every compositor implements them identically. Desktop alpha also depends
on the native backend/compositor. See [platform and evidence limits](testing.md)
before drawing cross-platform conclusions.

## Small study exercises — not implemented features

These are reading/prediction exercises, not additional product capabilities:

1. Trace `verify_completion_order_snapshot_and_active_shutdown` in
   [`tests/receiver.rs`](../tests/receiver.rs). Why does the slower request win,
   and why does the retained earlier snapshot stay unchanged?
2. Follow `burst_coalesces_and_app_drop_cancels_partial_client` in
   [`tests/window.rs`](../tests/window.rs). Predict the number of texture updates
   after three publications before one GUI pass, then compare with its assertion.
3. Imagine moving `repaint()` inside the mutex guard. Explain what would happen
   if a callback called `LatestFrame::get`. Do not make that change.
4. Follow `with_transparent(true)`, `clear_color`, the conditional black surface,
   and `from_rgba_unmultiplied`. Explain why the default still looks opaque, why
   enabling transparency cannot remove opaque black image pixels, and why neither
   mode enables click-through.
