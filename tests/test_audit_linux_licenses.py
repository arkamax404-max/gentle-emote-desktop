"""Linux source-bound offline notice CLI checks; no native build."""
import copy
import hashlib
import json
import shutil
from test_package_linux import LinuxFixture, TARGET


class AuditTests(LinuxFixture):
    def audit_cli(self, mode='--output', scenario='clean'):
        return self.cli('audit_linux_licenses.py', ['--source-root', self.root, mode, self.audit], scenario)

    def failure(self, message, mode='--output', scenario='clean'):
        def snapshot():
            return {p: p.read_bytes() for base in (self.root, self.audit)
                    for p in base.rglob('*') if p.is_file() and '.git' not in p.parts}
        before = snapshot()
        result = self.audit_cli(mode, scenario)
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (1, '', 'error: ' + message + '\n'))
        self.assertEqual(before, snapshot())
        if mode == '--output':
            self.assertFalse(self.audit.exists())

    def test_exact_linux_closure_fonts_and_tampering(self):
        self.generate()
        manifest = json.loads((self.audit / 'manifest.json').read_text())
        self.assertEqual(manifest['targets'], [TARGET])
        self.assertEqual(manifest['source_sha'], self.sha)
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
        self.snapshot()
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
                label = 'configured' if configured else 'default'
                self.registry_args = []
                self.audit = self.base / (label + '-omitted')
                with self.subTest(authorization='omitted'):
                    self.failure('dependency source outside authorized roots')
                self.audit = self.base / (label + '-audit')
                authorized = ['--registry-source-root', str(cargo_home / 'registry/src')]
                self.registry_args = authorized
                generated = self.generate()
                result = self.audit_cli('--validate')
                self.assertEqual((result.returncode, result.stdout, result.stderr), (0, generated.stdout, ''))
                self.assertTrue(json.loads(result.stdout)['notice_inventory_complete'])
                self.output = self.base / (label + '-rejected-export')
                wrong = ['--registry-source-root', str(home / 'different cargo/registry/src')]
                for args in ([], wrong):
                    with self.subTest(authorization=args):
                        self.registry_args = args
                        self.failure('dependency source outside authorized roots', '--validate')
                        self.rejected(self.package(), 'dependency source outside authorized roots')
                self.registry_args = authorized
                self.output = self.base / (label + '-export')
                result = self.package()
                self.assertEqual((result.returncode, result.stdout, result.stderr),
                                 (0, 'Packaged: ' + str(self.output) + '\n', ''))
                alias = home / 'linked cargo'
                alias.symlink_to(cargo_home, target_is_directory=True)
                self.registry_args = ['--registry-source-root', str(alias / 'registry/src')]
                self.metadata['packages'][1]['manifest_path'] = str(
                    alias / 'registry/src/fixture-index/fonts-1.2.3/Cargo.toml')
                self.save_metadata()
                self.failure('symlink in dependency source', '--validate')
                self.output = self.base / (label + '-symlink-export')
                self.rejected(self.package(), 'symlink in dependency source')

    def supplement_fixture(self, release=False, *, crate_root=None):
        crate = (crate_root or self.root) / 'supplement-crate'
        crate.mkdir()
        (crate / 'Cargo.toml').write_text('fixture')
        (crate / '.cargo_vcs_info.json').write_text(json.dumps({
            'git': {'sha1': 'a' * 40}, 'path_in_vcs': 'crates/fonts'}))
        self.metadata['packages'][1].update(manifest_path=str(crate / 'Cargo.toml'),
                                            repository='https://github.com/example/fonts')
        self.save_metadata()
        self.supplements = self.root / 'scripts/linux-license-supplements'
        (self.supplements / 'files').mkdir(parents=True)
        data = (self.crate / 'LICENSE-MIT').read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        (self.supplements / 'files' / sha).write_bytes(data)
        self.entry = {'name': 'fonts', 'version': '1.2.3', 'license_expression': 'MIT',
                      'repository': 'https://github.com/example/fonts',
                      'crate_revision': 'a' * 40, 'path_in_vcs': 'crates/fonts',
                      'notice_revision': ('b' if release else 'a') * 40,
                      'provenance_kind': ('same-version-upstream-release' if release
                                          else 'exact-crate-revision'),
                      'files': [{'path': 'files/' + sha, 'sha256': sha,
                                 'notice_path': 'LICENSE-MIT', 'role': 'license',
                                 'upstream_url': 'https://raw.githubusercontent.com/example/fonts/' +
                                 ('b' if release else 'a') * 40 + '/LICENSE-MIT'}]}
        if release:
            self.entry['release_tag'] = '1.2.3'
        self.save_supplement()
        return crate

    def save_supplement(self):
        (self.supplements / 'manifest.json').write_text(json.dumps(
            {'schema_version': 1, 'packages': [self.entry]}))
        self.snapshot()

    def test_exact_and_same_version_supplements_retain_provenance(self):
        self.supplement_fixture(release=True)
        self.generate()
        manifest = json.loads((self.audit / 'manifest.json').read_text())
        self.assertEqual(manifest['packages'][0]['notice_provenance'], self.entry)
        result = self.audit_cli('--validate')
        self.assertEqual((result.returncode, result.stderr), (0, ''))
        self.assertFalse(json.loads(result.stdout)['legal_audit_complete'])
        self.entry.update(provenance_kind='exact-crate-revision')
        self.save_supplement()
        self.failure('supplement provenance mismatch: fonts@1.2.3', '--validate')

    def test_supplements_are_scoped_to_the_exact_resolved_version(self):
        other = dict(self.metadata['packages'][1], id='fonts-old', version='1.2.2')
        self.supplement_fixture()
        project, pinned = self.metadata['packages']
        for both in (False, True):
            with self.subTest(both=both):
                packages = [pinned, other] if both else [other]
                self.metadata['packages'] = [project, *packages]
                self.metadata['resolve']['nodes'] = [{'id': p['id']} for p in [project, *packages]]
                self.save_metadata()
                self.audit = self.base / f'audit-{both}'
                result = self.audit_cli()
                self.assertEqual((result.returncode, result.stderr, self.audit.exists()), (0, '', True),
                                 repr(result.stdout))
                manifest = (self.audit / 'manifest.json').read_bytes()
                records = {p['version']: p for p in json.loads(manifest)['packages']}
                self.assertEqual(set(records), {p['version'] for p in packages})
                original = records['1.2.2']
                self.assertEqual(original['notice_provenance'], 'original files from resolved Cargo package')
                prefix = 'packages/fonts@1.2.2/'
                self.assertEqual([f['path'] for f in original['files']],
                                 [prefix + 'LICENSE-MIT', prefix + 'fonts/OFL.txt'])
                self.assertFalse((self.audit / prefix / 'supplemental').exists())
                if both:
                    self.assertEqual(records['1.2.3']['notice_provenance'], self.entry)
                    self.assertTrue((self.audit / 'packages/fonts@1.2.3/supplemental/LICENSE-MIT').is_file())
                stdout = json.dumps(dict(manifest_sha256=hashlib.sha256(manifest).hexdigest(),
                                         package_count=len(packages), file_count=2 + both,
                                         notice_inventory_complete=True, legal_audit_complete=False,
                                         license_exceptions=[]), sort_keys=True) + '\n'
                self.assertEqual(result.stdout, stdout)
                result = self.audit_cli('--validate')
                self.assertEqual((result.returncode, result.stdout, result.stderr), (0, stdout, ''))
        result = self.package()
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (0, f'Packaged: {self.output}\n', ''))
        self.assertTrue((self.output / 'verification.json').is_file())

    def test_supplement_binding_missing_tampered_paths_and_urls_fail_closed(self):
        registry = self.base / 'authorized registry'
        registry.mkdir()
        self.registry_args = ['--registry-source-root', str(registry)]
        crate = self.supplement_fixture(crate_root=registry)
        original = copy.deepcopy(self.entry)
        mutations = [lambda e: e.update(crate_revision='c' * 40),
                     lambda e: e.update(path_in_vcs='other'),
                     lambda e: e.update(repository='https://github.com/example/other'),
                     lambda e: e.update(license_expression='Apache-2.0'),
                     lambda e: e.update(notice_revision='b' * 40),
                     lambda e: e['files'][0].update(notice_path='../LICENSE'),
                     lambda e: e['files'][0].update(upstream_url='https://example.invalid/LICENSE'),
                     lambda e: e['files'][0].update(path='../escape'),
                     lambda e: e.update(files=[])]
        for mutate in mutations:
            with self.subTest(mutation=mutations.index(mutate)):
                self.entry = copy.deepcopy(original)
                mutate(self.entry)
                self.save_supplement()
                self.failure('supplement provenance mismatch: fonts@1.2.3')
        self.entry = copy.deepcopy(original)
        self.entry['files'][0].update(path='files/' + '0' * 64, sha256='0' * 64)
        self.save_supplement()
        self.failure('missing supplement file: fonts@1.2.3')
        self.entry = copy.deepcopy(original)
        self.save_supplement()
        payload = self.supplements / self.entry['files'][0]['path']
        original_bytes = payload.read_bytes()
        payload.write_text('tampered')
        self.snapshot()
        self.failure('supplement hash mismatch: fonts@1.2.3')
        payload.write_bytes(original_bytes)
        self.snapshot()
        (crate / '.cargo_vcs_info.json').unlink()
        self.failure('supplement provenance mismatch: fonts@1.2.3')

    def test_nested_font_witnesses_cannot_supply_crate_mit(self):
        (self.crate / 'fonts/UFL.txt').write_text('UBUNTU FONT LICENCE Version 1.0\n')
        self.metadata['packages'][1]['license'] = 'MIT AND OFL-1.1 AND Ubuntu-font-1.0'
        self.save_metadata()
        self.snapshot()
        self.generate()
        result = self.audit_cli('--validate')
        self.assertEqual((result.returncode, result.stderr), (0, ''))
        empty = self.root / 'font-only'
        (empty / 'fonts').mkdir(parents=True)
        (empty / 'fonts/LICENSE-MIT').write_bytes((self.crate / 'LICENSE-MIT').read_bytes())
        self.metadata['packages'][1].update(manifest_path=str(empty / 'Cargo.toml'), license='MIT')
        self.save_metadata()
        self.audit = self.base / 'font-only-audit'
        self.snapshot()
        self.failure('no original license notices: fonts@1.2.3 (MIT)')

    def test_checked_in_linux_supplements_and_source_attributions(self):
        from test_package_linux import ROOT
        checked = ROOT / 'scripts/linux-license-supplements'
        manifest = json.loads((checked / 'manifest.json').read_bytes())
        shutil.copytree(checked, self.root / 'scripts/linux-license-supplements')
        self.metadata['packages'] = self.metadata['packages'][:1]
        self.metadata['resolve']['nodes'] = [{'id': 'root'}]
        registry = self.base / 'authorized registry'
        self.registry_args = ['--registry-source-root', str(registry)]
        for entry in manifest['packages']:
            key = entry['name'] + '@' + entry['version']
            # Exercise VCS and source-prefix reads for an explicitly authorized registry crate.
            crate = (registry if entry['name'] == 'gl_generator' else self.root / 'vendor') / key
            crate.mkdir(parents=True)
            (crate / 'Cargo.toml').write_text('fixture')
            (crate / '.cargo_vcs_info.json').write_text(json.dumps({
                'git': {'sha1': entry['crate_revision']}, 'path_in_vcs': entry['path_in_vcs']}))
            for record in entry['files']:
                data = (checked / record['path']).read_bytes()
                self.assertEqual(hashlib.sha256(data).hexdigest(), record['sha256'])
                if record['role'] == 'source-attribution':
                    dest = crate / record['crate_path']
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_bytes(data + b'\nfixture source follows\n')
            if entry['name'] == 'epaint_default_fonts':
                (crate / 'fonts').mkdir()
                (crate / 'fonts/OFL.txt').write_text('SIL OPEN FONT LICENSE Version 1.1\n')
                (crate / 'fonts/UFL.txt').write_text('UBUNTU FONT LICENCE Version 1.0\n')
            self.metadata['packages'].append({'id': key, 'name': entry['name'],
                'version': entry['version'], 'license': entry['license_expression'],
                'repository': entry['repository'], 'manifest_path': str(crate / 'Cargo.toml')})
            self.metadata['resolve']['nodes'].append({'id': key})
        self.save_metadata()
        self.snapshot()
        self.generate()
        result = self.audit_cli('--validate')
        self.assertEqual((result.returncode, result.stderr), (0, ''))
        self.assertFalse(json.loads(result.stdout)['legal_audit_complete'])
        generated = json.loads((self.audit / 'manifest.json').read_bytes())
        self.assertEqual(len(generated['packages']), 14)
        self.assertEqual([p['notice_provenance'] for p in generated['packages']],
                         sorted(manifest['packages'], key=lambda e: e['name'] + '@' + e['version']))
        accesskit = next(e for e in manifest['packages'] if e['name'] == 'accesskit')
        self.assertTrue({'AUTHORS', 'LICENSE.chromium'} <=
                        {r['notice_path'] for r in accesskit['files']})
        header_entry = next(e for e in manifest['packages'] if e['name'] == 'gl_generator')
        header = next(r for r in header_entry['files'] if r['role'] == 'source-attribution')
        (registry / 'gl_generator@0.14.0' / header['crate_path']).write_text('changed')
        self.failure('supplement source attribution mismatch: gl_generator@0.14.0', '--validate')

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
