"""Public CLI checks; never build or launch the desktop."""
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "scripts/package_macos.py"
OUTPUT = SCRIPT.parents[1] / "target/macos-package"
USAGE = ("usage: package_macos.py [-h] [--package-version PACKAGE_VERSION] [--universal]\n"
         "                        [--ad-hoc-sign] [--release-tag RELEASE_TAG]\n"
         "                        [--license-audit DIR]\n")


class PackagingCliTests(unittest.TestCase):
    def invoke(self, *args):
        outputs_before = sorted(OUTPUT.iterdir()) if OUTPUT.exists() else None
        with tempfile.TemporaryDirectory(prefix="emote-package-test-") as scratch:
            result = subprocess.run(
                [sys.executable, str(SCRIPT), *args], cwd=scratch,
                text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(list(pathlib.Path(scratch).iterdir()), [])
        self.assertEqual(
            sorted(OUTPUT.iterdir()) if OUTPUT.exists() else None, outputs_before,
        )
        return result

    def test_help_without_build_or_outputs(self):
        result = self.invoke("--help")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, "")
        self.assertIn("--package-version", result.stdout)
        self.assertIn("native-only", result.stdout)
        self.assertIn("not notarized", result.stdout)

    def test_universal_help_without_build_or_outputs(self):
        result = self.invoke("--universal", "--help")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, "")
        self.assertIn("--universal", result.stdout)
        self.assertIn("Universal2", result.stdout)
        self.assertIn("--ad-hoc-sign", result.stdout)

    def test_ad_hoc_sign_requires_universal_without_outputs(self):
        result = self.invoke("--ad-hoc-sign")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(
            result.stderr,
            USAGE + "package_macos.py: error: --ad-hoc-sign requires --universal\n",
        )

    def test_explicit_universal_signing_help_without_outputs(self):
        result = self.invoke("--universal", "--ad-hoc-sign", "--help")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, "")
        self.assertIn("not Developer ID", " ".join(result.stdout.split()))

    def test_invalid_versions_fail_before_platform_or_build_checks(self):
        for version in ("", "v0.1.4", "1.2.3.4", "1.2-beta", "-1", "1..2",
                        "1.2/3", "١.٢", "10000", "1.100", "1.2.100"):
            with self.subTest(version=version):
                result = self.invoke("--package-version=" + version)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
                self.assertEqual(
                    result.stderr,
                    USAGE + "package_macos.py: error: argument --package-version: "
                    "expected 1-3 numeric components "
                    "(major 0-9999; minor/patch 0-99)\n",
                )

    def test_unknown_option_fails_without_outputs(self):
        result = self.invoke("--publish")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(
            result.stderr,
            USAGE + "package_macos.py: error: unrecognized arguments: --publish\n",
        )

    def test_release_requirements_fail_without_outputs(self):
        cases = [
            (("--release-tag", "v0.1.5-rc.1"),
             "--release-tag requires --universal --ad-hoc-sign and --license-audit"),
            (("--license-audit", "/unused"), "--license-audit requires --release-tag"),
            (("--release-tag", "v0.1.5-rc.1", "--universal", "--ad-hoc-sign",
              "--license-audit", "/unused", "--package-version", "0.1.4"),
             "--package-version must match release tag numeric version 0.1.5"),
            (("--release-tag", "v0.1.5"),
             "argument --release-tag: expected a prerelease tag like v0.1.5-rc.1"),
        ]
        for args, message in cases:
            with self.subTest(args=args):
                result = self.invoke(*args)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
                self.assertEqual(result.stderr, USAGE + "package_macos.py: error: " + message + "\n")

    def test_valid_version_preserves_help_behavior(self):
        for version in ("0", "0.1", "0.1.4", "9999.99.99"):
            with self.subTest(version=version):
                result = self.invoke("--package-version", version, "--help")
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stderr, "")
                self.assertIn("--package-version", result.stdout)


# A public-CLI subprocess with fake external tools, never a native build/sign.
# All state (including the copy of the packager) lives in OS temporary storage.
TOOLCHAIN = r'''
import hashlib, json, pathlib, platform, runpy, shutil, subprocess, sys, zipfile
root = pathlib.Path(__file__).resolve().parent
scenario = sys.argv.pop(1)
platform.system = lambda: "Darwin"
platform.machine = lambda: "x86_64"
shutil.which = lambda tool: "/fake/" + tool
head_reads = status_reads = 0
signed = {}
def snapshot(app):
    return {str(p.relative_to(app)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in app.rglob("*") if p.is_file()}
def fake_run(command, **kwargs):
    global head_reads, status_reads
    args = list(map(str, command))
    tool = pathlib.Path(args[0]).name
    with (root / "calls.jsonl").open("a") as log:
        log.write(json.dumps(args) + "\n")
    out, err, code = "", "", 0
    if tool == "git":
        if args[1] == "rev-parse":
            head_reads += 1
            out = ("b" if scenario == "head-change" and head_reads > 1 else "a") * 40 + "\n"
        else:
            status_reads += 1
            out = "?? .pi/state\n" if scenario == "dirty" or (
                scenario == "dirty-exit" and status_reads > 1) else ""
    elif tool == "cargo":
        if args[1] == "metadata":
            out = json.dumps({"packages": [{"manifest_path": str(root / "Cargo.toml"),
                                          "version": "0.1.0"}]})
        else:
            target = args[args.index("--target") + 1]
            dest = pathlib.Path(args[-1]) / target / "release/gentle-emote-desktop"
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text("arm64" if target.startswith("aarch64") else "x86_64")
            if scenario == "cargo-change":
                (root / "Cargo.lock").write_text("mutated")
    elif tool == "file":
        out = "Mach-O " + pathlib.Path(args[-1]).read_text()
    elif tool == "lipo":
        if "-create" in args:
            pathlib.Path(args[-1]).write_text("x86_64 arm64")
        else:
            out = pathlib.Path(args[-1]).read_text()
    elif tool == "otool":
        out = "binary:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n"
    elif tool == "codesign":
        path = pathlib.Path(args[-1])
        if "--sign" in args:
            assert path.suffix == ".app", "signed executable before bundle completion"
            assert (path / "Contents/Info.plist").is_file()
            resources = path / "Contents/Resources"
            receipt = json.loads((resources / "packaging-receipt.json").read_text())
            assert (resources / "LICENSE").is_file() and (resources / "README.txt").is_file()
            if "--release-tag" in sys.argv:
                assert (resources / "ThirdPartyNotices/manifest.json").is_file()
            if scenario == "license-exception":
                assert receipt["license_exceptions"], "exception missing before signing"
                assert "standard MIT reference is not dispatch's original notice" in (resources / "README.txt").read_text()
            assert "code_signature" not in receipt, "embedded signature would become stale"
            signed[str(path)] = snapshot(path)
        elif "--display" in args:
            err = "Executable={}\n".format(path.resolve())
            if (path.is_file() and path.read_text() == "x86_64 arm64" and not signed
                    and scenario != "invalid-signature"):
                code, err = 1, err + "code object is not signed at all"
            else:
                err += "Signature=adhoc\nCDHash=" + "c" * 40 + "\n"
        else:
            if (scenario == "invalid-signature" and path.is_file()
                    and path.read_text() == "x86_64 arm64" and not signed):
                code, err = 1, "{}: invalid signature (code or signature have been modified)\n".format(path)
            for app, before in signed.items():
                assert snapshot(pathlib.Path(app)) == before, "bundle mutated after signing"
    elif tool == "ditto":
        app, archive = map(pathlib.Path, args[-2:])
        for original, before in signed.items():
            assert snapshot(pathlib.Path(original)) == before
        with zipfile.ZipFile(archive, "w") as z:
            for p in app.rglob("*"):
                if p.is_file():
                    z.write(p, p.relative_to(app.parent))
    else:
        raise AssertionError(args)
    result = subprocess.CompletedProcess(args, code, out, err)
    if kwargs.get("check"):
        result.check_returncode()
    return result
subprocess.run = fake_run
sys.argv[0] = str(root / "scripts/package_macos.py")
sys.path.insert(0, str(root / "scripts"))
runpy.run_path(sys.argv[0], run_name="__main__")
'''

AUDIT_STUB = '''
import hashlib
from pathlib import Path
def validate_audit(audit_dir, source_root):
    audit_dir = Path(audit_dir)
    if (source_root / "reject-audit").exists():
        raise RuntimeError("invalid notice fixture")
    if (source_root / "reject-copy").exists() and audit_dir.name == "ThirdPartyNotices":
        raise RuntimeError("invalid copied notice fixture")
    import json
    exceptions = json.loads((audit_dir / "manifest.json").read_text()).get("license_exceptions", [])
    if (source_root / "change-exception").exists() and audit_dir.name == "ThirdPartyNotices":
        exceptions = []
    return {"manifest_sha256": hashlib.sha256((audit_dir / "manifest.json").read_bytes()).hexdigest(),
            "package_count": 1, "file_count": 1, "license_exceptions": exceptions,
            "notice_inventory_complete": not (source_root / "incomplete-inventory").exists(),
            "legal_audit_complete": False}
'''


class PackagingToolchainTests(unittest.TestCase):
    def fixture(self, scenario="clean", release=True, native=False, sign=True):
        scratch = tempfile.TemporaryDirectory(prefix="emote-package-toolchain-")
        self.addCleanup(scratch.cleanup)
        root = pathlib.Path(scratch.name)
        (root / "scripts").mkdir()
        shutil.copyfile(SCRIPT, root / "scripts/package_macos.py")
        (root / "scripts/audit_macos_licenses.py").write_text(AUDIT_STUB)
        for name in ("Cargo.toml", "Cargo.lock", "LICENSE"):
            (root / name).write_text("fixture")
        audit = root / "audit"
        audit.mkdir()
        exceptions = ([{"name": "dispatch", "version": "0.2.0",
                        "license_exception": "MIT declared in exact upstream Cargo.toml; no original notice; standard MIT reference only"}]
                      if scenario in ("license-exception", "exception-mismatch") else [])
        (audit / "manifest.json").write_text(json.dumps({"license_exceptions": exceptions}))
        if scenario == "exception-mismatch":
            (root / "change-exception").touch()
        if scenario == "incomplete-inventory":
            (root / "incomplete-inventory").touch()
        (audit / "NOTICE.txt").write_text("public notice")
        if scenario == "invalid-audit":
            (root / "reject-audit").touch()
        if scenario == "invalid-copy":
            (root / "reject-copy").touch()
        launcher = root / "toolchain.py"
        launcher.write_text(TOOLCHAIN)
        args = [] if native else ["--universal"]
        if sign:
            args += ["--ad-hoc-sign"]
        if release:
            args += ["--release-tag", "v0.1.5-rc.1", "--license-audit", str(audit)]
        result = subprocess.run([sys.executable, "-B", str(launcher), scenario, *args],
                                cwd=root, capture_output=True, text=True, timeout=10)
        log = root / "calls.jsonl"
        calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        return root, result, calls

    def test_complete_bundle_signed_last_and_release_receipt(self):
        root, result, calls = self.fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        outputs = list((root / "target/macos-package").glob("*-v0.1.5-rc.1-universal2-*"))
        self.assertEqual(len(outputs), 1)
        output = outputs[0].resolve()
        archive = output / (output.name + ".zip")
        self.assertEqual(result.stdout,
                         "Reserved output: {} (failure may leave an incomplete directory)\n"
                         "Building Universal2 release (no application launch)...\n"
                         "Bundle: {}\nZIP: {}\nSHA-256: {}\n"
                         "Signature class: ad-hoc; not notarized; explicit ad-hoc signing performed.\n"
                         .format(output, output / "Gentle Emote Desktop.app", archive,
                                 str(archive) + ".sha256"))
        app = outputs[0] / "Gentle Emote Desktop.app"
        receipt = json.loads((app / "Contents/Resources/packaging-receipt.json").read_text())
        self.assertEqual(receipt["artifact_status"], "prerelease")
        self.assertEqual(receipt["package_version"], "0.1.5")
        self.assertEqual(receipt["source_sha"], "a" * 40)
        self.assertIs(receipt["source_dirty"], False)
        self.assertIs(receipt["notice_inventory_complete"], True)
        self.assertIs(receipt["legal_audit_complete"], False)
        self.assertIs(receipt["third_party_license_audit_complete"], False)
        self.assertIs(receipt["arm_runtime_validated"], False)
        self.assertNotIn(str(root), json.dumps(receipt["license_audit"]))
        self.assertEqual(len([c for c in calls if "--sign" in c]), 1)
        self.assertTrue(any("--verify" in c and "--all-architectures" in c for c in calls))
        self.assertEqual(len(list(outputs[0].glob("*.zip"))), 1)
        self.assertEqual(len(list(outputs[0].glob("*.zip.sha256"))), 1)
        evidence = json.loads((outputs[0] / "signature-verification.json").read_text())
        self.assertIs(evidence["packager_signing_performed"], True)
        self.assertIs(evidence["exported_signatures_verified"], True)
        self.assertIs(evidence["zip_extraction_verified"], False)
        self.assertEqual(evidence["bundle_signature"]["class"], "ad-hoc")
        self.assertEqual(evidence["executable_signature"]["class"], "ad-hoc")
        self.assertEqual((app / "Contents/Resources/ThirdPartyNotices/NOTICE.txt").read_text(),
                         "public notice")

    def test_dispatch_exception_is_disclosed_without_legal_audit_claim(self):
        root, result, calls = self.fixture("license-exception")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertIn("Signature class: ad-hoc; not notarized; explicit ad-hoc signing performed.", result.stdout)
        output = next(root.glob("target/macos-package/*-v0.1.5-rc.1-universal2-*"))
        resources = output / "Gentle Emote Desktop.app/Contents/Resources"
        expected = json.loads((root / "audit/manifest.json").read_text())["license_exceptions"]
        embedded = json.loads((resources / "packaging-receipt.json").read_text())
        external = json.loads((output / "signature-verification.json").read_text())
        for evidence in (embedded, external):
            self.assertIs(evidence["notice_inventory_complete"], True)
            self.assertIs(evidence["legal_audit_complete"], False)
            self.assertIs(evidence["third_party_license_audit_complete"], False)
            self.assertEqual(evidence["license_exceptions"], expected)
            self.assertEqual(evidence["license_audit"]["license_exceptions"], expected)
        self.assertEqual((resources / "ThirdPartyNotices/manifest.json").read_bytes(),
                         (root / "audit/manifest.json").read_bytes())
        disclosure = (resources / "README.txt").read_text()
        self.assertIn("dispatch 0.2.0 declares MIT", disclosure)
        self.assertIn("no original copyright/license notice", disclosure)
        self.assertIn("standard MIT reference is not dispatch's original notice", disclosure)
        self.assertIn("No complete legal-license audit is claimed.", disclosure)
        self.assertEqual(len([c for c in calls if "--sign" in c]), 1)

    def test_signature_evidence_is_portable_in_all_receipt_fields(self):
        for scenario, merged_class in (("clean", "unsigned"), ("invalid-signature", "invalid")):
            with self.subTest(scenario=scenario):
                root, result, _ = self.fixture(scenario)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                output = next(root.glob("target/macos-package/*-v0.1.5-rc.1-universal2-*"))
                receipt = json.loads((output / "Gentle Emote Desktop.app/Contents/Resources/packaging-receipt.json").read_text())
                external = json.loads((output / "signature-verification.json").read_text())
                for artifact in (receipt, external):
                    serialized = json.dumps(artifact)
                    self.assertNotIn("Executable=", serialized)
                    self.assertNotIn(str(root.resolve()), serialized)
                    self.assertNotIn("/private/", serialized)
                    self.assertNotIn('"inspection"', serialized)
                signatures = [receipt["binary_signature_before_bundle_signing"],
                              receipt["merged_signature_before_signing"],
                              *[thin["code_signature"] for thin in receipt["thin_inspections"].values()],
                              external["bundle_signature"], external["executable_signature"]]
                for evidence, expected_class in zip(signatures,
                                                    (merged_class, merged_class, "ad-hoc", "ad-hoc", "ad-hoc", "ad-hoc")):
                    self.assertEqual(evidence, {"class": expected_class,
                                                "verified": expected_class == "ad-hoc"})

    def test_rejected_source_and_audit_never_export(self):
        for scenario, message in (("dirty", "release source must be clean"),
                                  ("dirty-exit", "release source must be clean"),
                                  ("head-change", "release source changed during packaging"),
                                  ("cargo-change", "release source changed during packaging"),
                                  ("invalid-audit", "invalid notice fixture"),
                                  ("invalid-copy", "invalid copied notice fixture"),
                                  ("exception-mismatch", "copied license audit differs from validated input"),
                                  ("incomplete-inventory", "validated notice inventory is incomplete")):
            with self.subTest(scenario=scenario):
                root, result, calls = self.fixture(scenario)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stderr, "Packaging failed: " + message + "\n")
                self.assertEqual(list(root.glob("target/macos-package/*/*.zip")), [])
                self.assertEqual(list(root.glob("target/macos-package/*/*.app")), [])
                if scenario in ("dirty", "invalid-audit", "incomplete-inventory"):
                    self.assertEqual(result.stdout, "")
                    self.assertFalse(any(c[1] == "build" for c in calls))
                    self.assertFalse((root / "target").exists())

    def test_native_default_preserves_target_and_no_signing(self):
        root, result, calls = self.fixture(release=False, native=True, sign=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        builds = [c for c in calls if c[1] == "build"]
        self.assertEqual(len(builds), 1)
        self.assertEqual(builds[0][builds[0].index("--target") + 1], "x86_64-apple-darwin")
        self.assertFalse(any("--sign" in c for c in calls))
        self.assertEqual(len(list(root.glob("target/macos-package/*-x86_64-local-*/*.zip"))), 1)

    def test_unsigned_universal_still_requires_explicit_signing(self):
        root, result, calls = self.fixture(release=False, sign=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Apple Silicon requires valid code integrity. No signing was performed.", result.stderr)
        self.assertFalse(any("--sign" in c for c in calls))
        self.assertEqual(list(root.glob("target/macos-package/*/*.zip")), [])

    def test_local_universal_signing_preserves_local_receipt(self):
        root, result, _ = self.fixture(release=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        receipts = list(root.glob("target/macos-package/*-local-*/*.app/Contents/Resources/packaging-receipt.json"))
        self.assertEqual(len(receipts), 1)
        receipt = json.loads(receipts[0].read_text())
        self.assertEqual(receipt["artifact_status"], "local build; not an official release")
        self.assertIs(receipt["third_party_license_audit_complete"], False)


if __name__ == "__main__":
    unittest.main()
