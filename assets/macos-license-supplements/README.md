# Supplemental macOS dependency notices

These portable assets fill notice omissions in the resolved Darwin dependency
closures. They are **not a complete legal-license audit**, nor proof that every
selected SPDX license grant is established. The helper performs no downloads.
Shipped crate notices normally take precedence. The ten historical MIT exceptions
below also require supplements when a crate ships a link-only declaration;
all discoverable shipped notices are retained.

## Manifest contract

`manifest.json` contains `schema_version: 1` and a `packages` array. Each package
has these fields:

- `name`, `version`, `license_expression`: exact Cargo metadata declarations.
- `repository`: normalized HTTPS repository URL (no trailing slash or `.git`;
  GitHub `/tree/<ref>/<crate-path>` declarations normalize to owner/repository).
- `crate_revision`, `path_in_vcs`: exact `.cargo_vcs_info.json` values from the
  published cached crate (`git.sha1` and `path_in_vcs`). Legacy VCS metadata
  omitting `path_in_vcs` denotes the repository root (`""`); explicit values
  still require exact equality (including rejection of `"."` or `null`).
- `notice_revision`: pinned 40-character upstream Git commit serving the notice.
- `provenance_kind`: `exact-crate-revision`, `same-version-upstream-release`, or
  the explicitly accepted `declared-license-exception` or `historical-mit-exception`
  described below.
- `files`: records containing `path: files/<sha256>`, `sha256`, and
  `upstream_url` (a pinned HTTPS GitHub blob URL).

`exact-crate-revision` requires equal crate and notice revisions. A
`same-version-upstream-release` record instead requires `release_tag` equal to
its version or `v` followed by its version, and separately records the resolved
release commit in `notice_revision`. This is a disclosed release fallback, **not
an exact published-crate source claim**. A human must verify that release mapping;
the offline helper cannot establish a tag's remote history.

Payloads in `files/` are verbatim bytes, addressed by SHA256, with no newline,
encoding, copyright, or other normalization. Retrieve notice bytes from the
pinned upstream source URLs after inspecting the exact cached crate's metadata,
then verify their hashes. Do not substitute another cached version, a moving
branch, or generic SPDX text for a missing original notice. Repository links and
revisions in each manifest entry provide the specific retrieval source refs.
The auditor verifies crate/repository correspondence, hashes, paths, and absence
of symlinks, and copies the complete provenance entry into the output manifest.

## Explicit dispatch 0.2.0 exception

The accepted prerelease exception concerns
[SSheldon/rust-dispatch at 82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748](https://github.com/SSheldon/rust-dispatch/tree/82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748).
Its [original Cargo.toml](https://github.com/SSheldon/rust-dispatch/blob/82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748/Cargo.toml)
declares MIT but the source provides no original copyright/license notice.
Neither an author declaration nor an SPDX identifier proves copyright ownership.
Do not invent a copyright holder, year, or original notice title.

The `declared-license-exception` entry must have a nonempty `license_exception`
disclosure and exactly two evidence files labeled by `role`:

- `license-declaration`: verbatim original upstream Cargo.toml at the crate
  revision, not an original license notice.
- `spdx-reference`: verbatim standard MIT reference from
  `https://github.com/spdx/license-list-data/blob/<commit>/text/MIT.txt`
  (the equivalent pinned raw URL is also accepted), pinned to a full revision.
  This is **reference text, not dispatch's original
  notice**; any template placeholders remain untouched.

The auditor preserves `license_exception` unchanged in the package record and
reports it in the summary's `license_exceptions` list with package name/version.
`selected_licenses` is an SPDX declaration selection, not proof of a grant;
exception packages explicitly have `selected_license_notice_verified: false`.
The summary retains `manifest_sha256`, `package_count`, and `file_count`, adds
`notice_inventory_complete: true` only after successful validation, and always
reports `legal_audit_complete: false`. Release disclosures must retain the
exception; a complete notice inventory must not be presented as a full legal
audit.

## Historical MIT exceptions: uncertain current applicability

The human accepted publication with authentic historical MIT text and current
upstream declarations, **without establishing applicability to the current crates**.
This covers block2 0.6.2, objc2 0.6.5, objc2-encode 4.1.0, dispatch2 0.3.1,
and objc2-{app-kit,core-foundation,core-graphics,io-surface,metal,quartz-core} 0.3.2.
It is distinct from dispatch's missing-original-notice exception above.

Each `historical-mit-exception` entry preserves the current `crate_revision`,
`notice_revision`, SPDX declaration and exact upstream `LICENSE.md` as a
`license-declaration` file. That declaration contains links, not original MIT
terms. A separate `historical-mit-notice` file has its own `notice_revision` and
[pinned original URL](https://raw.githubusercontent.com/madsmtm/objc2/757cd841d0293341dbefd9b99d7548dec244ffde/LICENSE.txt).
The fetched SHA256 is `e353f37b12aefbb9f9b29490e837cfee05d9bda70804b3562839a3285c1df1e5`,
verified byte-for-byte against the existing payload before reuse. Its original
`Copyright (c) Steven Sheldon` is unchanged, with no invented year.

The narrow exception requires `selected_licenses: ["MIT"]`: MIT is an allowed
alternative in these exact declarations. This does not assert that MIT covers
Zlib, change ordinary OR selection, or prove a current grant. The auditor pins
both evidence hashes, roles and source URLs, requires nonempty `license_exception`,
and requires `historical_notice_applicability_verified: false`. Historical
provenance cannot masquerade as exact-current evidence. Both the package's
`selected_license_notice_verified` and summary's `legal_audit_complete` stay false.
The bundle README and embedded/external receipts retain every exception.

## Checked-in retrieval provenance

The inventory contains 29 package entries and 51 package-file references backed
by 13 deduplicated payloads. Twelve entries use their exact published crate
VCS revision, six use the explicit same-version release fallback below, ten
are historical MIT exceptions retaining exact-current declarations, and one
is the dispatch exception. Each file's `upstream_url` preserves its exact source
ref; shared bytes do not collapse distinct per-package provenance.

The parent recovered the payloads from the pinned upstream sources before
import. Import verified every SHA256 and copied the bytes unchanged. No network
access or notice synthesis occurs in the helper or the tests.

For ecolor, egui-wgpu, egui-winit, egui_glow, emath, and epaint 0.37.0, the
published crate revision `7c0b40402271e8d40cda69f87cf3c2617c2e4382` was
unavailable through GitHub (HTTP 422). Parent verification resolved release tag
[`0.37.0`](https://github.com/emilk/egui/releases/tag/0.37.0), through its annotated
tag, to commit
[`841a328ba2dd8f6996beb7189ebb7fc00e668a2a`](https://github.com/emilk/egui/tree/841a328ba2dd8f6996beb7189ebb7fc00e668a2a).
Their two root license files are pinned to that release commit, **not** asserted
to have been recovered from the unavailable crate commit. The manifest retains
that distinction and its explanatory note. eframe 0.37.0 instead uses recovered
notices from its own exact crate commit
`3c6eb8261c3293ad9c7ea1ed58487e038860ebef`.

The dispatch declaration is the upstream Cargo.toml linked above. Its
`authors = ["Steven Sheldon"]` is author metadata, **not a copyright declaration**.
The separately labeled standard MIT reference is pinned to SPDX license-list-data
commit [`ab9b0904b43db6f4b67431efd8821feb821f1c8b`](https://github.com/spdx/license-list-data/blob/ab9b0904b43db6f4b67431efd8821feb821f1c8b/text/MIT.txt).
Its `<year>` and `<copyright holders>` template placeholders remain verbatim;
they have not been filled in or attributed to dispatch.

## Verification

Run `python3 -B -m unittest discover -s tests -p 'test_audit_macos_licenses.py'`.
Generate with `python3 scripts/audit_macos_licenses.py --output <new-directory>`;
validate with `--validate <directory>`. `--source-root` selects the frozen source
checkout whose Cargo files, commit, supplements, and both Darwin closures are
bound to the audit. Dependency reads are restricted to that checkout and the
exact `index.crates.io-1949cf8c6b5b557f` registry index under
`$CARGO_HOME/registry/src` (or `~/.cargo/registry/src` by default).
