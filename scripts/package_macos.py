#!/usr/bin/env python3
"""Build a local or clean-source prerelease macOS bundle without publishing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
NAME = "gentle-emote-desktop"
BUNDLE_ID = "org.gentle-emote.desktop"


def package_version(value):
    if not re.fullmatch(r"[0-9]{1,4}(?:\.[0-9]{1,2}){0,2}", value):
        raise argparse.ArgumentTypeError(
            "expected 1-3 numeric components (major 0-9999; minor/patch 0-99)"
        )
    return value


def release_tag(value):
    match = re.fullmatch(r"v([0-9]+\.[0-9]+\.[0-9]+)-rc\.[1-9][0-9]*", value)
    if not match:
        raise argparse.ArgumentTypeError("expected a prerelease tag like v0.1.5-rc.1")
    package_version(match.group(1))
    return value


def source_state():
    sha = run("git", "rev-parse", "HEAD").stdout.strip()
    dirty = bool(run("git", "status", "--porcelain", "--untracked-files=all").stdout)
    hashes = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
              for name in ("Cargo.toml", "Cargo.lock")}
    return sha, dirty, hashes


def require_frozen_source(expected, observed):
    if observed[1]:
        raise RuntimeError("release source must be clean")
    if observed != expected:
        raise RuntimeError("release source changed during packaging")


def run(*args):
    return subprocess.run(
        [str(arg) for arg in args], cwd=ROOT, check=True,
        text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )


def signature(executable, universal=False):
    command = ["/usr/bin/codesign", "--display", "--verbose=4", str(executable)]
    result = subprocess.run(command, text=True, capture_output=True)
    details = result.stdout + result.stderr
    if result.returncode:
        if result.returncode == 1 and "code object is not signed at all" in details:
            return {"class": "unsigned", "verified": False}
        raise subprocess.CalledProcessError(
            result.returncode, command, result.stdout, result.stderr
        )
    verify_options = ("--all-architectures",) if universal else ()
    run("/usr/bin/codesign", "--verify", "--strict", *verify_options, executable)
    # Persist only an explicit portable schema. Verbose codesign output includes
    # Executable=<absolute path> and other host-local diagnostics, not metadata
    # suitable for a public receipt. Failures still propagate to console handling.
    return {
        "class": "ad-hoc" if "Signature=adhoc" in details else "signed",
        "verified": True,
    }


def inspect(executable, arch):
    file_description = run("/usr/bin/file", "-b", executable).stdout.strip()
    arches = run("/usr/bin/lipo", "-archs", executable).stdout.split()
    if arches != [arch] or "Mach-O" not in file_description or arch not in file_description:
        raise RuntimeError("executable is not a native-only Mach-O: " + file_description)
    dependencies = inspect_dependencies(executable)
    return file_description, dependencies, signature(executable)


def inspect_dependencies(executable, arch=None):
    options = ("-arch", arch) if arch else ()
    dependency_report = run("/usr/bin/otool", *options, "-L", executable).stdout
    dependencies = []
    for line in dependency_report.splitlines()[1:]:
        match = re.fullmatch(r"\s+(.+?) \(compatibility version .+\)", line)
        if not match:
            raise RuntimeError("unrecognized otool dependency: " + line)
        dependency = match.group(1)
        if not dependency.startswith(("/System/Library/", "/usr/lib/")):
            raise RuntimeError("standalone bundle cannot resolve dependency: " + dependency)
        dependencies.append(dependency)
    if not dependencies:
        raise RuntimeError("otool reported no dependencies; refusing unverified bundle")
    return dependencies


def inspect_universal(executable, ad_hoc_sign):
    description = run("/usr/bin/file", "-b", executable).stdout.strip()
    arches = run("/usr/bin/lipo", "-archs", executable).stdout.split()
    if (len(arches) != 2 or set(arches) != {"x86_64", "arm64"}
            or "Mach-O" not in description):
        raise RuntimeError("expected exactly x86_64 and arm64 Mach-O slices: " + description)
    dependencies = {
        arch: inspect_dependencies(executable, arch) for arch in ("x86_64", "arm64")
    }
    # Inspect the merged output, not the thin binaries' old signature metadata.
    # An invalid signature is expected on some lipo/toolchain combinations.
    try:
        before_signing = signature(executable, universal=True)
    except subprocess.CalledProcessError:
        before_signing = {"class": "invalid", "verified": False}
    if not ad_hoc_sign and before_signing["class"] in ("unsigned", "invalid"):
        raise RuntimeError(
            "merged Universal2 executable signature is {}.\n"
            "Apple Silicon requires valid code integrity. No signing was performed. "
            "After explicit approval, rerun with --universal --ad-hoc-sign "
            "(ad-hoc only, not Developer ID; existing output reservations are not reused)."
            .format(before_signing["class"])
        )
    # Signing must wait until Info.plist and every resource exist.
    return description, dependencies, before_signing


def package(args):
    if platform.system() != "Darwin":
        raise RuntimeError("packaging requires macOS; cross-compilation is not supported")
    arch = platform.machine()
    if arch not in ("arm64", "x86_64"):
        raise RuntimeError("unsupported native architecture: " + arch)
    cargo = shutil.which("cargo")
    if not cargo:
        fallback = Path.home() / ".cargo/bin/cargo"
        if fallback.is_file() and os.access(fallback, os.X_OK):
            cargo = str(fallback)
    if not cargo:
        raise RuntimeError("cargo not found on PATH or at ~/.cargo/bin/cargo")
    frozen_source = source_state()
    source_sha, dirty, cargo_hashes = frozen_source
    audit_summary = None
    if args.release_tag:
        require_frozen_source(frozen_source, frozen_source)
        # Local packaging stays independent of the optional release audit helper.
        from audit_macos_licenses import validate_audit
        audit_summary = validate_audit(args.license_audit, ROOT)
        if audit_summary.get("notice_inventory_complete") is not True:
            raise RuntimeError("validated notice inventory is incomplete")
    metadata = json.loads(run(
        cargo, "metadata", "--no-deps", "--format-version", "1", "--locked"
    ).stdout)
    manifest = ROOT / "Cargo.toml"
    crate = next(p for p in metadata["packages"] if Path(p["manifest_path"]) == manifest)
    cargo_version = crate["version"]
    version = args.package_version or package_version(cargo_version)
    qualifier = source_sha[:12] + ("-dirty" if dirty else "-clean")
    artifact_arch = "universal2" if args.universal else arch
    stem = ("{}-{}-{}-{}".format(NAME, args.release_tag, artifact_arch, source_sha[:12])
            if args.release_tag else
            "{}-{}-{}-local-{}".format(NAME, version, artifact_arch, qualifier))
    output_root = ROOT / "target/macos-package"
    output = output_root / stem
    # Reserve once, before expensive work; never reuse or erase an existing output.
    output.mkdir(parents=True, exist_ok=False)
    print("Reserved output: {} (failure may leave an incomplete directory)".format(output),
          flush=True)
    target = ("aarch64" if arch == "arm64" else "x86_64") + "-apple-darwin"
    targets = ["x86_64-apple-darwin", "aarch64-apple-darwin"] if args.universal else [target]
    architectures = ["x86_64", "arm64"] if args.universal else [arch]
    build = output_root / "build"
    print("Building {} release (no application launch)...".format(
        "Universal2" if args.universal else "native"), flush=True)
    binaries = []
    thin_inspections = {}
    for build_target, build_arch in zip(targets, architectures):
        result = run(cargo, "build", "--release", "--locked", "--target", build_target,
                     "--target-dir", build)
        print(result.stdout, end="")
        print(result.stderr, end="", file=sys.stderr)
        binary = build / build_target / "release" / NAME
        if args.universal:
            description, dependencies, code_signature = inspect(binary, build_arch)
            thin_inspections[build_arch] = {
                "file": description, "dependencies": dependencies,
                "code_signature": code_signature,
            }
        binaries.append(binary)
    # Only this invocation's system-temp stage is automatically removed.
    with tempfile.TemporaryDirectory(prefix="emote-macos-package-") as scratch:
        stage = Path(scratch)
        app = stage / "Gentle Emote Desktop.app"
        contents = app / "Contents"
        macos = contents / "MacOS"
        resources = contents / "Resources"
        macos.mkdir(parents=True)
        resources.mkdir()
        executable = macos / NAME
        if args.universal:
            run("/usr/bin/lipo", "-create", *binaries, "-output", executable)
        else:
            shutil.copy2(binaries[0], executable)
        executable.chmod(0o755)
        if args.universal:
            description, dependencies, code_signature = inspect_universal(
                executable, args.ad_hoc_sign,
            )
        else:
            description, dependencies, code_signature = inspect(executable, arch)
        info = {
            "CFBundleIdentifier": BUNDLE_ID,
            "CFBundleName": "Gentle Emote Desktop",
            "CFBundleDisplayName": "Gentle Emote Desktop",
            "CFBundleExecutable": NAME,
            "CFBundlePackageType": "APPL",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleVersion": version,
            "CFBundleShortVersionString": version,
        }
        with (contents / "Info.plist").open("wb") as handle:
            plistlib.dump(info, handle, sort_keys=True)
        receipt = {
            "artifact_status": "prerelease" if args.release_tag else "local build; not an official release",
            "release_tag": args.release_tag,
            "cargo_source_sha256": cargo_hashes,
            "source_sha": source_sha, "source_dirty": dirty,
            "cargo_version": cargo_version, "package_version": version,
            "architecture": artifact_arch,
            "rust_target": None if args.universal else target,
            "mode": "universal2" if args.universal else "native",
            "architectures": architectures, "rust_targets": targets,
            "validation_scope": "static packaging inspection only; no runtime tests performed",
            "arm_runtime_validated": False,
            "file": description, "dependencies": dependencies,
            "binary_signature_before_bundle_signing": code_signature,
            "packager_signing_intent": "ad-hoc complete bundle" if args.ad_hoc_sign else "none",
            "final_signature_evidence": "external signature-verification.json; not embedded",
            "packager_signing_kind": "ad-hoc" if args.ad_hoc_sign else "none",
            "notarized": False,
            "notice_inventory_complete": False,
            "legal_audit_complete": False,
            "third_party_license_audit_complete": False,
            "license_exceptions": [],
            "license_audit": None,
        }
        if args.universal:
            receipt["thin_inspections"] = thin_inspections
            receipt["merged_signature_before_signing"] = code_signature
        if args.release_tag:
            notices = resources / "ThirdPartyNotices"
            shutil.copytree(args.license_audit, notices)
            copied_summary = validate_audit(notices, ROOT)
            if copied_summary != audit_summary:
                raise RuntimeError("copied license audit differs from validated input")
            receipt["license_audit"] = {key: copied_summary[key] for key in
                                        ("manifest_sha256", "package_count", "file_count",
                                         "notice_inventory_complete", "legal_audit_complete",
                                         "license_exceptions")}
            receipt["notice_inventory_complete"] = copied_summary["notice_inventory_complete"]
            receipt["license_exceptions"] = copied_summary["license_exceptions"]
            # A validated inventory is not a legal assurance. Preserve the
            # auditor's distinction and never declare a full audit on exception.
            receipt["legal_audit_complete"] = (
                copied_summary["legal_audit_complete"] is True
                and not receipt["license_exceptions"]
            )
            receipt["third_party_license_audit_complete"] = receipt["legal_audit_complete"]
        shutil.copyfile(ROOT / "LICENSE", resources / "LICENSE")
        (resources / "packaging-receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        (resources / "README.txt").write_text(
            "Gentle Emote Desktop — {} {} macOS build\n".format(
                args.release_tag or "local", "Universal2" if args.universal else "native-only") +
            ("Prerelease from frozen source commit {}.\n".format(source_sha) if args.release_tag
             else "Not an official release.\n") +
            "No Developer ID signing or notarization performed.\n"
            "Signing intent: {}; final verification is external to this signed bundle.\n".format(
                "explicit ad-hoc complete bundle" if args.ad_hoc_sign else "none") +
            "Gatekeeper may block downloaded copies. No security bypass is automated.\n" +
            ("Validated dependency-notice inventory: ThirdPartyNotices/manifest.json.\n"
             "No complete legal-license audit is claimed.\n" if args.release_tag
             else "Not a complete third-party license audit; LICENSE covers this project.\n") +
            ("License exception: dispatch 0.2.0 declares MIT in upstream Cargo.toml;\n"
             "no original copyright/license notice is available. The included\n"
             "standard MIT reference is not dispatch's original notice;\n"
             "no copyright holder or year is invented. See ThirdPartyNotices/manifest.json\n"
             "for exact provenance and the complete exception records.\n"
             if any(e['name'] == 'dispatch' and e['version'] == '0.2.0'
                    for e in receipt["license_exceptions"]) else "") +
            "".join("License exception: {} {}: {}\n".format(
                e['name'], e['version'], e['license_exception'])
                for e in receipt['license_exceptions']) +
            "No minimum macOS compatibility guarantee or ARM runtime validation claim.\n"
            "The app listens on 127.0.0.1:8001; close other instances before opening.\n",
            encoding="utf-8",
        )
        for path in (contents / "Info.plist", *resources.rglob("*")):
            if path.is_file():
                path.chmod(0o644)
        # The resource seal must cover the final receipt/notices/Info.plist. Never
        # mutate this app again after signing, even to record observed signatures.
        if args.ad_hoc_sign:
            run("/usr/bin/codesign", "--force", "--sign", "-", app)
            bundle_signature = signature(app, universal=True)
            code_signature = signature(executable, universal=True)
            if (bundle_signature["class"] != "ad-hoc"
                    or code_signature["class"] != "ad-hoc"):
                raise RuntimeError("complete bundle did not receive an ad-hoc signature")
        else:
            # Preserve local unsigned/linker-signed packaging: without opt-in,
            # only the executable's pre-bundle signature was inspected.
            bundle_signature = {"class": "not verified; no bundle signing requested"}
        archive = stage / (stem + ".zip")
        run("/usr/bin/ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", app, archive)
        digest = hashlib.sha256()
        with archive.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        # Export only after inspection and archive creation succeed; exclusive files
        # and copytree's default refusal prevent overwriting even on partial retries.
        if args.release_tag:
            require_frozen_source(frozen_source, source_state())
        exported_app = output / app.name
        shutil.copytree(app, exported_app, copy_function=shutil.copy2)
        exported_bundle_signature = bundle_signature
        exported_binary_signature = code_signature
        if args.ad_hoc_sign:
            exported_bundle_signature = signature(exported_app, universal=True)
            exported_binary_signature = signature(
                exported_app / "Contents/MacOS" / NAME, universal=True,
            )
        with archive.open("rb") as source, (output / archive.name).open("xb") as dest:
            shutil.copyfileobj(source, dest)
        with (output / (archive.name + ".sha256")).open("x", encoding="utf-8") as handle:
            handle.write(digest.hexdigest() + "  " + archive.name + "\n")
        with (output / "signature-verification.json").open("x", encoding="utf-8") as handle:
            json.dump({"source_sha": source_sha, "release_tag": args.release_tag,
                       **{key: receipt[key] for key in
                          ("license_audit", "license_exceptions", "notice_inventory_complete",
                           "legal_audit_complete", "third_party_license_audit_complete")},
                       "packager_signing_performed": args.ad_hoc_sign,
                       "bundle_signature": exported_bundle_signature,
                       "executable_signature": exported_binary_signature,
                       "all_architectures": args.universal,
                       "exported_signatures_verified": args.ad_hoc_sign,
                       "zip_extraction_verified": False}, handle, indent=2, sort_keys=True)
            handle.write("\n")
        if args.release_tag:
            require_frozen_source(frozen_source, source_state())
    print("Bundle: " + str(output / app.name))
    print("ZIP: " + str(output / archive.name))
    print("SHA-256: " + str(output / (archive.name + ".sha256")))
    print("Signature class: {}; not notarized; {}.".format(
        code_signature["class"],
        "explicit ad-hoc signing performed" if args.ad_hoc_sign else "no signing performed"))


def main():
    parser = argparse.ArgumentParser(
        description="Build a local or prerelease macOS .app and ZIP; native-only by default, "
                    "Universal2 opt-in; not notarized. No automatic signing, "
                    "publication, installation, or application launch.",
        epilog="Outputs: target/macos-package/<name>-<version>-<arch>-local-<revision>"
               "[-dirty|-clean]/; prereleases use <name>-<release-tag>-<arch>-<revision>/. "
               "Existing output directories are never reused.",
    )
    parser.add_argument(
        "--package-version", type=package_version,
        help="bundle version: 1-3 numeric components (major 0-9999; minor/patch 0-99); "
             "defaults to Cargo version; never edits Cargo.toml",
    )
    parser.add_argument(
        "--universal", action="store_true",
        help="build Universal2: x86_64 and arm64; requires both Rust targets installed",
    )
    parser.add_argument(
        "--ad-hoc-sign", action="store_true",
        help="explicitly ad-hoc sign the complete new bundle (not Developer ID); "
             "requires --universal",
    )
    parser.add_argument(
        "--release-tag", type=release_tag,
        help="prerelease tag like v0.1.5-rc.1; maps to numeric bundle version 0.1.5; "
             "requires clean committed source, --universal, --ad-hoc-sign and --license-audit",
    )
    parser.add_argument(
        "--license-audit", type=Path, metavar="DIR",
        help="validated dependency-notice directory; requires --release-tag",
    )
    args = parser.parse_args()
    if args.release_tag:
        if not (args.universal and args.ad_hoc_sign and args.license_audit):
            parser.error("--release-tag requires --universal --ad-hoc-sign and --license-audit")
        numeric_version = args.release_tag[1:].split("-", 1)[0]
        if args.package_version and args.package_version != numeric_version:
            parser.error("--package-version must match release tag numeric version " + numeric_version)
        args.package_version = numeric_version
    elif args.license_audit:
        parser.error("--license-audit requires --release-tag")
    if args.ad_hoc_sign and not args.universal:
        parser.error("--ad-hoc-sign requires --universal")
    try:
        package(args)
    except subprocess.CalledProcessError as error:
        print("Command failed (exit {}): {}".format(error.returncode, error.cmd),
              file=sys.stderr)
        print(error.stdout or "", end="", file=sys.stdout)
        print(error.stderr or "", end="", file=sys.stderr)
        return error.returncode if error.returncode > 0 else 1
    except (OSError, RuntimeError, ValueError, StopIteration) as error:
        print("Packaging failed: {}".format(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
