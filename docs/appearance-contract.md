# Shared appearance v1 — one source, confirmed updates

Native desktop reads and patches only
`<agentDir>/extensions/gentle-emote/appearance.json`. Gentle Emote owns this
shared format; desktop never imports, deletes, or silently falls back to the old
independent RGB/opacity files. A sender process is not required. PNG transport,
RGBA pixels, and source-owned `image` settings are unchanged.

## Select the source

```sh
gentle-emote-desktop --agent-dir /absolute/path/to/agent
gentle-emote-desktop --help
```

Resolution: `--agent-dir ABSOLUTE` > `PI_CODING_AGENT_DIR` > `$HOME/.pi/agent`.
Explicit empty or relative values, and explicit `.` or `..` path components,
fail with exit 2 and stderr; they never select a fallback or cwd. Components are
checked in the raw OS representation, not normalized away (`/agent/./nested` is
invalid). Windows treats both slash styles as separators; Unix treats backslash
as an ordinary filename byte. `Store::new` enforces the same rule. The option may appear once. Help exits 0 without native GUI,
receiver, or preference I/O. Native/backend startup errors still exit nonzero.
On Windows the GUI subsystem can hide process stdout/stderr from double-click
launches; the in-app persistence error remains visible.

## Read and patch

Example (all three desktop fields explicitly supplied):

```json
{
  "version": 1,
  "revision": 1,
  "desktop": {
    "backgroundRgb": [32, 128, 224],
    "backgroundOpacity": 55,
    "transparentBackground": true
  }
}
```

| Rule | Behavior |
| --- | --- |
| Envelope | UTF-8 JSON object, at most 65,536 bytes; version exactly 1, required unsigned JS-safe integer revision, required desktop object with all three known fields |
| Required desktop fields | `backgroundRgb`: three byte integers; `backgroundOpacity`: integer 0–100; `transparentBackground`: boolean. Missing known fields invalidate an existing file. False requires opacity 100; true with 100 is valid |
| Whole-document validation | Every numeric lexeme, including unknown nested fields, is an integer in ±9,007,199,254,740,991. Fractions, exponents, duplicate keys (including escaped equivalents) anywhere, invalid UTF-8/escapes, a UTF-8 BOM, unpaired surrogate escapes and trailing content are rejected |
| Container depth | `MAX_CONTAINER_DEPTH=64`: root object depth 1, every nested object/array adds 1, including desktop, RGB arrays and empty containers. Reject depth 65 before entering it (`invalid`, `JSON container depth exceeds 64`) |
| Unknown fields | Preserve semantically at root, desktop and any nested location. `image` is reserved: never applied or patched. Object order/whitespace and `-0` spelling need not survive |
| Missing source | In-memory black/55/checked, revision 0. No startup directory/file creation; first explicit save becomes revision 1 |
| Invalid/unreadable/future source | Leave bytes untouched, show a nonfatal error, keep last confirmed display (initial defaults if none). Never repair or use another store |
| Patches | RGB changes only `backgroundRgb`. Opacity and checkbox are one grouped change. Other known and unknown leaves remain unchanged |
| CAS | Under lock reread and validate, compare expected revision; mismatch returns current snapshot with no write, counter increment or automatic replay. Increment only accepted patches; reject revision overflow |

Contract revision L12 explicitly rejects escaped unpaired UTF-16 surrogates in
all strings and keys, including unknown nested fields. Valid surrogate pairs are
accepted and preserved semantically. Escaped and literal equivalent keys still
count as duplicates. The source must not start with UTF-8 BOM bytes `EF BB BF`.

Peer-ready literal-byte fixtures live in `tests/fixtures/appearance/`:

| File | Expected |
| --- | --- |
| `invalid-bom.json` | Reject: starts with actual BOM bytes, not a textual escape |
| `invalid-surrogate-value.json` | Reject: nested unknown value contains `\\ud800` |
| `invalid-surrogate-key.json` | Reject: nested unknown key contains `\\udfff` |
| `valid-surrogate-pairs.json` | Accept: paired escaped keys/values and literal Unicode |
| `missing-rgb.json`, `missing-opacity.json`, `missing-transparent.json` | Reject: each omits exactly one required desktop field; revision 7 and all source bytes survive rejected patches |

All Unicode/numeric fixtures otherwise supply the complete known desktop schema,
so schema rejection cannot masquerade as their intended lexical check. L17 defaults
apply only to a missing file's complete in-memory snapshot, never omitted fields.

Each fixture ends with one LF. Send the file bytes unchanged to the peer parser;
no sender execution or cross-language conformance is claimed here.

The strict parser, encoder, clone/comparison and tree destruction remain iterative.
Contract revision L26 additionally bounds all known/unknown object and array nesting
to 64 containers. The parser checks open ancestors before entering any container,
including an empty one. Serialization receives only validated trees; RGB patches
replace the array at depth 3 and grouped opacity/checkbox patches replace scalars,
so neither can deepen a valid document. No JSON or serialization dependency was added. Existing locked `getrandom 0.3` supplies
16 OS-random bytes; existing locked `libc` supplies Unix effective-UID and
no-follow/nonblocking flags. Neither dependency was upgraded.

## File transaction (Unix implementation)

1. Walk every root-to-parent prefix with `symlink_metadata`, rejecting symlinks
   and non-directories. System ancestors need no UID or private-mode requirement.
   AgentDir and every existing descendant must have the effective UID and no
   group/other write bits (`mode & 022 == 0`). Reads create nothing. Only explicit
   saves create missing directories with mode 0700; never chmod existing entries.
2. Atomically mkdir sibling **directory** `appearance.json.lock` with mode 0700.
   Create its `owner` marker with `create_new`, mode 0600. Exact contents:
   `v1:<32 lowercase hex characters from 16 OS-random bytes>\n` (36 bytes).
3. Retry contention every 50 ms, stopping after 1,000 ms. An incomplete or
   malformed marker is not permission to recover: the lock stays held. Never
   steal by age/PID, replace a lock, or remove someone else's marker.
4. While owning the lock, reread, validate and perform revision CAS. Write only
   the patch's desktop leaves and the incremented revision to sibling
   `appearance.json.<token>.tmp`, exclusive-create mode 0600. Check the encoded
   byte limit; write, flush, sync the file and close it before commit.
5. Before rename, repeat ancestor guards, lock-directory/marker identity and
   exact token checks, temp identity, and source type/identity (including whether
   it was absent at the locked read). **Atomic rename is the sole commit point.**
   There is no delete-destination fallback. Sync the parent directory afterward.
6. Before commit, an error leaves source bytes/revision unchanged. After rename,
   only actual unsupported directory-fsync errors are tolerated: Rust
   `Unsupported`, or POSIX `EINVAL`, `ENOTSUP`/`EOPNOTSUPP` from syncing the valid
   opened directory. Directory-open errors and every other sync error return
   **committed-durability-uncertain**, retaining the confirmed snapshot. Display
   it, report the error, and do not retry.
7. Attempt temp and lock cleanup independently, checking ancestor/object
   identities and the complete marker token before unlink. Partial owned markers
   can be removed on acquisition errors. Ordinary cleanup failures appear as
   separate `cleanupError` metadata without changing the primary status, error,
   or snapshot; Drop is only an unwind backstop. An unavailable initial identity
   is fail-closed: leave the unproven object, report manual recovery in
   `cleanupError`, and still attempt other proven cleanup. Crashed/orphaned and
   unknown-identity objects require manual recovery, never speculative deletion.

Existing source/marker/temp leaves must be same-user regular files with no
group/other write bits; symlinks and other nonregular leaves are rejected. Lock
paths have the same ownership/write guard and must be directories. Read bits
such as existing 0644 files or 0755 directories are allowed; modes are never
"fixed". Reads are bounded and use no-follow/nonblocking opens, checking the
opened identity against lstat; FIFO/device input cannot stall a read. New objects
use 0700/0600, subject to a more restrictive process umask.

**Trust boundary:** even with prefix checks, ancestors must remain trusted against
malicious replacement by other actors. Checks are not an openat-based sandbox;
they do not prevent a malicious same-user actor from swapping ancestor paths or
racing checks and rename. Do not place the store in a hostile shared directory.
No universal TOCTOU immunity, privileged-user protection or network-filesystem
atomicity guarantee is claimed.

**Windows:** persistence returns an explicit unsupported error pending owner
clarification of ACL/atomic-replace behavior. Unix mode checks are not presented
as Windows protection. Native UI keeps receiving/rendering with initial defaults
and disables shared appearance edits, with a visible nonfatal error; it never
uses an independent store. Windows compilation/runtime and ACL interoperability
are unverified. Other non-Unix targets also reject persistence.

### A leftover lock

A crashed writer may leave the lock and/or a unique temporary file. There is no
automatic recovery. Stop cooperating writers and have the owner inspect the
source and lock identity before any manual recovery. Do not blindly retry an
uncertain commit: reload first and inspect the reported revision. Desktop does
not delete backups, legacy files or unrelated temporary files.

## UI and embedding

The worker owns startup/panel reads, locked writes, conflict reloads and 500 ms
polling (full reads, not mtime-only detection). Render only drains notifications
and submits nonblocking messages. Bounded channels apply backpressure rather
than growing an unbounded queue. Appearance controls are disabled before the
first valid read and while confirming an explicit action. Their local proposals
never alter displayed settings before confirmation. A conflict refreshes the
snapshot without retrying the proposal, and its notice survives panel reload.
Only warning-free explicit saves clear prior errors; external reloads retain notices
so they cannot disappear before the user reads them. Cleanup or durability warnings
accompany the confirmed snapshot, including an already-saved update, without a retry.

`Store::load_report` and `Store::patch_report` expose a typed `Completion` with
`status`, optional `snapshot`, original `error`, and independent `cleanup_error`.
`Status::code()` is stable: `ready`, `missing`, `saved`, `conflict`,
`revision-exhausted`, `busy`, `invalid`, `unsafe-path`, `io-error`,
`unsupported-platform`, and `committed-durability-uncertain`. Unsupported non-Unix
persistence returns `unsupported-platform` with `error.kind() == Unsupported`
before any filesystem access. Compatibility `load`/`patch` remain available;
`SaveResult::WithCleanupError` retains an original successful/conflict outcome,
and rejected writes retain their I/O error kind plus cleanup context.

`EmoteApp::new(receiver)` does no preference I/O. Opt in with
`with_shared_appearance(Store::new(absolute_agent_dir)?, context)`.
Legacy `background_color`, `background_opacity` and explicit legacy builders
remain available to embeddings. Shared mode clears/ignores legacy path opt-ins;
native startup uses shared mode exclusively. No user data migration occurs.

## Verification boundaries

See [testing](testing.md#shared-appearance-source) for reproducible focused
commands, baseline failures and unverified fault/platform cases. Independent
verification is required for permissions, interoperable locking/CAS, concurrent
writers and dependencies. Desktop-local JSON fixtures are not evidence that the
sender independently ran the same fixture corpus. No native restart or release
is part of this source-only implementation.
