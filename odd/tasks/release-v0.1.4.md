# Release v0.1.4
Objective: create the next Windows x64 release from current main and verify the README is up to date.
Branch: main; delivery: direct main integration + tag + GitHub release; checks: frozen-source fmt/test/clippy/build, package audit.

## Specs
S1. «vamos a crear una nueva release y verifica que el readme este al día»

## Tasks
T1 | S1 | parent | done | README/docs test count corrected 49→120 (Windows); commit c3f2397 pushed.
T2 | S1 | parent | done | Full all-targets Clippy surfaced 2 pre-existing errors in v0.1.4 source; fixed (background_color as_chunks::<2>; sessions apply_request #[allow(too_many_arguments)]); commit 70f958c pushed.
T3 | S1 | parent | done | Frozen build v014b PASS: 120 tests, fmt, clippy -D warnings, release build exit 0. Receipt target/release-v0.1.4/attempts/v014b/checks-passed.json.
T4 | S1 | parent | done | Package PASS: ZIP 4,480,105 bytes SHA256 302de626ad6e15a48b641297b5e599cdce3fc87111eb8b4a887d32ffc74de309, 313 entries, 149 deps, 302 license files (reused from v0.1.3; Cargo.toml/lock byte-identical). EXE 7,998,976 bytes.
T5 | S1 | parent | done | Published GitHub release v0.1.4 (latest, not draft/prerelease), tag object 9f651c5 peels 70f958c. FINALRELEASE_PUBLICATION_PASS, two assets readback-verified.

## Log
L1. User: "vamos a crear una nueva release y verifica que el readme este al día".
L2. Latest published was v0.1.3 (tag d26c3ef). New content since v0.1.3: desktop focus request (X-Emote-Focus-Request-Id/X-Emote-Focus-Ack), background RGB color persistence, clock-card dismissal/reorder; already pushed on main 0833a6a.
L3. README and docs/testing.md claimed "49 tests"; actual Windows suite is 120 (34 receiver, 56 window, 10 sessions, 3 background_color, 9 idle_clock_image, 8 lib). Corrected; commit c3f2397.
L4. Frozen build attempt v014 failed Clippy -D warnings: src/background_color.rs:48 chunks_exact(2) and src/sessions.rs:291 apply_request 8 args. Both are v0.1.4 code never covered by the file-scoped Clippy of the features. Fixed; commit 70f958c; re-ran attempt v014b (PASS).
L5. Dependency reconciliation generated and byte-verified under Windows PowerShell 5.1 (pwsh 7 ConvertTo-Json differs; must use powershell.exe -File). Cargo.toml/Cargo.lock OIDs identical to v0.1.3 (7a0d9748…/351cbf6b…).
L6. gh auth status was exit 1 due to stale santiagopm (default) invalid-token entry; user authorized gh auth logout -h github.com -u santiagopm; arkamax404-max (repo/workflow) active, exit 0.
L7. Publisher FINALRELEASE_PUBLICATION_PASS: remote tag v0.1.4 → 70f958c, release isDraft=false/isPrerelease=false, latest, two assets (ZIP digest sha256:302de626…; sidecar 110 bytes). URL https://github.com/arkamax404-max/gentle-emote-desktop/releases/tag/v0.1.4.
