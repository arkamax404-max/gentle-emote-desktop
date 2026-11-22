"""Linux source-bound offline notice CLI checks; no native build."""
import json
import shutil
from test_package_linux import LinuxFixture, TARGET


class AuditTests(LinuxFixture):
    def audit_cli(self, mode='--output', scenario='clean'):
        return self.cli('audit_linux_licenses.py', ['--source-root', self.root, mode, self.audit], scenario)

    def failure(self, message, mode='--output', scenario='clean'):
        before = {str(p.relative_to(self.root)): p.read_bytes()
                  for p in self.root.rglob('*') if p.is_file()}
        result = self.audit_cli(mode, scenario)
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (1, '', 'error: ' + message + '\n'))
        self.assertEqual(before, {str(p.relative_to(self.root)): p.read_bytes()
                                for p in self.root.rglob('*') if p.is_file()})
        if mode == '--output':
            self.assertFalse(self.audit.exists())

    def test_exact_linux_closure_fonts_and_tampering(self):
        self.generate()
        manifest = json.loads((self.audit / 'manifest.json').read_text())
        self.assertEqual(manifest['targets'], [TARGET])
        self.assertEqual(manifest['source_sha'], 'a' * 40)
        self.assertEqual(len(manifest['packages']), 1)
        self.assertEqual(len(manifest['packages'][0]['files']), 2)
        self.assertNotIn(str(self.base), json.dumps(manifest))
        result = self.audit_cli('--validate')
        self.assertEqual((result.returncode, result.stderr), (0, ''))
        summary = json.loads(result.stdout)
        self.assertTrue(summary['notice_inventory_complete'])
        self.assertFalse(summary['legal_audit_complete'])
        (self.audit / 'extra').write_text('tampered')
        self.failure('notice files do not match manifest', '--validate')

    def test_missing_original_notices_and_link_only_declarations_fail_closed(self):
        self.metadata['packages'][1]['manifest_path'] = str(self.root / 'empty/Cargo.toml')
        (self.root / 'empty').mkdir()
        self.save_metadata()
        self.failure('no original license notices: fonts@1.2.3 (MIT)')
        (self.root / 'empty/LICENSE').write_text('See https://example.invalid/LICENSE')
        self.failure('no original license notices: fonts@1.2.3 (MIT)')

    def test_spdx_selection_needs_corresponding_original_text(self):
        self.metadata['packages'][1]['license'] = 'Apache-2.0 OR MIT'
        self.save_metadata()
        self.failure('no original license notices: fonts@1.2.3 (Apache-2.0)')
        self.metadata['packages'][1]['license'] = 'GPL-3.0-only'
        self.save_metadata()
        self.failure('unsupported or missing SPDX license: fonts@1.2.3')

    def test_dirty_mutated_source_and_escaping_dependency(self):
        self.failure('source must be clean and committed', scenario='dirty')
        self.failure('source changed during packaging', scenario='mutated')
        self.metadata['packages'][1]['manifest_path'] = '/outside/Cargo.toml'
        self.save_metadata()
        self.failure('dependency source outside authorized roots')

    def test_registry_root_selection_and_boundary_from_public_cli(self):
        for configured in (True, False):
            with self.subTest(configured=configured):
                home = self.base / ('configured home' if configured else 'default home')
                cargo_home = home / ('chosen cargo' if configured else '.cargo')
                crate = cargo_home / 'registry/src/fixture-index/fonts-1.2.3'
                shutil.copytree(self.crate, crate)
                self.env['HOME'] = str(home)
                self.env.pop('CARGO_HOME', None)
                if configured:
                    self.env['CARGO_HOME'] = str(cargo_home)
                self.metadata['packages'][1]['manifest_path'] = str(crate / 'Cargo.toml')
                self.save_metadata()
                self.audit = self.base / ('configured-audit' if configured else 'default-audit')
                self.generate()
                result = self.audit_cli('--validate')
                self.assertEqual((result.returncode, result.stderr), (0, ''))
                self.assertTrue(json.loads(result.stdout)['notice_inventory_complete'])
                before = {str(p.relative_to(self.audit)): p.read_bytes()
                          for p in self.audit.rglob('*') if p.is_file()}
                self.env['CARGO_HOME'] = str(home / 'different cargo')
                self.failure('dependency source outside authorized roots', '--validate')
                self.assertEqual(before, {str(p.relative_to(self.audit)): p.read_bytes()
                                         for p in self.audit.rglob('*') if p.is_file()})
                alias = home / 'linked cargo'
                alias.symlink_to(cargo_home, target_is_directory=True)
                self.env['CARGO_HOME'] = str(alias)
                self.metadata['packages'][1]['manifest_path'] = str(
                    alias / 'registry/src/fixture-index/fonts-1.2.3/Cargo.toml')
                self.save_metadata()
                self.failure('symlink in dependency source', '--validate')

    def test_binding_collision_symlink_and_manifest_omission(self):
        self.generate()
        result = self.audit_cli()
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (1, '', 'error: output already exists\n'))
        path = self.audit / 'manifest.json'
        manifest = json.loads(path.read_text())
        manifest['packages'] = []
        path.write_text(json.dumps(manifest))
        self.failure('manifest does not match source closure', '--validate')
        (self.audit / 'link').symlink_to(self.root)
        self.failure('symlink in audit', '--validate')
