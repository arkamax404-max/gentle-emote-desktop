# Study the receiver-to-window path

The application turns bounded local HTTP PNG uploads into a single reusable GPU
texture. This guide follows the ownership and synchronization choices in the
commented Rust source; it is not a proposal for a larger service.

## Start with the data flow

```text
Native sender: POST /emote with a 64×64 PNG
    │ TCP on 127.0.0.1:8001
    ▼
Receiver worker: Hyper → request policy → bounded body → PNG decode
    │ publish a complete Frame; rejected requests leave the slot unchanged
    ▼
LatestFrame: Arc<Mutex<Option<Arc<Frame>>>>
    │ release lock, then request_repaint()
    ▼
Main/native event loop: EmoteApp::show → clone latest snapshot
    │ new Arc identity? upload/update texture; otherwise reuse it
    ▼
egui + Glow → opaque, frameless native window
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
| Owns and updates the GPU texture | Parses HTTP, validates policy, decodes PNG |
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

## Unpack the shared snapshot type

Read `Arc<Mutex<Option<Arc<Frame>>>>` from the inside out:

| Part | Purpose |
| --- | --- |
| `Frame` | A vector of exactly 64 × 64 × 4 row-major, unpremultiplied RGBA8 bytes |
| Inner `Arc<Frame>` | Shares one frame allocation without copying pixels; treat published pixels as immutable |
| `Option` | `None` before the first accepted frame, `Some` afterward |
| `Mutex` | Serializes replacement of the slot and cloning of its current value |
| Outer `Arc` | Lets the worker and GUI share the slot's lifetime |

`LatestFrame::get` holds the mutex only while cloning the `Option<Arc<Frame>>`.
The returned snapshot keeps its pixels alive after the lock is released, even if
another image replaces the slot. The GUI never keeps this mutex locked while
converting pixels, uploading textures, or painting.

`publish` swaps in a complete new `Arc`. Only after it returns does `handle` call
`repaint()`. Calling the callback outside the lock avoids holding a shared-state
lock across external code and allows the callback to read the slot safely.
Rejected input neither replaces the slot nor invokes the callback.

Both lock sites use `expect("frame lock poisoned")`. Rust poisons a mutex when a
thread panics while holding its guard; this implementation treats that as an
invariant failure, not a recoverable input error. The tiny locked sections contain
no decoding or user callback. There is no poison-recovery mechanism or guarantee
of continuing after arbitrary panics.

## Latest wins; notification is not a queue

Suppose valid frames A, B, and C finish before the next GUI pass. The slot ends
with C, and the GUI uploads C once. It does not drain A and B later. Concurrent
senders have no request-arrival ordering guarantee: **publication order wins**.
An older-started request can finish last and replace a newer-started request.

The app retains the last uploaded `Arc` in `uploaded`. `Arc::ptr_eq` asks whether
the current snapshot is the same allocation, not whether all pixel bytes compare
equal. An unchanged snapshot causes no upload. A newly accepted frame with
identical pixels is still a new allocation and can cause an upload.

On the first image, `load_texture` creates a texture. Later images use
`TextureHandle::set` on that reusable handle. Repaint requests may coalesce;
notifications carry no image payload. The receiver retains one current frame,
and the app may temporarily retain an older uploaded snapshot. Other library
consumers that retain snapshots are responsible for their own memory use.

## Pixels and native window policy are separate

PNG decoding normalizes supported inputs to RGBA8 while preserving alpha.
`ColorImage::from_rgba_unmultiplied` tells egui that RGB has **not** already been
multiplied by alpha, so egui performs the blending conversion correctly. Passing
these bytes as premultiplied data would misinterpret partially transparent colors.

`TextureOptions::NEAREST` prevents interpolation between adjacent pixel-art
texels. Menu sizes (128, 192, 256, 384 logical pixels) are integer multiples of
64; physical dimensions also depend on the display's scale factor.

The native window is opaque: PNG alpha blends against its painted background,
not the desktop. `native_options` requests a fixed, frameless, always-on-top
square with Glow rendering. Drag, resize, and close are viewport commands asking
the OS to act, not promises that every compositor implements them identically.
See [platform and evidence limits](testing.md) before drawing cross-platform
conclusions.

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
4. Find `with_transparent(false)` and `from_rgba_unmultiplied`. Explain why PNG
   alpha support does not imply a transparent or click-through desktop window.
