"""Audit public CLI with offline fake Cargo and disposable source repositories."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/audit_macos_licenses.py"
TARGETS = ["x86_64-apple-darwin", "aarch64-apple-darwin"]


class AuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="emote-audit-test-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "source"
        self.root.mkdir()
        (self.root / "Cargo.toml").write_text('[package]\nname="app"\nversion="1.0.0"\n')
        (self.root / "Cargo.lock").write_text("version = 4\n")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Fixture", "-c",
                        "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "fixture"], check=True)
        self.crate = self.root / "vendor/fonts"
        (self.crate / "fonts").mkdir(parents=True)
        (self.crate / "LICENSE-MIT").write_text("MIT license copyright fixture\n")
        (self.crate / "fonts/OFL.txt").write_text("OFL copyright fixture\n")
        (self.crate / "fonts/egui-icons.txt").write_text("MIT copyright icons\n")
        (self.crate / "Cargo.toml").write_text("fixture")
        self.metadata = {
            "packages": [
                {"id": "app-id", "name": "app", "version": "1.0.0", "license": "MIT",
                 "manifest_path": str(self.root / "Cargo.toml")},
                {"id": "fonts-id", "name": "fonts", "version": "1.2.3", "license": "Apache-2.0 OR GPL-3.0-only",
                 "manifest_path": str(self.crate / "Cargo.toml")},
                {"id": "unused-id", "name": "unused", "version": "9", "license": None,
                 "manifest_path": str(self.root / "absent/Cargo.toml")},
            ],
            "resolve": {"root": "app-id", "nodes": [{"id": "app-id"}, {"id": "fonts-id"}]},
        }
        self.meta = self.base / "metadata.json"
        self.save_metadata()
        self.bin = self.base / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text(f"#!{sys.executable}\nimport json,sys\nfrom pathlib import Path\n"
                         "assert sys.argv[1:5] == ['metadata','--offline','--locked','--format-version']\n"
                         "assert sys.argv[5:7] == ['1','--filter-platform']\n"
                         "assert sys.argv[7] in ['x86_64-apple-darwin','aarch64-apple-darwin']\n"
                         f"data = json.loads(Path({str(self.meta)!r}).read_text())\n"
                         "print(json.dumps(data.get(sys.argv[7], data)))\n")
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ.get("PATH", ""),
                        PYTHONDONTWRITEBYTECODE="1")
        self.out = self.base / "audit"
        self.package_count = 1
        self.file_count = 3

    def save_metadata(self):
        self.meta.write_text(json.dumps(self.metadata))

    def snapshot(self):
        return {str(p.relative_to(self.base)): ("link", os.readlink(p)) if p.is_symlink()
                else ("file", p.read_bytes()) for p in self.base.rglob("*") if p.is_file() or p.is_symlink()}

    def invoke(self, mode="--output", expected_error=None):
        before = self.snapshot()
        result = subprocess.run([sys.executable, str(SCRIPT), mode, str(self.out),
                                 "--source-root", str(self.root)], env=self.env,
                                text=True, capture_output=True, timeout=10)
        if expected_error is not None:
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(result.stdout, "")
            self.assertEqual(result.stderr, "error: " + expected_error + "\n")
            self.assertEqual(self.snapshot(), before)
        else:
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, "")
            summary = json.loads(result.stdout)
            self.assertEqual(summary["package_count"], self.package_count)
            self.assertEqual(summary["file_count"], self.file_count)
            self.assertEqual(summary["manifest_sha256"], hashlib.sha256((self.out / "manifest.json").read_bytes()).hexdigest())
        return result

    def generate(self):
        self.invoke()
        return json.loads((self.out / "manifest.json").read_text())

    def rewrite(self, manifest):
        (self.out / "manifest.json").write_text(json.dumps(manifest))

    def test_generate_validate_portable_exact_closure_and_fonts(self):
        manifest = self.generate()
        self.assertNotIn(str(self.base), json.dumps(manifest))
        self.assertEqual(manifest["targets"], TARGETS)
        package = manifest["packages"][0]
        self.assertEqual(package["license"], "Apache-2.0 OR GPL-3.0-only")
        self.assertEqual(package["selected_licenses"], ["Apache-2.0"])
        self.assertEqual(package["targets"], TARGETS)
        self.assertEqual(len(package["files"]), 3)
        self.assertTrue(any("fonts/egui-icons.txt" in f["path"] for f in package["files"]))
        self.invoke("--validate")

    def test_help_describes_contract(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--help"], text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, "")
        for phrase in ("--output", "--validate", "--source-root", "offline", "legal advice"):
            self.assertIn(phrase, result.stdout)

    def test_no_overwrite(self):
        self.generate()
        self.invoke(expected_error="output already exists")

    def test_missing_or_unsupported_license(self):
        for license in (None, "GPL-3.0-only", "MIT AND Unknown-1.0", "MIT OR", "MIT garbage"):
            with self.subTest(license=license):
                self.metadata["packages"][1]["license"] = license
                self.save_metadata()
                self.invoke(expected_error="unsupported or missing SPDX license: fonts@1.2.3")

    def test_supported_license_expressions(self):
        for license in ("MIT AND (Apache-2.0 OR GPL-3.0-only)", "Unicode-3.0", "OFL-1.1",
                        "Ubuntu-font-1.0", "BSD-3-Clause", "ISC", "Zlib", "Unlicense"):
            with self.subTest(license=license):
                self.metadata["packages"][1]["license"] = license
                self.save_metadata()
                self.out = self.base / ("audit-" + str(len(list(self.base.iterdir()))))
                self.generate()
                self.invoke("--validate")

    def test_missing_notice_fails_without_output(self):
        empty = self.root / "empty"
        empty.mkdir()
        self.metadata["packages"][1]["manifest_path"] = str(empty / "Cargo.toml")
        self.save_metadata()
        self.invoke(expected_error="no notices found: fonts@1.2.3")

    def test_source_escape_and_symlink_rejected(self):
        self.metadata["packages"][1]["manifest_path"] = str(self.base / "outside/Cargo.toml")
        self.save_metadata()
        self.invoke(expected_error="dependency source outside authorized roots")
        self.metadata["packages"][1]["manifest_path"] = str(self.crate / "Cargo.toml")
        self.save_metadata()
        (self.crate / "NOTICE").symlink_to(self.base / "outside")
        self.invoke(expected_error="symlink in dependency source")

    def test_tamper_hash_and_extra_file(self):
        manifest = self.generate()
        notice = self.out / manifest["packages"][0]["files"][0]["path"]
        original = notice.read_bytes()
        notice.write_text("tamper")
        self.invoke("--validate", "notice files do not match manifest")
        notice.write_bytes(original)
        (self.out / "extra").write_text("extra")
        self.invoke("--validate", "notice files do not match manifest")

    def test_manifest_omission_duplicates_targets_and_traversal(self):
        original = self.generate()
        changes = [lambda m: m.update(packages=[]),
                   lambda m: m["packages"].append(m["packages"][0].copy()),
                   lambda m: m["packages"][0].update(targets=[TARGETS[0]]),
                   lambda m: m["packages"][0]["files"][0].update(path="../escape"),
                   lambda m: m["packages"][0]["files"].pop()]
        for change in changes:
            manifest = json.loads(json.dumps(original))
            change(manifest)
            self.rewrite(manifest)
            self.invoke("--validate", "manifest does not match source closure")

    def test_cargo_hash_and_commit_binding(self):
        self.generate()
        for name in ("Cargo.toml", "Cargo.lock"):
            path = self.root / name
            original = path.read_bytes()
            path.write_text("changed")
            self.invoke("--validate", "manifest does not match source closure")
            path.write_bytes(original)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Fixture", "-c",
                        "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "next"], check=True)
        self.invoke("--validate", "manifest does not match source closure")

    def test_output_symlink_and_invalid_json(self):
        self.out.mkdir()
        (self.out / "manifest.json").write_text("not json")
        self.invoke("--validate", "invalid audit")
        (self.out / "manifest.json").write_text("{}")
        (self.out / "escape").symlink_to(self.root)
        self.invoke("--validate", "symlink in audit")

    def test_recursive_license_directory_without_filename_extension(self):
        (self.crate / "LICENSES").mkdir()
        (self.crate / "LICENSES/Apache-2.0").write_text("Apache license notice\n")
        self.file_count = 4
        manifest = self.generate()
        self.assertTrue(any(f["path"].endswith("LICENSES/Apache-2.0")
                            for f in manifest["packages"][0]["files"]))
        self.invoke("--validate")

    def test_target_specific_closures_and_rederived_omissions(self):
        intel = json.loads(json.dumps(self.metadata))
        arm = json.loads(json.dumps(self.metadata))
        intel["packages"].append(dict(intel["packages"][1], id="extra-id", name="intel-only"))
        intel["resolve"]["nodes"].append({"id": "extra-id"})
        self.metadata = {TARGETS[0]: intel, TARGETS[1]: arm}
        self.save_metadata()
        self.package_count = 2
        self.file_count = 6
        manifest = self.generate()
        self.assertEqual(manifest["packages"][1]["name"], "intel-only")
        self.assertEqual(manifest["packages"][1]["targets"], [TARGETS[0]])
        self.invoke("--validate")
        # Validator must call Cargo again, not trust an otherwise valid inventory.
        arm["packages"].append(dict(arm["packages"][1], id="new-id", name="new-package"))
        arm["resolve"]["nodes"].append({"id": "new-id"})
        self.save_metadata()
        self.invoke("--validate", "manifest does not match source closure")

    def fallback_environment(self):
        # Standard HOME resolution, not a production test-only escape hatch.
        home = self.base / "home"
        (home / ".cargo/bin").mkdir(parents=True)
        tools = self.base / "tools"
        tools.mkdir()
        import shutil
        (tools / "git").symlink_to(shutil.which("git"))
        self.env.update(HOME=str(home), PATH=str(tools))
        return home / ".cargo/bin/cargo"

    def test_cargo_fallback_without_cargo_on_path(self):
        cargo = self.fallback_environment()
        cargo.write_bytes((self.bin / "cargo").read_bytes())
        cargo.chmod(0o755)
        self.generate()
        self.invoke("--validate")

    def test_missing_cargo_reports_lookup_failure_without_mutation(self):
        self.fallback_environment()
        self.invoke(expected_error="cargo not found on PATH or at ~/.cargo/bin/cargo")

    def test_metadata_failure_preserves_child_diagnostics(self):
        cargo = self.bin / "cargo"
        cargo.write_text(f"#!{sys.executable}\nimport sys\nprint('metadata output')\n"
                         "print('locked dependency unavailable', file=sys.stderr)\nsys.exit(7)\n")
        import shlex
        command = shlex.join([str(cargo), "metadata", "--offline", "--locked", "--format-version",
                              "1", "--filter-platform", TARGETS[0]])
        self.invoke(expected_error="cargo metadata failed (exit 7)\ncommand: " + command +
                    "\nstdout:\nmetadata output\nstderr:\nlocked dependency unavailable")


if __name__ == "__main__":
    unittest.main()
