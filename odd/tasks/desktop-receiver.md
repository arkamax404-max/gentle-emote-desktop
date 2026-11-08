# Gentle Emote Desktop
Objective: build a local Rust desktop receiver; branch: `feat/desktop-receiver`; delivery: local work-unit commits only, no remote or publication.
Checks: `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, native build and local HTTP/GUI smoke. Use `/Users/santiagoperez/.cargo/bin/cargo`.

## Specs
S1. "Quiero una aplicacion de escritorio que sea compatible con mac, windows y linux para mostrar el emote en una mini ventana sin marco que este always on top, la ejecución por el momento sería local en la misma maquina que corre el agente".
S2. "ok, creemos el nuevo proyecto y vayamos con rust".
S3. "no escatimes en comentarios en el código para poder estudiarlo y haz una documentación prolija".
S4. The user answered "si" to creating `/Users/santiagoperez/Development/gentle-emote-desktop` and separately "si" to installing Rust under `/Users/santiagoperez/.cargo` and `/Users/santiagoperez/.rustup`, without modifying PATH or shell startup files. No publishing or other system installation is authorized.

Implementation assumptions accepted from the proposal: egui/eframe, existing PNG `POST /emote` protocol, loopback-only receiver on port 8001 (simulator retains 8000); last valid image wins without an unbounded queue; nearest-neighbor rendering; drag to move; context menu for size and exit. No transparent-PNG conversion or click-through in this version. Platform support is a source/build target, not a claim of Windows/Linux execution without evidence. Wayland topmost behavior depends on the compositor. Comments and repository documentation default to English. Global agent targets are not changed automatically.

## Tasks
T1 | S2,S4 | inline | done | Install/verify Rust stable, rustfmt, clippy; rustc/cargo 1.99.1; official installer hash checked; no PATH changes. Commit: setup evidence in bootstrap document.
T2 | S1,S2 | delegated | in_progress | Implement bounded local HTTP/PNG receiver with public socket tests, RED/GREEN, and protocol notes. Risk: items 2/4 (local HTTP trust boundary and cross-thread latest-frame state). Commit: pending.
T3 | S1,S2,S3 | delegated | pending | Implement educational native egui window integration, drag, size/exit controls, nearest rendering, and event-driven redraw; compile and launch macOS smoke. Commit: pending.
T4 | S1,S3,S4 | delegated + parent verification | pending | Finish study-oriented README/architecture/protocol/testing/platform docs; run full checks and real sender integration; independent verification of HTTP/concurrency and platform-claim limits. Commit: pending.

## Log
L1. Original request verbatim: "muy bien, vamos a abrir un nuevo proyecto asociado a nuetrs gentle-emote y dime si me recomiendas usar rust o go. Quiero una aplicacion de escritorio que sea compatible con mac, windows y linux para mostrar el emote en una mini ventana sin marco que este always on top, la ejecución por el momento sería local en la misma maquina que corre el agente".
L2. Implementation authorization verbatim: "ok, creemos el nuevo proyecto y vayamos con rust, no escatimes en comentarios en el código para poder estudiarlo y haz una documentación prolija". User then answered "si" to the proposed absolute project path, and "si" to the explicit rustup installation proposal.
L3. Toolchain setup succeeded: official rustup-init SHA256 validated before execution; --no-modify-path, minimal profile, stable toolchain with rustfmt/clippy. rustc 1.99.1 and cargo 1.99.1. Existing Apple Command Line Tools; macOS 14.8.7 x86_64. No system package installations.
L4. Protocol reconnaissance: current sender emits binary opaque 64x64 PNG with Content-Type image/png, X-Emote-Version 1 and additional state/frame/set/activity/stream headers. Do not redraw metadata labels (already embedded). Receiver must bind 127.0.0.1 only and preserve the previous image on rejection. eframe 0.37.0 requires Rust >=1.95, satisfied here. Windows/Linux actual execution remains unavailable on this host.
L5. Forecast approximately 800–1100 authored lines including teaching comments, tests and docs, plus generated Cargo.lock. Keep coherent work units; no PR/delivery shape chosen because no publishing authorized. About 400 lines per commit is advisory; do not remove explanatory comments/tests or split behavior artificially to hit it.
