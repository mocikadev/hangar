#!/usr/bin/env python3
"""Verify a deb candidate in a temporary root. Never install to the host system."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FILES = {
    'usr/bin/hangar-desktop',
    'usr/share/applications/dev.mocika.hangar.desktop',
    'usr/share/icons/hicolor/scalable/apps/dev.mocika.hangar.svg',
    'usr/share/doc/hangar-desktop/README.md',
    'usr/share/doc/hangar-desktop/copyright',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def output(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def checksums(directory, manifest):
    require(manifest['schema_version'] == 2, 'Expected manifest schema 2')
    require(len(manifest['artifacts']) == 1, 'Expected exactly one deb artifact')
    artifact = manifest['artifacts'][0]
    name = artifact['path']
    require(Path(name).name == name and name.endswith('.deb'), 'Invalid artifact path')
    expected = {name, 'manifest.json'}
    actual = {}
    for line in (directory / 'SHA256SUMS.txt').read_text().splitlines():
        checksum, path = line.split('  ', 1)
        require(path in expected and path not in actual, 'Invalid/duplicate checksum entry')
        actual[path] = checksum
    require(set(actual) == expected, 'Incomplete checksum coverage')
    for path, checksum in actual.items():
        require(digest(directory / path) == checksum, f'Checksum mismatch: {path}')
    require(artifact['sha256'] == actual[name], 'Manifest checksum mismatch')
    require((directory / name).stat().st_size == artifact['bytes'], 'Artifact size mismatch')
    return directory / name


def contents(deb):
    payload = subprocess.check_output(['dpkg-deb', '--fsys-tarfile', str(deb)])
    with tarfile.open(fileobj=io.BytesIO(payload)) as tar:
        files = set()
        for member in tar:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and '..' not in path.parts, 'Unsafe archive path')
            require(member.isdir() or member.isfile(), 'Unexpected link or special file')
            require(member.uid == 0 and member.gid == 0, 'Package ownership must be root')
            if member.isfile():
                name = str(path)
                files.add(name)
                require(member.mode == (0o755 if name == 'usr/bin/hangar-desktop' else 0o644),
                        f'Unexpected permissions: {name}')
        require(files == FILES, f'Unexpected payload: {files.symmetric_difference(FILES)}')
    control = subprocess.check_output(['dpkg-deb', '--ctrl-tarfile', str(deb)])
    with tarfile.open(fileobj=io.BytesIO(control)) as tar:
        require({str(PurePosixPath(m.name)) for m in tar if m.isfile()} == {'control'},
                'Unexpected maintainer scripts/control files')


def verify(directory, smoke):
    manifest = json.loads((directory / 'manifest.json').read_text())
    deb = checksums(directory, manifest)
    contents(deb)
    version = manifest['version']
    for field, expected in [('Package', 'hangar-desktop'), ('Version', f'{version}-1'),
                            ('Architecture', {'x64': 'amd64', 'arm64': 'arm64'}[manifest['arch']]),
                            ('Depends', manifest['debian_depends'])]:
        require(output('dpkg-deb', '-f', str(deb), field) == expected, f'{field} mismatch')
    with tempfile.TemporaryDirectory(prefix='hangar-deb-check-') as temporary:
        work = Path(temporary)
        home = work / 'home'
        home.mkdir(mode=0o700)
        env = {**os.environ, 'HOME': str(home), 'CODEX_HOME': str(home / '.codex'),
               'HANGAR_TEST_HOME': str(home), 'HANGAR_TEST_TOKEN_ENDPOINT': 'http://127.0.0.1:9/token',
               'HANGAR_TEST_USAGE_ENDPOINT': 'http://127.0.0.1:9/usage'}
        extracted = work / 'extracted'
        subprocess.run(['dpkg-deb', '-x', str(deb), str(extracted)], check=True, env=env)
        binary = extracted / 'usr/bin/hangar-desktop'
        require(output(str(binary), '--version', env=env) == f'hangar-desktop {version}', 'Binary version mismatch')
        require('not found' not in output('ldd', str(binary), env={**env, 'LC_ALL': 'C'}), 'Missing runtime library')
        desktop = extracted / 'usr/share/applications/dev.mocika.hangar.desktop'
        subprocess.run(['desktop-file-validate', str(desktop)], check=True)
        require('Exec=hangar-desktop\n' in desktop.read_text(), 'Unexpected launcher command')
        require('GTK_THEME' not in desktop.read_text() and 'GDK_BACKEND' not in desktop.read_text(),
                'Launcher must not force theme or display backend')
        # Only unpack/register in our own dpkg database: do not bypass host apt dependencies.
        install_root = work / 'dpkg-root'
        admin = install_root / 'var/lib/dpkg'
        admin.mkdir(parents=True)
        (admin / 'status').touch()
        sentinel = install_root / 'home/test/.hangar/preserve-marker'
        sentinel.parent.mkdir(parents=True)
        sentinel.write_text('isolated non-credential marker')
        cli = install_root / 'usr/bin/hangar'
        cli.parent.mkdir(parents=True)
        cli.write_text('unrelated CLI marker')
        command = ['dpkg', f'--root={install_root}', '--force-not-root', '--log=' + str(work / 'dpkg.log')]
        subprocess.run([*command, '--unpack', str(deb)], check=True, env=env)
        subprocess.run([*command, '--unpack', str(deb)], check=True, env=env)  # Reinstall/upgrade file replacement.
        installed = install_root / 'usr/bin/hangar-desktop'
        require(digest(installed) == digest(binary), 'Installed binary differs')
        state = output('dpkg-query', f'--admindir={admin}', '-W', '-f=${Status}', 'hangar-desktop')
        require(state == 'install ok unpacked', 'Unexpected isolated dpkg state')
        subprocess.run([*command, '--purge', 'hangar-desktop'], check=True, env=env)
        require(all(not (install_root / name).exists() for name in FILES), 'Package files remain after purge')
        require(sentinel.read_text() == 'isolated non-credential marker' and cli.read_text() == 'unrelated CLI marker',
                'Purge changed account directory or CLI')
        require(not (home / '.codex/auth.json').exists(), 'Verification wrote auth.json')
        if smoke:
            subprocess.run(['/usr/bin/python3', str(ROOT / 'scripts/test-linux-desktop.py'),
                            '--binary', str(binary)], check=True, env=env)
    report = ROOT / 'build/linux-qa' / f'package-{manifest["build_id"]}.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps({'build_id': manifest['build_id'], 'artifact': deb.name,
                                 'sha256': digest(deb), 'checksums': 'passed', 'payload': 'passed',
                                 'isolated_dpkg_unpack_reinstall_purge': 'passed',
                                 'host_apt_install': 'not performed',
                                 'desktop_smoke': os.environ.get('GDK_BACKEND', 'wayland') if smoke else 'not performed'}, indent=2) + '\n')
    print(f'Package verified: {deb}\nEvidence: {report}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='directory containing manifest.json and SHA256SUMS.txt')
    parser.add_argument('--smoke', action='store_true', help='also run isolated GUI smoke on the current display')
    args = parser.parse_args()
    try:
        verify(args.directory.resolve(), args.smoke)
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Package verification failed: {error}\n')


if __name__ == '__main__':
    main()
