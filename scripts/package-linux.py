#!/usr/bin/env python3
"""Build native Linux desktop candidates. No install, tag, upload or publication."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def output(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def sha256(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def copy(source, target, mode=0o644):
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    target.chmod(mode)


def debian(exe, dest, work, version, arch):
    package = work / 'debian/hangar-desktop'
    copy(exe, package / 'usr/bin/hangar-desktop', 0o755)
    copy(ROOT / 'apps/linux/data/dev.mocika.hangar.desktop', package / 'usr/share/applications/dev.mocika.hangar.desktop')
    copy(ROOT / 'resources/shared/hangar-icon.svg', package / 'usr/share/icons/hicolor/scalable/apps/dev.mocika.hangar.svg')
    copy(ROOT / 'apps/linux/README.md', package / 'usr/share/doc/hangar-desktop/README.md')
    copy(ROOT / 'apps/linux/data/copyright', package / 'usr/share/doc/hangar-desktop/copyright')
    (package / 'DEBIAN').mkdir()
    source_control = work / 'debian/control'
    source_control.parent.mkdir(exist_ok=True)
    source_control.write_text('Source: hangar-desktop\nSection: utils\nPriority: optional\n'
                              'Maintainer: Mocika <mocikadev@users.noreply.github.com>\n\n'
                              'Package: hangar-desktop\nArchitecture: any\nDescription: Codex account manager\n')
    dependencies = output('dpkg-shlibdeps', '-O', '-e' + str(package / 'usr/bin/hangar-desktop'), cwd=work)
    dependencies = next(line.removeprefix('shlibs:Depends=') for line in dependencies.splitlines()
                        if line.startswith('shlibs:Depends='))
    dependencies += ', libgtk-4-1 (>= 4.14), libadwaita-1-0 (>= 1.5)'
    control = package / 'DEBIAN/control'
    installed_size = (sum(p.stat().st_size for p in package.rglob('*') if p.is_file()) + 1023) // 1024
    control.write_text(f'Package: hangar-desktop\nVersion: {version}-1\nArchitecture: {arch}\n'
                       'Maintainer: Mocika <mocikadev@users.noreply.github.com>\n'
                       'Section: utils\nPriority: optional\n'
                       f'Installed-Size: {installed_size}\nDepends: {dependencies}\n'
                       'Recommends: xdg-utils\nHomepage: https://github.com/mocikadev/hangar\n'
                       'Description: Native GTK4 Codex account manager\n'
                       ' Manage accounts and compare weekly quota with a GNOME native desktop.\n')
    for directory in [package, *(p for p in package.rglob('*') if p.is_dir())]:
        directory.chmod(0o755)
    subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(package), str(dest)], check=True, stdout=sys.stderr)
    if output('dpkg-deb', '-f', str(dest), 'Version') != f'{version}-1':
        raise RuntimeError('Debian version readback mismatch')
    return dependencies


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'dist/linux')
    args = parser.parse_args()
    os.chdir(ROOT)
    if platform.system() != 'Linux':
        parser.error('Linux host required')
    rustc = output('rustc', '-vV')
    target = next(line.removeprefix('host: ') for line in rustc.splitlines() if line.startswith('host: '))
    mapping = {'x86_64-unknown-linux-gnu': ('x64', 'amd64', 'Advanced Micro Devices X86-64'),
               'aarch64-unknown-linux-gnu': ('arm64', 'arm64', 'AArch64')}
    if target not in mapping:
        parser.error('Only native GNU Linux builds are supported')
    arch, deb_arch, machine = mapping[target]
    for tool in ['readelf', 'ldd', 'desktop-file-validate', 'dpkg-shlibdeps', 'dpkg-deb']:
        if not shutil.which(tool):
            parser.error(f'Missing build tool: {tool}')
    subprocess.run(['desktop-file-validate', 'apps/linux/data/dev.mocika.hangar.desktop'], check=True)
    build = output('cargo', 'build', '--locked', '--release', '--target', target, '-p', 'hangar-linux',
                   '--message-format=json-render-diagnostics')
    executables = [item['executable'] for line in build.splitlines()
                   if (item := json.loads(line)).get('reason') == 'compiler-artifact'
                   and item.get('executable') and item['target']['name'] == 'hangar-desktop']
    if len(executables) != 1:
        raise RuntimeError('Expected exactly one Cargo desktop executable')
    exe = Path(executables[0])
    version = tomllib.loads((ROOT / 'apps/linux/Cargo.toml').read_text())['package']['version']
    if output(str(exe), '--version') != f'hangar-desktop {version}':
        raise RuntimeError('Executable version does not match Cargo package')
    elf = output('readelf', '-h', str(exe), env={**os.environ, 'LC_ALL': 'C'})
    dependencies = output('ldd', str(exe), env={**os.environ, 'LC_ALL': 'C'})
    if machine not in elf or 'not found' in dependencies:
        raise RuntimeError('ELF target mismatch or missing runtime dependency')
    epoch = int(os.environ.get('SOURCE_DATE_EPOCH', output('git', 'show', '-s', '--format=%ct', 'HEAD')))
    os.environ['SOURCE_DATE_EPOCH'] = str(epoch)
    build_id = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    dest = args.output_dir.resolve() / build_id
    dest.mkdir(parents=True, exist_ok=False)
    staging = ROOT / 'build/staging'
    staging.mkdir(parents=True, exist_ok=True)
    artifacts = []
    extra = {}
    with tempfile.TemporaryDirectory(prefix=f'{build_id}-', dir=staging) as temporary:
        work = Path(temporary)
        path = dest / f'hangar-desktop_{version}-1_{deb_arch}.deb'
        extra['debian_depends'] = debian(exe, path, work, version, deb_arch)
        artifacts.append(path)
    manifest = {
        'schema_version': 2, 'version': version, 'build_id': build_id,
        'channel': 'linux-native-build-validation', 'product': 'hangar', 'entry': 'desktop',
        'git_commit': output('git', 'rev-parse', 'HEAD'),
        'dirty_source': bool(output('git', 'status', '--porcelain')), 'profile': 'release',
        'target': target, 'arch': arch, 'abi': 'gnu', 'source_date_epoch': epoch,
        'rustc': rustc, 'host': platform.platform(), 'os_release': Path('/etc/os-release').read_text(),
        'system_libraries': {p: output('pkg-config', '--modversion', p) for p in ['gtk4', 'libadwaita-1', 'glib-2.0']},
        'elf': elf, 'runtime_dependencies': dependencies,
        'glibc_required_symbols': sorted(set(re.findall(r'GLIBC_[0-9.]+', output('readelf', '--version-info', str(exe))))),
        'artifacts': [{'path': p.name, 'bytes': p.stat().st_size, 'sha256': sha256(p)} for p in artifacts],
        'signature': 'unsigned', 'validation': {'built': True, 'version_readback': True,
            'desktop_interaction': 'not performed by build; run verify-linux-package.py --smoke',
            'other_distributions': 'unverified', 'real_accounts': 'not validated by packaging'},
        **extra,
    }
    manifest_path = dest / 'manifest.json'
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    (dest / 'SHA256SUMS.txt').write_text(''.join(f'{sha256(p)}  {p.name}\n' for p in [*artifacts, manifest_path]))
    print(dest)


if __name__ == '__main__':
    main()
