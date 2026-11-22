# Original Linux notice supplements

These are authentic original files omitted from published crate root layouts,
not generic SPDX replacements and not approval to reuse macOS exceptions.
The Linux auditor performs **no network access**. `manifest.json` binds the
checked-in bytes to 14 package/version identities using 29 file records and
10 distinct SHA256-addressed payloads. Original bytes and copyright attributions
are retained without reformatting. Legal-audit completeness remains false.

## Provenance

| Packages | Notice repository and pinned revision | Kind |
| --- | --- | --- |
| accesskit 0.25.2 | AccessKit/accesskit `f3026222559c7f5f46d8f25cb7404de68eb149f0` | Exact published crate revision |
| eframe 0.37.0 | emilk/egui `3c6eb8261c3293ad9c7ea1ed58487e038860ebef` | Exact published crate revision |
| ecolor, egui, egui-wgpu, egui-winit, egui_glow, emath, epaint, epaint_default_fonts 0.37.0 | emilk/egui `841a328ba2dd8f6996beb7189ebb7fc00e668a2a` | Same-version upstream release, **not exact published revision** |
| profiling 1.0.18 | aclysma/profiling `8271551172eb6fa4cba47369aedd93790c623df9` | Exact published crate revision |
| spirv 0.4.0+sdk-1.4.341.0 | gfx-rs/rspirv `8afc3d0ac8e158128cd1410bb2e4b4c26ab11bb4` | Exact published crate revision |
| gl_generator 0.14.0 | brendanzab/gl-rs `ea503e8d5fb6d73c6030e6191ce738cd3bf3433e` | Exact published crate revision |
| khronos_api 3.1.0 | brendanzab/gl-rs `f150967b1c44ae888e6676f93f639ebc82771bdc` | Exact published crate revision |

Every remote payload's full retrieval URL is in its manifest record:
`https://raw.githubusercontent.com/<repository>/<notice_revision>/<notice_path>`.
License files were retrieved from those pinned HTTPS URLs, never a moving branch.
AccessKit includes `LICENSE-MIT`, `LICENSE-APACHE`, `AUTHORS` and
`LICENSE.chromium`. Egui/eframe retain both original MIT and Apache files;
profiling retains its original MIT file; spirv/gl-rs retain their original
Apache `LICENSE`. Identical originals share content-addressed storage but retain
separate per-package provenance records.

The eight fallback crates declare published revision
`7c0b40402271e8d40cda69f87cf3c2617c2e4382`, whose root notices were unavailable
(404), as recorded by the release owner's exploration. Their notice revision is
instead the official annotated `0.37.0` release:

- [Pinned tag object](https://api.github.com/repos/emilk/egui/git/tags/418687fa18fdac8a9b1b6c8f4a2eb1afd2df4c47)
  resolves to `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`.
- [Cargo.toml at that commit](https://raw.githubusercontent.com/emilk/egui/841a328ba2dd8f6996beb7189ebb7fc00e668a2a/Cargo.toml)
  declares workspace version `0.37.0` (SHA256
  `b2da1bc9f221cdacfe0ed9467e29cb8f15f3b6e674dfbc193bc273c1815be285`).

The tag resolution and workspace version were independently checked during
supplement preparation. Same-version provenance is disclosed in each affected
entry; it is never represented as retrieval from the published crate revision.

## Published source attributions

`source-attribution` records preserve the exact leading copyright/license comment
block from the published AccessKit and gl-rs crate sources. Their `crate-source:`
URIs identify the package/version/local relative source path, **not an upstream
HTTP retrieval**. The manifest binds their SHA256, byte length and source path;
validation compares the payload against that exact prefix of the authorized
published crate source. The unchanged gl-rs prefix identifies Brendan Zabarauskas
and the gl-rs developers. AccessKit's prefix identifies the AccessKit Authors;
its separate AUTHORS and Chromium BSD notice remain intact.

## Validation boundary

The auditor checks repository normalization, package/version, declared SPDX,
published `.cargo_vcs_info.json` revision and subdirectory, provenance kind,
notice revision, release-version label where applicable, safe relative paths,
exact pinned retrieval URLs, roles and every payload SHA256. Symlinks, absent or
altered payloads, mismatched declarations and source-prefix mismatches reject
export. The generated audit retains each full provenance entry, and its validator
rederives exact inventory/bytes from the source checkout and resolved dependencies.
This is offline provenance consistency, not an online tag-signature verification
or independent legal opinion.

The first supported SPDX branch is unchanged. Only explicit `license` records
can witness a crate's grant; attribution records cannot replace a license.
Original bundled `fonts/OFL.txt` and `fonts/UFL.txt` remain in the crate inventory
and witness only their corresponding font grants. Nested MIT font attribution
never stands in for a crate MIT license.
