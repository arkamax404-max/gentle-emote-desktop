#!/usr/bin/env python3
"""Build portable dependency notices, not legal advice or a compliance guarantee.

Only the two resolved Darwin closures are inventoried. The packager separately
owns the application's root license. No builds, downloads or Cargo mutations.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit

TARGETS = ["x86_64-apple-darwin", "aarch64-apple-darwin"]
REGISTRY_INDEX = "index.crates.io-1949cf8c6b5b557f"
SUPPLEMENTS = "assets/macos-license-supplements"
SUPPORTED = {
    "MIT", "Apache-2.0", "Unicode-3.0", "Unicode-DFS-2016", "OFL-1.1",
    "Ubuntu-font-1.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
    "Unlicense", "0BSD", "CC0-1.0", "BSL-1.0",
    "Apache-2.0 WITH LLVM-exception",
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def child_diagnostics(stdout, stderr):
    parts = []
    for label, data in (("stdout", stdout), ("stderr", stderr)):
        if isinstance(data, bytes):
            data = data.decode(errors="replace")
        parts.append(label + ":\n" + (data or "").rstrip("\n"))
    return "\n" + "\n".join(parts)


def command(args, root, error):
    context = "\ncommand: " + shlex.join(args)
    try:
        result = subprocess.run(args, cwd=root, capture_output=True, text=True, timeout=60)
    except OSError as exc:
        raise RuntimeError(error + ": could not execute command" + context + "\n" + str(exc)) from None
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError(error + " (timeout after 60s)" + context +
                           child_diagnostics(exc.stdout, exc.stderr)) from None
    if result.returncode:
        raise RuntimeError(error + " (exit " + str(result.returncode) + ")" + context +
                           child_diagnostics(result.stdout, result.stderr))
    return result.stdout


def cargo_executable():
    # Match the packager: honor PATH first, then the standard rustup install.
    cargo = shutil.which("cargo")
    if not cargo:
        fallback = Path.home() / ".cargo/bin/cargo"
        if fallback.is_file() and os.access(fallback, os.X_OK):
            cargo = str(fallback)
    if not cargo:
        raise RuntimeError("cargo not found on PATH or at ~/.cargo/bin/cargo")
    return cargo


def select_license(expression):
    """Small SPDX expression parser: AND binds tighter; choose a supported OR.

    Unknown identifiers may occur on an unselected OR branch, never in a
    selected AND branch. Keep the original expression separately in inventory.
    """
    if not isinstance(expression, str) or not expression.strip():
        raise ValueError("missing SPDX")
    tokens = re.findall(r"[A-Za-z0-9.+-]+|[()/]", expression)
    if re.sub(r"\s+", "", expression) != "".join(tokens):
        raise ValueError("invalid SPDX")
    tokens = ["OR" if t == "/" else t for t in tokens]
    pos = 0

    def atom():
        nonlocal pos
        token = tokens[pos]
        pos += 1
        if token == "(":
            value = either()
            if tokens[pos] != ")":
                raise ValueError("unclosed SPDX")
            pos += 1
            return value
        if token in ("AND", "OR", "WITH", ")"):
            raise ValueError("invalid SPDX atom")
        if pos < len(tokens) and tokens[pos] == "WITH":
            pos += 1
            token += " WITH " + tokens[pos]
            pos += 1
        return [token] if token in SUPPORTED else None

    def both():
        nonlocal pos
        value = atom()
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            right = atom()
            value = value + right if value is not None and right is not None else None
        return value

    def either():
        nonlocal pos
        value = both()
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            right = both()
            if value is None:
                value = right
        return value

    selected = either()
    if pos != len(tokens) or selected is None:
        raise ValueError("unsupported SPDX")
    return sorted(set(selected))


def beneath(path, root):
    return path == root or root in path.parents


def safe_source(path, root):
    # Check lexical and resolved containment before reading any dependency file.
    path = Path(os.path.abspath(path))
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")).absolute()
    allowed = [root, cargo_home / "registry/src" / REGISTRY_INDEX]
    if not any(beneath(path, r) for r in allowed):
        raise RuntimeError("dependency source outside authorized roots")
    if any(p.is_symlink() for p in [path, *path.parents]):
        raise RuntimeError("symlink in dependency source")
    if not any(beneath(path.resolve(), r.resolve()) for r in allowed):
        raise RuntimeError("dependency source outside authorized roots")
    return path


def walk_error(error):
    # os.walk silently ignores inaccessible directories unless onerror is set.
    raise RuntimeError("cannot inventory directory") from error


def notice_files(crate, root):
    files = {}
    for directory, dirs, names in os.walk(crate, followlinks=False, onerror=walk_error):
        for name in sorted(dirs + names):
            path = safe_source(Path(directory) / name, root)
            if not path.is_file():
                continue
            relative = path.relative_to(crate)
            # Include nested font attribution texts (egui-icons, Hack, etc.),
            # not just conventional top-level LICENSE names.
            conventional = any(re.match(
                r"^(licen[cs]es?|copying|copyright|notices?|unlicense|ofl|ufl)([._-]|$)",
                component, re.I) for component in relative.parts)
            font_text = "fonts" in relative.parts and path.suffix.lower() in (".txt", ".md")
            copyright_text = False
            if not conventional and not font_text and path.suffix.lower() in (".txt", ".md"):
                copyright_text = b"copyright" in path.read_bytes().lower()
            if conventional or font_text or copyright_text:
                data = path.read_bytes()
                if not data.strip():
                    raise RuntimeError("empty notice file")
                files[relative.as_posix()] = data
    return files


def normalized_repository(value):
    if not isinstance(value, str):
        raise ValueError("missing repository")
    parsed = urlsplit(value.removeprefix("git+"))
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password:
        raise ValueError("invalid repository")
    if parsed.query or parsed.fragment or parsed.port:
        raise ValueError("invalid repository")
    path = parsed.path.rstrip("/").removesuffix(".git")
    if not path or any(p in (".", "..") for p in path.split("/")):
        raise ValueError("invalid repository")
    if parsed.hostname.lower() == "github.com":
        parts = path.strip("/").split("/")
        if len(parts) < 2 or not all(parts):
            raise ValueError("invalid repository")
        if len(parts) > 2 and (len(parts) < 5 or parts[2] not in ("tree", "blob")):
            raise ValueError("invalid repository")
        # Cargo repository declarations sometimes link to a crate subdirectory.
        # Those links still identify the same owner/repository, not a new repo.
        path = "/" + "/".join(parts[:2])
    return "https://" + parsed.hostname.lower() + path


def supplement_path(base, relative):
    # Never follow links, even when their destination is contained.
    if not isinstance(relative, str) or relative.startswith("/") or "\\" in relative:
        raise RuntimeError("invalid supplement manifest")
    parts = relative.split("/")
    if any(p in ("", ".", "..") for p in parts):
        raise RuntimeError("invalid supplement manifest")
    path = base / relative
    if any(p.is_symlink() for p in [path, *path.parents]):
        raise RuntimeError("symlink in supplement")
    return path


def supplemental_notices(package, crate, root, key):
    base = root / SUPPLEMENTS
    manifest_path = supplement_path(base, "manifest.json")
    if not manifest_path.exists():
        raise RuntimeError("no notices found: " + key)
    try:
        manifest = json.loads(manifest_path.read_bytes())
        if manifest["schema_version"] != 1 or not isinstance(manifest["packages"], list):
            raise ValueError()
        entries = {}
        for entry in manifest["packages"]:
            identity = (entry["name"], entry["version"])
            if identity in entries:
                raise ValueError()
            entries[identity] = entry
    except (OSError, ValueError, KeyError, TypeError):
        raise RuntimeError("invalid supplement manifest") from None
    entry = entries.get((package["name"], package["version"]))
    if entry is None:
        raise RuntimeError("no notices found: " + key)
    try:
        vcs = json.loads(safe_source(crate / ".cargo_vcs_info.json", root).read_bytes())
        revision = entry["crate_revision"]
        notice_revision = entry["notice_revision"]
        kind = entry["provenance_kind"]
        repository = normalized_repository(package["repository"])
        if (entry["repository"] != repository or
                not re.fullmatch(r"[0-9a-f]{40}", revision) or
                not re.fullmatch(r"[0-9a-f]{40}", notice_revision) or
                vcs["git"]["sha1"] != revision or
                # Legacy Cargo VCS metadata omits the repository-root path.
                # Only absence defaults to root; explicit values stay exact.
                vcs.get("path_in_vcs", "") != entry["path_in_vcs"] or
                entry["license_expression"] != package["license"]):
            raise ValueError()
        if kind in ("exact-crate-revision", "declared-license-exception"):
            if notice_revision != revision:
                raise ValueError()
        elif kind == "same-version-upstream-release":
            if (not isinstance(entry["release_tag"], str) or
                    entry["release_tag"] not in (package["version"], "v" + package["version"])):
                raise ValueError()
        else:
            raise ValueError()
        if "license_exception" in entry:
            if (key != "dispatch@0.2.0" or package["license"] != "MIT" or
                    kind != "declared-license-exception" or not entry["license_exception"]):
                raise ValueError()
        elif kind == "declared-license-exception":
            raise ValueError()
        if not isinstance(entry["files"], list) or not entry["files"]:
            raise ValueError()
    except (OSError, ValueError, KeyError, TypeError):
        raise RuntimeError("supplement provenance mismatch: " + key) from None
    notices = {}
    roles = []
    for record in entry["files"]:
        try:
            sha = record["sha256"]
            relative = record["path"]
            if not re.fullmatch(r"[0-9a-f]{64}", sha) or relative != "files/" + sha:
                raise ValueError()
        except (ValueError, KeyError, TypeError):
            raise RuntimeError("invalid supplement path: " + key) from None
        source = supplement_path(base, relative)
        try:
            data = source.read_bytes()
        except OSError:
            raise RuntimeError("missing supplement file: " + key) from None
        if digest(data) != sha or not data.strip():
            raise RuntimeError("supplement hash mismatch: " + key)
        # Exact pinned source links, never a branch or a claim that a release
        # fallback is the unavailable published crate commit.
        try:
            url = urlsplit(record["upstream_url"])
            if url.scheme != "https" or url.query or url.fragment or url.username or url.password:
                raise ValueError()
            if record.get("role") == "spdx-reference" and kind == "declared-license-exception":
                raw = (url.hostname == "raw.githubusercontent.com" and re.fullmatch(
                    r"/spdx/license-list-data/[0-9a-f]{40}/text/MIT.txt", url.path))
                blob = (url.hostname == "github.com" and re.fullmatch(
                    r"/spdx/license-list-data/blob/[0-9a-f]{40}/text/MIT.txt", url.path))
                if not (raw or blob) or url.port:
                    raise ValueError()
            else:
                prefix = repository + "/blob/" + notice_revision + "/"
                if not record["upstream_url"].startswith(prefix) or not url.path.split("/")[-1]:
                    raise ValueError()
            roles.append(record.get("role"))
        except (ValueError, KeyError, TypeError):
            raise RuntimeError("invalid supplement upstream URL: " + key) from None
        if relative in notices:
            raise RuntimeError("duplicate supplement file: " + key)
        notices[relative] = data
    if kind == "declared-license-exception" and sorted(roles) != ["license-declaration", "spdx-reference"]:
        raise RuntimeError("invalid exception evidence: " + key)
    return notices, entry


def inventory(source_root):
    root = source_root.resolve(strict=True)
    source_sha = command(["git", "rev-parse", "HEAD"], root, "source commit unavailable").strip()
    if not re.fullmatch(r"[0-9a-f]{40,64}", source_sha):
        raise RuntimeError("source commit unavailable")
    cargo_hashes = {}
    for name in ("Cargo.toml", "Cargo.lock"):
        cargo_hashes[name] = digest(safe_source(root / name, root).read_bytes())
    packages, payload = {}, {}
    cargo = cargo_executable()
    for target in TARGETS:
        metadata = json.loads(command(
            [cargo, "metadata", "--offline", "--locked", "--format-version", "1",
             "--filter-platform", target], root, "cargo metadata failed"))
        available = {p["id"]: p for p in metadata["packages"]}
        nodes = [n["id"] for n in metadata["resolve"]["nodes"]]
        if len(nodes) != len(set(nodes)) or not nodes:
            raise RuntimeError("invalid Cargo closure")
        root_id = metadata["resolve"]["root"]
        if root_id not in nodes or Path(available[root_id]["manifest_path"]).resolve() != root / "Cargo.toml":
            raise RuntimeError("invalid Cargo root")
        for package_id in nodes:
            if package_id == root_id:
                continue
            p = available[package_id]
            key = p["name"] + "@" + p["version"]
            if not re.fullmatch(r"[A-Za-z0-9_-]+@[A-Za-z0-9.+-]+", key):
                raise RuntimeError("invalid package identity")
            try:
                selected = select_license(p["license"])
            except (ValueError, IndexError):
                raise RuntimeError("unsupported or missing SPDX license: " + key) from None
            crate = safe_source(Path(p["manifest_path"]).parent, root)
            notices = notice_files(crate, root)
            provenance = None
            if not notices:
                notices, provenance = supplemental_notices(p, crate, root, key)
            records = []
            for relative, data in sorted(notices.items()):
                destination = "packages/" + key + "/" + relative
                records.append({"path": destination, "sha256": digest(data)})
                payload[destination] = data
            record = {"name": p["name"], "version": p["version"], "license": p["license"],
                      "selected_licenses": selected, "targets": [], "files": records}
            if provenance is not None:
                record["notice_provenance"] = provenance
                if "license_exception" in provenance:
                    record["license_exception"] = provenance["license_exception"]
                    record["selected_license_notice_verified"] = False
            if key in packages:
                previous_id, previous = packages[key]
                if previous_id != package_id or dict(previous, targets=[]) != record or target in previous["targets"]:
                    raise RuntimeError("ambiguous package identity: " + key)
                previous["targets"].append(target)
            else:
                record["targets"].append(target)
                packages[key] = (package_id, record)
    return {"schema_version": 1, "source_sha": source_sha, "cargo_sha256": cargo_hashes,
            "targets": TARGETS, "packages": [packages[k][1] for k in sorted(packages)]}, payload


def audit_files(audit_dir):
    if audit_dir.is_symlink():
        raise RuntimeError("symlink in audit")
    audit_dir = audit_dir.resolve(strict=True)
    files = {}
    for directory, dirs, names in os.walk(audit_dir, followlinks=False, onerror=walk_error):
        for name in dirs + names:
            path = Path(directory) / name
            if path.is_symlink():
                raise RuntimeError("symlink in audit")
            if path.is_file():
                files[path.relative_to(audit_dir).as_posix()] = path.read_bytes()
            elif not path.is_dir():
                raise RuntimeError("invalid audit file")
    return files


def summary(manifest_bytes, manifest, payload):
    return {"manifest_sha256": digest(manifest_bytes),
            "package_count": len(manifest["packages"]), "file_count": len(payload),
            "notice_inventory_complete": True,
            "legal_audit_complete": False,
            "license_exceptions": [
                {"name": p["name"], "version": p["version"], "license_exception": p["license_exception"]}
                for p in manifest["packages"] if "license_exception" in p]}


def validate_audit(audit_dir: Path, source_root: Path) -> dict:
    """Re-derive exact offline closures and notices; raise RuntimeError if invalid."""
    try:
        files = audit_files(audit_dir.absolute())
        manifest_bytes = files.pop("manifest.json")
        manifest = json.loads(manifest_bytes)
        expected, payload = inventory(source_root)
        # Exact structural equality rejects omitted/duplicate packages and files,
        # target omissions, traversal/absolute paths, and invented license data.
        if manifest != expected:
            raise RuntimeError("manifest does not match source closure")
        if files != payload:
            raise RuntimeError("notice files do not match manifest")
        return summary(manifest_bytes, manifest, payload)
    except RuntimeError:
        raise
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError):
        raise RuntimeError("invalid audit") from None


def generate(output, root):
    if output.exists() or output.is_symlink():
        raise RuntimeError("output already exists")
    if not output.parent.is_dir():
        raise RuntimeError("invalid output parent")
    manifest, payload = inventory(root)
    manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    with tempfile.TemporaryDirectory(prefix="emote-license-audit-") as temporary:
        stage = Path(temporary) / "notices"
        stage.mkdir()
        for relative, data in payload.items():
            dest = stage / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        (stage / "manifest.json").write_bytes(manifest_bytes)
        validate_audit(stage, root)
        # copytree refuses an existing destination, including a concurrent one.
        shutil.copytree(stage, output)
    return summary(manifest_bytes, manifest, payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__ +
        " Cargo metadata is offline and locked. Missing shipped notices require checked-in "
        "assets/macos-license-supplements with pinned provenance and SHA256 payloads. "
        "Explicit license exceptions are disclosed, not proof of a selected license grant.")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path, help="create a NEW notice directory (parent must exist)")
    mode.add_argument("--validate", type=Path, help="validate an existing directory against exact source closures")
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parents[1],
                        help="source checkout (default: script's repository)")
    args = parser.parse_args()
    try:
        result = (generate(args.output.absolute(), args.source_root) if args.output is not None
                  else validate_audit(args.validate, args.source_root))
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        message = str(error) if isinstance(error, RuntimeError) else "audit failed"
        print("error: " + message, file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
