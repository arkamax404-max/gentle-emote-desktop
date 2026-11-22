#!/usr/bin/env python3
"""Inventory original Linux dependency notices offline; not legal advice."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

# Imported helpers are pure; Darwin inventories, supplements and exceptions are
# deliberately not used. Avoid generating caches in a clean source checkout.
sys.dont_write_bytecode = True
from audit_macos_licenses import digest, select_license, audit_files, summary, beneath

TARGET = 'x86_64-unknown-linux-gnu'
REGISTRY = (Path(os.environ.get('CARGO_HOME') or Path.home() / '.cargo') /
            'registry/src').absolute()
ROOT = Path(__file__).resolve().parents[1]


def command(args, root, error, env=None):
    result = subprocess.run(list(map(str, args)), cwd=root, env=env,
                            capture_output=True, text=True)
    if result.returncode:
        # Child diagnostics can contain private paths; never persist them.
        raise RuntimeError(error + ' (exit ' + str(result.returncode) + ')')
    return result.stdout


def source_file(path, root):
    path = Path(os.path.abspath(path))
    if not any(beneath(path, base) for base in (root, REGISTRY)):
        raise RuntimeError('dependency source outside authorized roots')
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise RuntimeError('symlink in dependency source')
    return path


def source_state(root):
    sha = command(['git', 'rev-parse', 'HEAD'], root, 'source commit unavailable').strip()
    dirty = command(['git', 'status', '--porcelain', '--untracked-files=all'], root,
                    'source status unavailable')
    if dirty or not re.fullmatch('[0-9a-f]{40,64}', sha):
        raise RuntimeError('source must be clean and committed')
    tracked = command(['git', 'ls-files', '-z'], root, 'source inventory unavailable').split('\0')
    hashes = {}
    for name in filter(None, tracked):
        if Path(name).is_absolute() or '..' in Path(name).parts:
            raise RuntimeError('invalid tracked source path')
        path = source_file(root / name, root)
        if not path.is_file():
            raise RuntimeError('non-regular tracked source')
        hashes[name] = [digest(path.read_bytes()), path.stat().st_mode & 0o777]
    if not hashes:
        raise RuntimeError('source must be clean and committed')
    return sha, hashes


def frozen(expected, root):
    if source_state(root) != expected:
        raise RuntimeError('source changed during packaging')


def new_output(output, root):
    if output.exists() or output.is_symlink():
        raise RuntimeError('output already exists')
    if not output.parent.is_dir() or beneath(output.resolve(), root):
        raise RuntimeError('output must have an existing parent outside source')


# Conservative text witnesses, not a legal determination. A link-only LICENSE,
# unrelated font notice or generic Cargo SPDX declaration cannot close a grant.
WITNESSES = {
    'MIT': ('permission is hereby granted, free of charge',),
    'Apache-2.0': ('apache license', 'version 2.0', 'terms and conditions'),
    'Apache-2.0 WITH LLVM-exception': ('apache license', 'llvm exception'),
    'BSD-2-Clause': ('redistribution and use in source and binary forms', 'disclaimer'),
    'BSD-3-Clause': ('redistribution and use in source and binary forms', 'neither the name'),
    'ISC': ('permission to use, copy, modify', 'for any purpose'),
    'Zlib': ('this software is provided', 'altered source versions'),
    'Unlicense': ('this is free and unencumbered software', 'public domain'),
    '0BSD': ('permission to use, copy, modify', 'for any purpose'),
    'CC0-1.0': ('creative commons', 'waiver', 'public domain'),
    'BSL-1.0': ('boost software license', 'version 1.0'),
    'OFL-1.1': ('sil open font license', '1.1'),
    'Ubuntu-font-1.0': ('ubuntu font licence', '1.0'),
    'Unicode-3.0': ('unicode', 'permission', 'copyright'),
    'Unicode-DFS-2016': ('unicode', 'permission', 'copyright'),
}


def notices(crate, root):
    payload = {}
    def walk_error(error):
        raise RuntimeError('cannot inventory dependency directory') from error
    for directory, dirs, files in os.walk(crate, followlinks=False, onerror=walk_error):
        for name in sorted(dirs + files):
            path = source_file(Path(directory) / name, root)
            if path.is_dir():
                continue
            if not path.is_file():
                raise RuntimeError('non-regular dependency file')
            relative = path.relative_to(crate)
            conventional = any(re.match(
                r'^(licen[cs]es?|copying|copyright|notices?|unlicense|ofl|ufl)([._-]|$)',
                part, re.I) for part in relative.parts)
            text = path.suffix.lower() in ('.txt', '.md')
            if conventional or text:
                data = path.read_bytes()
                if conventional or ('fonts' in relative.parts) or b'copyright' in data.lower():
                    if not data.strip():
                        raise RuntimeError('empty notice file')
                    payload[relative.as_posix()] = data
    return payload


def inventory(root):
    state = source_state(root)
    cargo = shutil.which('cargo')
    if not cargo:
        raise RuntimeError('cargo not found on PATH')
    metadata = json.loads(command([cargo, 'metadata', '--offline', '--locked', '--format-version',
                                   '1', '--filter-platform', TARGET], root, 'cargo metadata failed'))
    available = {p['id']: p for p in metadata['packages']}
    nodes = [n['id'] for n in metadata['resolve']['nodes']]
    root_id = metadata['resolve']['root']
    if (not nodes or len(nodes) != len(set(nodes)) or root_id not in nodes or
            Path(available[root_id]['manifest_path']).absolute() != root / 'Cargo.toml'):
        raise RuntimeError('invalid Cargo closure')
    packages, payload, gaps = {}, {}, []
    for identity in nodes:
        if identity == root_id:
            continue
        package = available[identity]
        key = package['name'] + '@' + package['version']
        if not re.fullmatch(r'[A-Za-z0-9_-]+@[A-Za-z0-9.+-]+', key) or key in packages:
            raise RuntimeError('ambiguous package identity')
        try:
            selected = select_license(package['license'])
        except (ValueError, IndexError):
            raise RuntimeError('unsupported or missing SPDX license: ' + key) from None
        crate = source_file(Path(package['manifest_path']).parent, root)
        files = notices(crate, root)
        # Only original conventional license payloads can witness the selected
        # grant; nested font attribution remains included but cannot replace it.
        license_texts = [re.sub(r'\s+', ' ', data.decode(errors='replace').lower())
                         for name, data in files.items()
                         if len(Path(name).parts) == 1 and re.match(
                             r'^(licen[cs]e|copying|unlicense|copyright)', name, re.I)]
        for license_id in selected:
            if not any(all(marker in text for marker in WITNESSES[license_id])
                       for text in license_texts):
                gaps.append(key + ' (' + license_id + ')')
        records = []
        for name, data in sorted(files.items()):
            destination = 'packages/' + key + '/' + name
            records.append({'path': destination, 'sha256': digest(data)})
            payload[destination] = data
        packages[key] = {'name': package['name'], 'version': package['version'],
                         'license': package['license'], 'selected_licenses': selected,
                         'targets': [TARGET], 'files': records,
                         'notice_provenance': 'original files from resolved Cargo package'}
    if gaps:
        raise RuntimeError('no original license notices: ' + '; '.join(sorted(gaps)))
    frozen(state, root)
    return {'schema_version': 1, 'source_sha': state[0], 'targets': [TARGET],
            'cargo_version': available[root_id]['version'],
            'cargo_sha256': {n: digest(source_file(root / n, root).read_bytes())
                             for n in ('Cargo.toml', 'Cargo.lock')},
            'packages': [packages[k] for k in sorted(packages)]}, payload


def encode(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode()


def validate_audit(audit_dir, root):
    files = audit_files(Path(audit_dir).absolute())
    manifest_bytes = files.pop('manifest.json')
    manifest = json.loads(manifest_bytes)
    expected, payload = inventory(root)
    if manifest != expected:
        raise RuntimeError('manifest does not match source closure')
    if files != payload:
        raise RuntimeError('notice files do not match manifest')
    return summary(manifest_bytes, manifest, payload)


def generate(output, root):
    new_output(output, root)
    state = source_state(root)
    manifest, payload = inventory(root)
    with tempfile.TemporaryDirectory(prefix='emote-linux-notices-') as temp:
        stage = Path(temp) / 'notices'
        stage.mkdir()
        for relative, data in {**payload, 'manifest.json': encode(manifest)}.items():
            dest = stage / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        result = validate_audit(stage, root)
        frozen(state, root)
        shutil.copytree(stage, output)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__ +
        ' Uses locked offline Linux platform metadata and SHA256; missing original notices fail closed. '
        'No Darwin exceptions or generic SPDX substitutes are approved for Linux.')
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--output', type=Path, help='NEW directory outside source; existing parent required')
    mode.add_argument('--validate', type=Path, help='rederive exact source closure and notice bytes')
    parser.add_argument('--source-root', type=Path, default=ROOT, help='clean committed checkout')
    args = parser.parse_args()
    try:
        root = args.source_root.resolve(strict=True)
        result = (generate(args.output.absolute(), root) if args.output is not None
                  else validate_audit(args.validate, root))
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, IndexError) as error:
        print('error: ' + (str(error) if isinstance(error, RuntimeError) else 'invalid audit'), file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
