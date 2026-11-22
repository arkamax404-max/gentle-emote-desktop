# Build and verify a Linux prerelease

The Linux packager creates a **Linux x86_64 GNU tar.gz**, SHA-256 sidecar and
source-bound external `verification.json`. It neither installs, publishes nor
launches the app. Native extracted-binary smoke and publication are separate
release-owner gates. Windows/macOS scripts and application behavior are unchanged.

## Quick path

Use a clean, committed checkout, Python 3 (stdlib only), Git, Cargo on PATH, the
already installed `x86_64-unknown-linux-gnu` toolchain, a native compiler/linker,
`readelf` and `ldd`. All dependencies must already be available offline. No tool,
target or system-library installation is automated.

Choose new output names with existing parents **outside the checkout**. Run from
any directory, using absolute script paths if not in the checkout:

```sh
registry=/path/to/previously-authorized/registry/src
python3 -B scripts/audit_linux_licenses.py --registry-source-root "$registry" --output /tmp/emote-linux-notices
python3 -B scripts/audit_linux_licenses.py --registry-source-root "$registry" --validate /tmp/emote-linux-notices
# Explicitly choose the intended wrapper executable, or deliberately choose none:
export RUSTC_WRAPPER=''
python3 -B scripts/package_linux.py --registry-source-root "$registry" --release-tag v0.1.5-rc.3 \
  --license-audit /tmp/emote-linux-notices --output /tmp/emote-linux-package
```

The tag above is an example label, **not a reservation or tag creation**. Cargo's
source version (currently 0.1.0) remains unchanged and is recorded separately from
the packaging version and prerelease label. `--source-root DIR` selects another
checkout; the default is the scripts' checkout, never the caller's working directory.
Use `--help` for either script without invoking tools or creating output.

Do not select an empty wrapper unless overriding any configured wrapper is
intentional. A temporary compiler shim chains the explicitly selected executable
(including spaces), appending path remaps to Cargo's resolved arguments. `RUSTC`,
`RUSTC_WORKSPACE_WRAPPER`, `RUSTFLAGS`, and `CARGO_ENCODED_RUSTFLAGS` retain their
values and precedence, including empty values. Workspace, Cargo home, home and
temporary/build prefixes map to `/source`, `/cargo`, `/toolchain`, and `/build`.
The wrapper selection is mandatory because config-only selection cannot be
transparently preserved. Neither Cargo configuration nor credentials are read by
the Python scripts; normal Cargo execution still follows Cargo's own configuration.

## What is checked

| Gate | Evidence and limits |
| --- | --- |
| Source | Tracked bytes and executable modes must match committed HEAD; tracked-file SHA256/modes and manifest/lock hashes checked repeatedly through export; untracked files reject packaging. No source mutation is performed. |
| Cargo | Metadata uses `--offline --locked --filter-platform x86_64-unknown-linux-gnu`; build uses `--release --offline --locked --target x86_64-unknown-linux-gnu` in fresh system-temp storage. |
| ELF | ELF64 little-endian x86_64, approved GNU interpreter, actual `NEEDED` and version-needs inspection with `readelf`; no RPATH/RUNPATH; `ldd` must resolve dependencies to system library locations. No raw host diagnostic paths enter receipts. |
| glibc | Maximum observed numeric GLIBC version need, plus full version-needs names, is recorded. This is a requirement of this binary, **not a broad distro, GPU or display-server compatibility guarantee**. |
| Privacy | Binary and all shipped texts are scanned for the actual checkout, build, wrapper, home/Cargo paths, the explicit registry source root and common private home prefixes; detection rejects export, without changing original license bytes. This is not a claim of universal secret detection. |
| Archive | Only regular files, one named top-level directory, no absolute/traversal/link members, executable mode 0755, text mode 0644, zero owner IDs and exact member sizes/SHA256; archive readback must match the complete external file inventory. |
| Output | New output only; collisions and all validation/build failures before export leave no final artifacts. An export I/O error or source mutation during the final copy may leave an incomplete directory; never reuse it. |

Successful output contains only the archive, its `.tar.gz.sha256` sidecar and
`verification.json`. The archive includes the executable, project `LICENSE`,
`ThirdPartyNotices/`, `README.txt` with execution requirements, and an embedded
`packaging-receipt.json`. The external JSON binds the archive hash and every
payload's path/hash/mode/size to the source commit, target, versions, Cargo hashes
and notice inventory. No signatures or reproducible-build guarantee are claimed.
The archive is verified by reading every member, without executing or extracting
untrusted archive paths. `archive_verified: true` is **not** native smoke proof;
`runtime_verified` stays false.

## Original notices, not substitute licenses

The Linux auditor reads dependency sources only within the checkout or
the exact `--registry-source-root DIR` passed to both scripts. Omit it for
checkout-only reads. `CARGO_HOME` selects Cargo's cache, not the Python read
boundary. Reading the selected registry source root requires the operator's
prior authorization; selecting a path does not grant access. It rejects symlinks
and paths outside those roots. It retains actual conventional license/notice files,
nested font attributions and copyright texts with hashes and package/version
identities. Validation rederives metadata and bytes rather than trusting a supplied
manifest, so omissions, extra payloads and tampering reject packaging. Clean-source
binding applies to audit generation as well as packaging.

SPDX parsing/selection and byte-inventory helpers are reused from the macOS
module, **not** its Darwin closure, supplements or accepted exceptions. The first
supported SPDX OR branch must have corresponding original license text;
conservative text witnesses reject link-only declarations and unrelated font
notices as substitutes for that grant. Only `fonts/OFL.txt` and `fonts/UFL.txt`
can supply nested OFL-1.1 and Ubuntu-font-1.0 witnesses respectively; a nested font
MIT notice cannot satisfy the crate's MIT grant. This intentionally can reject
legitimate unrecognized license layouts; it must not be bypassed with a generic
SPDX text. All available original notice/font payloads are retained, not only
that witness.
`notice_inventory_complete` is an inventory statement; `legal_audit_complete`
remains **false** and is never inferred from it.

Checked-in [Linux supplements](../scripts/linux-license-supplements/README.md)
retain authentic upstream original bytes when published packages omit their root
licenses. Each supplement binds package/version, declared SPDX, repository,
published VCS revision/subdirectory, notice revision/path/URL, provenance kind
and SHA256. Exact-crate provenance requires equal published/notice revisions.
Eight egui-family 0.37.0 crates instead use **same-version upstream-release**
notices at official release commit `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`;
this is explicitly **not** their unavailable published revision. Eframe 0.37.0
uses its own exact published revision. AccessKit AUTHORS and Chromium's original
BSD notice are retained; original published source-attribution prefixes are
included and checked against the crate bytes for AccessKit and gl-rs crates.
Supplement bytes and provenance are copied into the generated notice inventory
and rederived on validation. The runtime auditor does not fetch anything.

Missing, tampered or mismatched supplements fail closed; unresolved original-text
gaps name the exact package/version and selected license and create no output.
There are **no Linux license exceptions**. Resolve further gaps only from
authorized, authentic evidence under a separately reviewed change; never copy
macOS exceptions as Linux approval, invent attribution, fetch without authorization,
or replace a missing original with generic SPDX boilerplate. A real Linux closure
audit must pass before a release package can be produced. Supplements and a
complete inventory still do not establish legal clearance.

## Verify and run the extracted executable

From the exported directory:

```sh
sha256sum -c gentle-emote-desktop-*.tar.gz.sha256
# Inspect the single top-level directory and members before extraction:
tar -tzf gentle-emote-desktop-*.tar.gz
# Extract into a NEW scratch directory chosen by the release owner:
tar -xzf gentle-emote-desktop-*.tar.gz -C /path/to/new/scratch
```

Independently compare the extracted executable and all texts against the external
file inventory, including modes. Run `./gentle-emote-desktop` from the extracted
top-level directory in a graphical session with working OpenGL drivers and the
recorded native libraries/glibc requirement. Runtime assets are embedded.
**The app has no CLI help/health mode: even flags start the GUI.** Close other
listeners on 127.0.0.1:8001 first; use an isolated absolute `XDG_CONFIG_HOME` for
smoke so the user's saved background color is untouched.

Native smoke must identify the exact extracted child and confirm its GUI/port
ownership: GET `/emote` returns 405 with `method not allowed\n`; GET `/health`
returns 404 with `not found\n`; a valid bodyless version-1 session/stream heartbeat
POST to `/emote/heartbeat` returns 204 with an empty body. Check the
[protocol](protocol.md) for exact headers. Do not infer app health from a different
listener or a fabricated help command. Record native evidence separately, shut
down only the identified child and verify port release. X11, other distros and
Windows/macOS native validation are not implied by a Linux Wayland smoke.

Only the release owner may authorize authentication, commits, a fresh tag and
prerelease publication. Recheck tag/asset absence and preserve stable/latest and
all existing releases. This workflow performs none of those operations.

## Deterministic CLI checks

```sh
python3 -m unittest discover -s tests -p 'test_*linux*.py' -v
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

These use disposable fake-tool fixtures from unrelated working directories. They
prove CLI guards and archive/notice contracts, **not** that a real compiled ELF is
portable, private-path-free, legally cleared or capable of native GUI startup.
