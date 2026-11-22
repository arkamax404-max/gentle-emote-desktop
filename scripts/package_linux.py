#!/usr/bin/env python3
"""Build a clean-source Linux x86_64 GNU prerelease tar.gz; never publish or launch."""
import argparse
from contextlib import contextmanager
import io
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import struct
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True
from audit_linux_licenses import (ROOT, TARGET, audit_files, command, digest, encode,
                                 frozen, new_output, source_state, validate_audit)

NAME = 'gentle-emote-desktop'


def release_tag(value):
    if not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*', value):
        raise argparse.ArgumentTypeError('expected a prerelease tag like v0.1.5-rc.3')
    return value


@contextmanager
def build_environment(root, build):
    inherited = os.environ['RUSTC_WRAPPER']
    with tempfile.TemporaryDirectory(prefix='emote-linux-rustc-') as temp:
        remaps = []
        for path, replacement in [(Path.home(), '/toolchain'),
                                  (Path(tempfile.gettempdir()), '/build'),
                                  (Path(os.environ.get('CARGO_HOME') or Path.home() / '.cargo'), '/cargo'),
                                  (root, '/source'), (build, '/build'), (Path(temp), '/build')]:
            for absolute in dict.fromkeys([str(path.absolute()), str(path.resolve())]):
                remaps.append('--remap-path-prefix=' + absolute + '=' + replacement)
        driver = Path(temp) / 'driver.py'
        driver.write_text('import os,sys\ncommand = sys.argv[1:]\n'
                          "if '--crate-name' in command:\n    command += " + repr(remaps) + '\n'
                          'inherited = ' + repr(inherited) + '\n'
                          'if inherited:\n    command.insert(0, inherited)\n'
                          'os.execvpe(command[0], command, os.environ)\n')
        wrapper = Path(temp) / 'wrapper'
        wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' ' +
                           shlex.quote(str(driver)) + ' "$@"\n')
        wrapper.chmod(0o755)
        env = os.environ.copy()
        # Do not shadow Cargo's flag precedence or compiler/workspace wrapper.
        env['RUSTC_WRAPPER'] = str(wrapper)
        yield env, Path(temp)


def private_paths(data, roots):
    prefixes = [str(path.absolute()).encode() for path in roots]
    prefixes += [b'/home/', b'/Users/', b'/root/', b'/private/var/']
    if any(prefix and prefix in data for prefix in prefixes):
        raise RuntimeError('private path in payload')


def inspect_elf(binary, root):
    data = binary.read_bytes()
    if (len(data) < 64 or data[:7] != b'\x7fELF\x02\x01\x01' or
            struct.unpack_from('<H', data, 16)[0] not in (2, 3) or
            struct.unpack_from('<H', data, 18)[0] != 62):
        raise RuntimeError('expected ELF64 little-endian x86_64 executable')
    program = command(['readelf', '-lW', binary], root, 'ELF program inspection failed')
    interpreters = re.findall(r'\[Requesting program interpreter: ([^\]]+)\]', program)
    if interpreters != ['/lib64/ld-linux-x86-64.so.2']:
        raise RuntimeError('unexpected ELF interpreter')
    dynamic = command(['readelf', '-dW', binary], root, 'ELF dependency inspection failed')
    if re.search(r'\((?:RPATH|RUNPATH)\)', dynamic):
        raise RuntimeError('RPATH/RUNPATH is not permitted')
    needed = re.findall(r'\(NEEDED\).*?\[([^\]]+)\]', dynamic)
    if not needed or len(needed) != len(set(needed)) or any(
            not re.fullmatch(r'[A-Za-z0-9_+.-]+', n) for n in needed):
        raise RuntimeError('invalid ELF NEEDED inventory')
    versions = command(['readelf', '-VW', binary], root, 'ELF version inspection failed')
    # Use only version NEEDS, never definitions or incidental strings.
    needs_section = versions.split('Version needs section', 1)
    if len(needs_section) != 2 or 'GLIBC_PRIVATE' in needs_section[1]:
        raise RuntimeError('invalid GLIBC version needs')
    glibc = re.findall(r'Name: GLIBC_([0-9]+(?:\.[0-9]+)+)\b', needs_section[1])
    if not glibc:
        raise RuntimeError('missing GLIBC version needs')
    required = max(glibc, key=lambda v: tuple(map(int, v.split('.'))))
    # Only inspect this invocation's compiler output with the approved loader.
    resolved = command(['ldd', binary], root, 'native dependency resolution failed')
    if 'not found' in resolved:
        raise RuntimeError('unresolved native dependency')
    libraries = set()
    for line in resolved.splitlines():
        line = line.strip()
        if not line:
            continue
        match = re.fullmatch(r'([A-Za-z0-9_+.-]+) => (/(?:usr/)?lib(?:64)?/[^ ]+) \(0x[0-9a-fA-F]+\)', line)
        if match:
            resolved_path = Path(match[2])
            system_roots = [Path(p).resolve() for p in ('/lib', '/lib64', '/usr/lib', '/usr/lib64')]
            if ('..' in resolved_path.parts or not resolved_path.is_file() or
                    not any(base == resolved_path.resolve() or base in resolved_path.resolve().parents
                            for base in system_roots)):
                raise RuntimeError('unresolved or private native dependency')
            libraries.add(match[1])
        elif re.fullmatch(r'/(?:usr/)?lib(?:64)?/[^ ]+ \(0x[0-9a-fA-F]+\)', line):
            continue
        elif re.fullmatch(r'linux-vdso\.so\.1 \(0x[0-9a-fA-F]+\)', line):
            continue
        else:
            raise RuntimeError('unrecognized or private native dependency resolution')
    if not set(needed) <= libraries:
        raise RuntimeError('unresolved native dependency')
    return {'file': 'ELF64 little-endian x86_64', 'interpreter': interpreters[0],
            'needed': sorted(needed), 'resolved_libraries': sorted(libraries),
            'version_needs': sorted(set(re.findall(r'Name: ([A-Za-z0-9_.+-]+)', needs_section[1]))),
            'required_glibc': required}


def verify_archive(archive, expected):
    with tarfile.open(archive, 'r:gz') as tar:
        seen = set()
        for member in tar.getmembers():
            name = member.name
            if (name in seen or name not in expected or not member.isfile() or
                    name.startswith('/') or any(p in ('', '.', '..') for p in name.split('/'))):
                raise RuntimeError('unsafe archive member')
            seen.add(name)
            record = expected[name]
            if (member.mode != record['mode'] or member.size != record['size'] or
                    digest(tar.extractfile(member).read()) != record['sha256'] or
                    member.uid != 0 or member.gid != 0):
                raise RuntimeError('archive payload mismatch')
        if seen != set(expected):
            raise RuntimeError('archive inventory mismatch')


def package(args):
    if 'RUSTC_WRAPPER' not in os.environ:
        raise RuntimeError('set RUSTC_WRAPPER explicitly to a wrapper executable or empty')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise RuntimeError('packaging requires native Linux x86_64 GNU')
    root = args.source_root.resolve(strict=True)
    output = args.output.absolute()
    new_output(output, root)
    state = source_state(root)
    try:
        audit = validate_audit(args.license_audit, root)
    finally:
        frozen(state, root)
    manifest = json.loads((args.license_audit / 'manifest.json').read_bytes())
    stem = NAME + '-' + args.release_tag + '-linux-x86_64-' + state[0][:12]
    cargo = shutil.which('cargo')
    if not cargo:
        raise RuntimeError('cargo not found on PATH')
    with tempfile.TemporaryDirectory(prefix='emote-linux-package-') as temp:
        scratch = Path(temp)
        build = scratch / 'build'
        with build_environment(root, build) as (env, wrapper_temp):
            command([cargo, 'build', '--release', '--offline', '--locked', '--target', TARGET,
                     '--target-dir', build], root, 'cargo build failed', env)
            binary = build / TARGET / 'release' / NAME
            if binary.is_symlink() or not binary.is_file():
                raise RuntimeError('build did not produce a regular executable')
            observation = inspect_elf(binary, root)
            copied = scratch / 'notices'
            shutil.copytree(args.license_audit, copied)
            if validate_audit(copied, root) != audit:
                raise RuntimeError('copied license audit differs from input')
            payload = {NAME: binary.read_bytes(), 'LICENSE': (root / 'LICENSE').read_bytes()}
            payload.update({'ThirdPartyNotices/' + name: data for name, data in audit_files(copied).items()})
            receipt = {'schema_version': 1, 'source_sha': state[0], 'source_dirty': False,
                       'release_tag': args.release_tag, 'cargo_version': manifest['cargo_version'],
                       'package_version': args.release_tag[1:].split('-')[0], 'rust_target': TARGET,
                       'cargo_sha256': manifest['cargo_sha256'], 'license_audit': audit,
                       'notice_inventory_complete': True, 'legal_audit_complete': False,
                       'runtime_verified': False, **observation}
            payload['packaging-receipt.json'] = encode(receipt)
            payload['README.txt'] = (
                'Gentle Emote Desktop Linux x86_64 GNU prerelease\n'
                'Source: ' + state[0] + '\n'
                'Run: ./' + NAME + '\n'
                'Requires a graphical session (Wayland or X11), working OpenGL drivers,\n'
                'the recorded native libraries, and glibc >= ' + observation['required_glibc'] + '.\n'
                'This observed glibc requirement is not a broad distribution compatibility claim.\n'
                'Native NEEDED: ' + ', '.join(observation['needed']) + '\n'
                'No runtime smoke was performed by the packager. No installation or launch is automatic.\n'
                'The GUI listens on 127.0.0.1:8001; close other instances first.\n'
                'There is no CLI health/help mode: flags still launch the GUI.\n'
                'LICENSE covers this project; ThirdPartyNotices retains dependency/font notices.\n'
                'Notice inventory completeness is not a complete legal-license audit.\n'
            ).encode()
            private_roots = [root, scratch, wrapper_temp, Path.home(),
                             Path(os.environ.get('CARGO_HOME') or Path.home() / '.cargo')]
            files = {}
            archive = scratch / (stem + '.tar.gz')
            with tarfile.open(archive, 'w:gz', format=tarfile.USTAR_FORMAT) as tar:
                for relative, data in sorted(payload.items()):
                    private_paths(data, private_roots)
                    name = stem + '/' + relative
                    mode = 0o755 if relative == NAME else 0o644
                    files[name] = {'sha256': digest(data), 'mode': mode, 'size': len(data)}
                    info = tarfile.TarInfo(name)
                    info.mode, info.size, info.mtime = mode, len(data), 0
                    tar.addfile(info, io.BytesIO(data))
            verify_archive(archive, files)
            verification = {**receipt, 'files': files, 'archive_verified': True,
                            'archive_sha256': digest(archive.read_bytes()), 'archive': archive.name,
                            'validation_scope': 'static inspection and exact archive readback; no runtime smoke'}
            private_paths(encode(verification), private_roots)
            # Prepare everything before export; no partial release on validation
            # or build rejection. copytree exclusively reserves the destination.
            export = scratch / 'export'
            export.mkdir()
            shutil.copyfile(archive, export / archive.name)
            (export / (archive.name + '.sha256')).write_text(
                verification['archive_sha256'] + '  ' + archive.name + '\n')
            (export / 'verification.json').write_bytes(encode(verification))
            for path in export.iterdir():
                path.chmod(0o644)
            frozen(state, root)
            shutil.copytree(export, output)
            frozen(state, root)
    print('Packaged: ' + str(output))


def main():
    parser = argparse.ArgumentParser(description=__doc__ +
        ' Requires locked offline Cargo, readelf, ldd, original Linux notices and explicitly '
        'selected RUSTC_WRAPPER (empty selects none). Compiler paths are remapped without '
        'replacing Cargo flags. Outputs: archive, SHA256, external verification.json. '
        'Static inspection is not runtime or legal clearance.')
    parser.add_argument('--source-root', type=Path, default=ROOT, help='clean committed checkout')
    parser.add_argument('--release-tag', type=release_tag, required=True, help='label only; never creates a tag')
    parser.add_argument('--license-audit', type=Path, required=True, help='Linux audit directory to revalidate')
    parser.add_argument('--output', type=Path, required=True, help='NEW directory outside source; parent must exist')
    args = parser.parse_args()
    try:
        package(args)
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, IndexError) as error:
        print('error: ' + (str(error) if isinstance(error, RuntimeError) else 'packaging failed'), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
