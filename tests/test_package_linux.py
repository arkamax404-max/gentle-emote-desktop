"""Public CLI fixtures: fake tool output is not native artifact evidence."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'x86_64-unknown-linux-gnu'
NAME = 'gentle-emote-desktop'
LAUNCHER = r'''
import json, os, pathlib, platform, runpy, subprocess, sys
root = pathlib.Path(sys.argv.pop(1))
scenario = sys.argv.pop(1)
script = sys.argv.pop(1)
real_run = subprocess.run
real_is_file = pathlib.Path.is_file
# A fictional system library exists only in this fake-tool fixture.
pathlib.Path.is_file = lambda path: (True if str(path) == '/usr/lib/fixture-libc.so.6'
                                     else real_is_file(path))
reads = 0
platform.system = lambda: 'Linux'
platform.machine = lambda: 'x86_64'
def fake_run(command, **kwargs):
    global reads
    args = list(map(str, command))
    tool = pathlib.Path(args[0]).name
    out, err, code = '', '', 0
    if tool == 'git':
        if args[1:3] == ['rev-parse', 'HEAD']:
            reads += 1
            out = ('b' if scenario == 'mutated' and reads > 1 else 'a') * 40 + '\n'
        elif args[1] == 'status':
            out = '?? dirty\n' if scenario == 'dirty' else ''
        elif args[1] == 'ls-files':
            out = '\0'.join(str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()) + '\0'
        else:
            raise AssertionError(args)
    elif tool == 'cargo':
        if args[1] == 'metadata':
            assert args[2:] == ['--offline','--locked','--format-version','1','--filter-platform','x86_64-unknown-linux-gnu']
            out = (root.parent / 'metadata.json').read_text()
        else:
            assert args[1:7] == ['build','--release','--offline','--locked','--target','x86_64-unknown-linux-gnu']
            if scenario == 'build-failed':
                code, err = 7, 'fixture build failed\n'
            else:
                env = kwargs['env']
                for key in ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC','RUSTC_WORKSPACE_WRAPPER'):
                    assert env.get(key) == os.environ.get(key), key
                probe = real_run([env['RUSTC_WRAPPER'], sys.executable, '-c',
                    'import json,sys; print(json.dumps(sys.argv[1:]))',
                    '--crate-name','fixture','argument with spaces'], env=env,
                    text=True, capture_output=True, check=True)
                (root.parent / 'probe.json').write_text(probe.stdout)
                build = pathlib.Path(args[args.index('--target-dir') + 1])
                dest = build / 'x86_64-unknown-linux-gnu/release/gentle-emote-desktop'
                dest.parent.mkdir(parents=True)
                data = b'\x7fELF\x02\x01\x01' + b'\0' * 9 + b'\x03\x00\x3e\x00' + b'\0' * 44
                if scenario == 'malformed': data = b'not ELF'
                if scenario == 'private-binary': data += str(root).encode()
                dest.write_bytes(data)
    elif tool == 'readelf':
        if '-lW' in args:
            out = '[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]\n'
        elif '-dW' in args:
            out = ' 0x01 (NEEDED) Shared library: [libc.so.6]\n'
            if scenario == 'rpath': out += ' 0x02 (RUNPATH) Library runpath: [/home/private/lib]\n'
        else:
            out = "Version needs section '.gnu.version_r' contains 1 entry:\n  Name: GLIBC_2.43 Flags: none Version: 2\n"
    elif tool == 'ldd':
        out = 'libc.so.6 => /usr/lib/fixture-libc.so.6 (0x123)\n/lib64/ld-linux-x86-64.so.2 (0x456)\n'
        if scenario == 'missing-lib': out = 'libc.so.6 => not found\n'
    else:
        raise AssertionError(args)
    result = subprocess.CompletedProcess(args, code, out, err)
    if kwargs.get('check'): result.check_returncode()
    return result
subprocess.run = fake_run
sys.argv[0] = script
sys.path.insert(0, str(pathlib.Path(script).parent))
runpy.run_path(script, run_name='__main__')
'''


class LinuxFixture(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='emote linux tests ')
        self.addCleanup(temp.cleanup)
        self.base = Path(temp.name)
        self.root = self.base / 'source with spaces'
        self.root.mkdir()
        for name in ('Cargo.toml', 'Cargo.lock', 'LICENSE'):
            (self.root / name).write_text('fixture project license\n')
        crate = self.root / 'vendor/fonts'
        (crate / 'fonts').mkdir(parents=True)
        (crate / 'Cargo.toml').write_text('fixture')
        (crate / 'LICENSE-MIT').write_text('MIT License\nCopyright fixture\nPermission is hereby granted, free of charge\n')
        (crate / 'fonts/OFL.txt').write_text('SIL OPEN FONT LICENSE Version 1.1\nCopyright fixture\n')
        self.crate = crate
        self.metadata = {'packages': [
            {'id': 'root', 'name': NAME, 'version': '0.1.0', 'manifest_path': str(self.root / 'Cargo.toml')},
            {'id': 'fonts', 'name': 'fonts', 'version': '1.2.3', 'license': 'MIT',
             'manifest_path': str(crate / 'Cargo.toml')}],
            'resolve': {'root': 'root', 'nodes': [{'id': 'root'}, {'id': 'fonts'}]}}
        self.save_metadata()
        self.launcher = self.base / 'launcher.py'
        self.launcher.write_text(LAUNCHER)
        self.audit = self.base / 'notices'
        self.output = self.base / 'export'
        self.env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1', RUSTC_WRAPPER='',
                        RUSTFLAGS='--cfg=with_space', CARGO_ENCODED_RUSTFLAGS='',
                        RUSTC='/compiler with spaces', RUSTC_WORKSPACE_WRAPPER='/workspace wrapper')

    def save_metadata(self):
        (self.base / 'metadata.json').write_text(json.dumps(self.metadata))

    def cli(self, script, args, scenario='clean'):
        return subprocess.run([sys.executable, '-B', str(self.launcher), str(self.root),
                               scenario, str(ROOT / 'scripts' / script), *map(str, args)],
                              cwd=self.base, env=self.env, text=True, capture_output=True, timeout=10)

    def generate(self):
        result = self.cli('audit_linux_licenses.py', ['--source-root', self.root, '--output', self.audit])
        self.assertEqual((result.returncode, result.stderr), (0, ''), result.stderr)
        return result

    def package(self, scenario='clean'):
        return self.cli('package_linux.py', ['--source-root', self.root, '--release-tag',
                        'v0.1.5-rc.3', '--license-audit', self.audit, '--output', self.output], scenario)

    def rejected(self, result, message):
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr, 'error: ' + message + '\n')
        self.assertFalse(self.output.exists())


class PackageTests(LinuxFixture):
    def test_help_and_invalid_input_have_no_side_effects(self):
        for script in ('package_linux.py', 'audit_linux_licenses.py'):
            result = self.cli(script, ['--help'])
            self.assertEqual((result.returncode, result.stderr), (0, ''))
            self.assertIn('--source-root', result.stdout)
            result = self.cli(script, ['--publish'])
            self.assertEqual(result.returncode, 2)
            self.assertEqual(result.stdout, '')
            self.assertIn('error:', result.stderr)
        self.assertFalse(self.output.exists())
        self.assertFalse(self.audit.exists())
        result = self.cli('package_linux.py', ['--release-tag', '../bad', '--license-audit',
                          self.audit, '--output', self.output])
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertIn('expected a prerelease tag', result.stderr)

    def test_dirty_mutated_and_failed_build_never_export(self):
        self.generate()
        for scenario, message in [('dirty', 'source must be clean and committed'),
                                  ('mutated', 'source changed during packaging'),
                                  ('build-failed', 'cargo build failed (exit 7)')]:
            with self.subTest(scenario=scenario):
                self.rejected(self.package(scenario), message)

    def test_output_collision_and_explicit_wrapper(self):
        self.generate()
        self.env.pop('RUSTC_WRAPPER')
        self.rejected(self.package(), 'set RUSTC_WRAPPER explicitly to a wrapper executable or empty')
        self.env['RUSTC_WRAPPER'] = ''
        self.output.mkdir()
        result = self.package()
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (1, '', 'error: output already exists\n'))
        self.assertEqual(list(self.output.iterdir()), [])

    def test_malformed_elf_private_paths_missing_libs_and_rpath(self):
        self.generate()
        for scenario, message in [('malformed', 'expected ELF64 little-endian x86_64 executable'),
                                  ('private-binary', 'private path in payload'),
                                  ('missing-lib', 'unresolved native dependency'),
                                  ('rpath', 'RPATH/RUNPATH is not permitted')]:
            with self.subTest(scenario=scenario):
                self.rejected(self.package(scenario), message)

    def test_notice_tampering_and_private_text_never_export(self):
        self.generate()
        notice = next(self.audit.glob('packages/*/LICENSE-MIT'))
        original = notice.read_bytes()
        notice.write_text('tampered')
        self.rejected(self.package(), 'notice files do not match manifest')
        notice.write_bytes(original)
        (self.root / 'LICENSE').write_text(str(self.root))
        self.rejected(self.package(), 'private path in payload')

    def test_archive_modes_hashes_provenance_and_wrapper_spaces(self):
        self.generate()
        wrapper = self.base / 'inherited wrapper'
        wrapper.write_text('#!/bin/sh\nexec "$@"\n')
        wrapper.chmod(0o755)
        self.env['RUSTC_WRAPPER'] = str(wrapper)
        result = self.package()
        self.assertEqual((result.returncode, result.stderr), (0, ''), result.stderr)
        self.assertEqual(result.stdout, 'Packaged: ' + str(self.output) + '\n')
        archive = next(self.output.glob('*.tar.gz'))
        sha = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertEqual(Path(str(archive) + '.sha256').read_text(), sha + '  ' + archive.name + '\n')
        evidence = json.loads((self.output / 'verification.json').read_text())
        self.assertEqual(evidence['source_sha'], 'a' * 40)
        self.assertEqual(evidence['rust_target'], TARGET)
        self.assertEqual(evidence['cargo_version'], '0.1.0')
        self.assertEqual(evidence['required_glibc'], '2.43')
        self.assertEqual(evidence['needed'], ['libc.so.6'])
        self.assertTrue(evidence['archive_verified'])
        self.assertFalse(evidence['runtime_verified'])
        self.assertFalse(evidence['legal_audit_complete'])
        self.assertNotIn(str(self.base), json.dumps(evidence))
        with tarfile.open(archive) as tar:
            members = tar.getmembers()
            self.assertEqual(len(members), len(evidence['files']))
            self.assertEqual({m.name for m in members}, set(evidence['files']))
            for member in members:
                self.assertTrue(member.isfile())
                self.assertFalse(member.name.startswith('/'))
                self.assertNotIn('..', Path(member.name).parts)
                data = tar.extractfile(member).read()
                record = evidence['files'][member.name]
                self.assertEqual(record, {'sha256': hashlib.sha256(data).hexdigest(),
                                          'mode': member.mode, 'size': len(data)})
                self.assertEqual(member.mode, 0o755 if member.name.endswith('/' + NAME) else 0o644)
        probe = json.loads((self.base / 'probe.json').read_text())
        self.assertEqual(probe[:3], ['--crate-name', 'fixture', 'argument with spaces'])
        self.assertIn('--remap-path-prefix=' + str(self.root) + '=/source', probe)
        self.assertTrue(any(p.endswith('=/cargo') for p in probe))


if __name__ == '__main__':
    unittest.main()
