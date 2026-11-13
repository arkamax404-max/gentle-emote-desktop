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
Main/native event loop: EmoteApp::show → ordered snapshots()
    │ per-key new Arc identity? upload/update texture; otherwise reuse it
    ▼
egui + Glow → frameless native window; opaque black by default, optional transparency
```

There is no pending-frame queue, GUI polling timer, persistence layer, or second
metadata display. Sender labels are already part of the pixels. The
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
| Paints image, placeholder, and context menu | Publishes complete immutable snapshots |
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
removed avatar stays visible; a new or reactivated session replaces it, rather
than appending to a dead placeholder. Legacy traffic is a nonexpiring singleton
ordered before session keys. The GUI uses registry ordering, never HashMap order.

`LatestFrame::get()` remains a compatibility API returning the last accepted image
from any session (initially `None`), unaffected by lifecycle-only traffic. Both
APIs release the mutex before the caller converts pixels, uploads, or paints.
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
initial square with Glow rendering. It widens into a horizontal row of square
avatar tiles. The size menu chooses each tile's side length, so native size is
`count × tile size` by `tile size` (one tile while waiting). Resize commands are
sent only when count or chosen size changes, not on image updates or heartbeats.
The row may exceed monitor width; no wrapping, scrolling, or overflow policy is
provided. `EmoteApp::clear_color` clears to transparent RGBA;
`show` paints an opaque black surface by default. The right-click checkbox
**Transparent background** controls a private boolean initialized to `false`.
Enabling it omits that black surface, so unused space and PNG alpha can reveal
the desktop where the native compositor supports transparency. Disabling it
restores black. The choice lives only in the app instance and resets on restart;
it is not a saved setting and does not enable click-through.

Both modes use the same texture and full image UVs. The complete 64×64 image is
uniformly fitted within each tile into a centered virtual 64×69 composition, with
five source rows of padding above it. That added padding becomes transparent when enabled; opaque
black pixels already in the PNG, including the sender's five-row bottom gap,
remain black. No color-keying, cropping, or pixel rewriting occurs, and toggling
the background does not require re-uploading an unchanged frame.

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
