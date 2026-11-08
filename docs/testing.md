# Verify behavior without overstating evidence

Start with the headless suite, then use native checks only in a suitable graphical
session. Socket tests, framebuffer capture, human window inspection, and a real
external sender each establish different things. The external sender integration
has **not** been run; Windows/Linux execution and human visual OS-behavior checks
also remain outstanding.

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

The current default suite has **20 non-ignored tests: 14 receiver + 6 window**.
On macOS, Cargo also lists one ignored native startup test; it is not part of
those 20. On other platforms, that startup test is excluded by a target guard.
A headless suite needs no running GUI, but still compiles native dependencies;
Linux builds need suitable graphics/toolchain libraries.

| Test file | What its passing tests establish |
| --- | --- |
| [`tests/receiver.rs`](../tests/receiver.rs), 14 | Real TCP/HTTP validation, exact status/body assertions, rejection preserving snapshot and notification count, byte/deadline/connection bounds, PNG normalization, publication ordering, snapshots, bind failures, and owned shutdown; includes three response-reader guards |
| [`tests/window.rs`](../tests/window.rs), 6 | Headless egui options, waiting/menu text, viewport commands, exact texture pixels and nearest filtering, no redundant uploads, burst coalescing, and app-owned receiver shutdown |
| [`tests/startup.rs`](../tests/startup.rs), 1 ignored on macOS | An explicitly launched production process reports an occupied fixed port and terminates as expected on that host |

Receiver and headless window tests bind `127.0.0.1:0`: the OS chooses an
**ephemeral port**, and clients use `local_addr()` to discover it. They do not
need production port 8001 or an existing sender. They create no scratch
directories. Window tests inspect egui outputs without a native event loop;
asserting `StartDrag` or `AlwaysOnTop` does not prove the OS honored the request.

For focused investigation, the existing test targets can be run separately:

```sh
cargo test --test receiver
cargo test --test window
```

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
`EmoteApp` and `native_options`; the wrapper's observation and deadline are not
product behavior. Its sequence is:

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

The smoke passed on the macOS development host, as did the 20 normal tests and
the separately requested native startup test, format, Clippy, and build checks.
These are recorded implementation results, not a claim that the documentation
change reran them. The smoke uses its own test sender, **not the actual external
Gentle Emote sender**, and does not use production port 8001.

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
- [ ] With another ordinary window overlapping, visually check always-on-top
  behavior. Check primary drag, right-click size choices (128/192/256/384), and
  Exit. Confirm menu selection does not start a drag and exit releases port 8001.
- [ ] Repeat applicable build/run and human window checks on Windows and Linux;
  record OS, graphics backend, and display scale. On Wayland, record compositor
  support/policy for topmost placement and native moves rather than promising it.

Eframe/egui 0.37.0 with Glow is the implementation choice; X11 and Wayland source
backends are enabled. macOS, Windows, and Linux are **source/build targets**.
Only macOS execution evidence is currently recorded. A framebuffer pixel proves
part of the native rendering pipeline, not topmost placement, successful OS
dragging, native menu usability, every display scale, or cross-platform parity.

Continue with the [architecture study guide](architecture.md) for ownership and
threading, or the [wire protocol](protocol.md) for exact input and trust limits.
