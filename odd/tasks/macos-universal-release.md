# macOS Universal 2 prerelease
Publish v0.1.5-rc.1 with an ad-hoc-signed Universal 2 app, dependency notices and explicit runtime limits.
Local Intel execution is verified; native Apple Silicon execution remains unverified. Windows releases stay unchanged.

## Specs
S1. User: "la utilidad de escritorio para mac se puede empauetar para crear una release de MacOs"; accepted local .app/ZIP packaging with "ok".
S2. User: "yo creo que lo podemos subir, pero tenemos que hacer algo para los macs con Apple Silicon"; selected "Preparar Universal 2 — recomendado" and authorized the aarch64 Rust target installation.
S3. User: "publicalo"; selected "Sí, publicar la prerelease recomendada": v0.1.5-rc.1, public GitHub prerelease, dependency-license reads under /Users/santiagoperez/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f, and ad-hoc signing of the new complete .app including resources. No Developer ID or notarization.
S4. User: "recordar no hacer nada que vaya a romper la version para windows". Preserve Windows behavior and existing Windows releases.

## Tasks
T1 [S3,S4] DONE: dependency-notice inventory and validation implemented; 14 public-CLI tests passed. Route: bounded worker; commit: pending.
T2 [S1-S4] DONE: final-bundle signing and clean-source prerelease mode implemented; 14 public-CLI tests passed, including signing-order and private-path regressions. Route: bounded worker; commit: pending.
T3 [S1-S4] IN PROGRESS: independently verify, freeze a committed source worktree, build/audit/extract Universal 2 ZIP and run Intel bundle smoke. Route: parent and independent verifier; commit: pending.
T4 [S3,S4] PENDING: publish source/tag/prerelease and verify uploaded assets/checksums; report ARM/signing/test limitations. Route: parent; commit: pending.

## Log
L1. Original packaging request: "la utilidad de escritorio para mac se puede empauetar para crear una release de MacOs".
L2. User acceptance: "ok". Local Intel app visually accepted: "va fenomenal".
L3. Apple Silicon request: "yo creo que lo podemos subir, pero tenemos que hacer algo para los macs con Apple Silicon". Human approved Universal 2 preparation and Rust target installation, then ad-hoc signing of the generated executable.
L4. Publication request: "publicalo". Human selected public v0.1.5-rc.1 prerelease and approved cache-license reads plus signing the complete newly generated app/resources. Keep Windows stable/latest release unchanged.
L5. Existing local Universal 2 build succeeded, but final verification failed: "code has no resources but signature indicates they must be present". Current script signed the executable before creating Info.plist/resources; cannot publish that artifact.
L6. Latest GitHub stable release is v0.1.4 - Windows x64. Cargo version intentionally remains 0.1.0; macOS bundle numeric version will be 0.1.5 and release tag v0.1.5-rc.1.
L7. Integration contract: scripts/audit_macos_licenses.py exposes validate_audit(audit_dir: Path, source_root: Path) -> dict raising RuntimeError on invalid/mismatched notices. Returns manifest_sha256, package_count, file_count. CLI creates a notice directory with manifest.json, third-party notices and license files under a requested --output; binds Cargo.toml/Cargo.lock SHA256 and source commit, both Darwin target closures, package versions/licenses and copied notice hashes; no private absolute source paths. The packager validates/copies this directory under Contents/Resources/ThirdPartyNotices before final bundle signing. T1 owns helper/tests, T2 owns packager/tests/README; surfaces are disjoint.
L8. Source freeze: parent commits intended files, adds a clean detached same-clone worktree for packaging, registers it, creates dependency audit there, and builds only from that worktree. Release mode requires clean source at entry/exit and verifies commit/manifest hashes. Local mode remains available without a claim of official provenance.
L9. Delivery: two functional work-unit commits plus release evidence slice; initial forecast ~700 authored lines excluding notices/generated metadata. Actual coherent source/tests total exceeds forecast (~1,250 lines): public command tests include complete fake-toolchain fixtures and validation cases. Direct prerelease publication explicitly requested; no PR requested. Keep each unit reviewable and assess committed ranges. No source or artifact overwrite, no existing tag replacement, no credentials, no Gatekeeper bypass. License and signature verification failures block publication.
L10. Parent observed 27 public CLI tests pass (16.339s). Independent static review found private absolute paths in raw codesign evidence. T2 correction observed RED (2 failing subtests) then GREEN (14 tests passed), replacing raw artifact signature diagnostics with portable class/verified fields. No real frozen audit/build yet. Independent runtime probes unavailable because verifier scratch preparation failed twice; no retries attempted.
