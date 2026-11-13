"""Package an existing release; measure bytes and collect actual resolved dependency licenses."""
import hashlib
import argparse
import json
from pathlib import Path
import platform
import subprocess
import zipfile
import tempfile
import shutil

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding='utf-8').strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--parity-release', action='store_true', help='Require complete upstream Windows parity before packaging.')
    options = parser.parse_args()
    if options.parity_release:
        subprocess.run(['python', 'scripts/check_parity.py', '--release'], cwd=ROOT, check=True)
    # Full resolved-graph license inventory includes non-Windows packages too.
    subprocess.run(['cargo', 'fetch', '--locked'], cwd=ROOT, check=True)
    metadata = json.loads(run('cargo', 'metadata', '--locked', '--offline', '--format-version', '1'))
    target = Path(metadata['target_directory'])
    executable = target / 'release' / 'nekopak.exe'
    if not executable.is_file():
        raise SystemExit('Build the Windows release executable first (cargo build --release -p nekopak-desktop).')
    # The native desktop client is built only from the Rust workspace; icons,
    # palettes and locales generated from the shared sources are committed.
    source_roots = ['crates']
    build_inputs = [ROOT / name for name in ['Cargo.toml', 'Cargo.lock']] + [p for base in source_roots for p in (ROOT / base).rglob('*') if p.is_file() and 'gen' not in p.relative_to(ROOT).parts]
    if any(p.stat().st_mtime > executable.stat().st_mtime for p in build_inputs):
        raise SystemExit('Source files are newer than the executable; rebuild before packaging.')
    output = ROOT / 'dist'
    output.mkdir(exist_ok=True)
    version = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))['version']
    archive = output / f'NekoPak-{version}-windows-x64-portable.zip'
    license_entries = []
    supplemental_sources = json.loads((ROOT / 'licenses/sources.json').read_text(encoding='utf-8'))
    # Glyph paths and colour schemes generated from these packages are embedded.
    ui_sources = ['@material-symbols/svg-400', '@material/material-color-utilities']
    ui_licenses = []
    for name in ui_sources:
        directory = ROOT / 'node_modules' / name
        package = json.loads((directory / 'package.json').read_text(encoding='utf-8'))
        texts = [p for p in directory.iterdir() if p.is_file() and p.name.lower().startswith(('license', 'licence', 'notice'))]
        if not texts:
            raise SystemExit(f'Missing frontend license text: {name}')
        ui_licenses.append({'name': name, 'version': package['version'], 'license': package.get('license'), 'files': [p.name for p in texts]})
    # Published crate archives may normalize mtimes to 1970; ZIP starts at 1980.
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as bundle:
        for path in [executable, ROOT / 'LICENSE', ROOT / 'THIRD_PARTY_NOTICES.md', ROOT / 'docs' / 'RUNNING.md', ROOT / 'docs' / 'DESKTOP_INTEGRATION_REPORT.md', ROOT / 'docs' / 'MOUNT_IMPLEMENTATION.md', ROOT / 'docs' / 'ACCOUNTS_IMPLEMENTATION.md', ROOT / 'docs' / 'REFERENCE_ARCHIVE_IMPLEMENTATION.md', ROOT / 'docs' / 'RELEASE_SIGNING.md']:
            bundle.write(path, path.name)
        for item in ui_licenses:
            for name in item['files']:
                bundle.write(ROOT / 'node_modules' / item['name'] / name, f"licenses/frontend/{item['name']}/{name}")
        bundle.writestr('FRONTEND_LICENSES.json', json.dumps(ui_licenses, ensure_ascii=False, indent=2))
        for package in metadata['packages']:
            if package['source'] is None:
                continue
            base = Path(package['manifest_path']).parent
            files = sorted({p for pattern in ['LICENSE*', 'LICENCE*', 'COPYING*', 'NOTICE*', 'UNLICENSE*', 'license*', 'licenses/**/*'] for p in base.glob(pattern) if p.is_file()})
            supplements = [entry for entry in supplemental_sources if entry['name'] == package['name'] and entry['version'] == package['version'] and 'file' in entry]
            record = {'name': package['name'], 'version': package['version'], 'license': package.get('license'), 'source': package['source'], 'source_archive_url': f"https://static.crates.io/crates/{package['name']}/{package['name']}-{package['version']}.crate", 'license_files': [str(p.relative_to(base)) for p in files], 'supplemental_licenses': supplements}
            license_entries.append(record)
            for file in files:
                bundle.write(file, f"licenses/{package['name']}-{package['version']}/{file.relative_to(base).as_posix()}")
            for entry in supplements:
                file = ROOT / entry['file']
                if hashlib.sha256(file.read_bytes()).hexdigest() != entry['sha256']:
                    raise SystemExit(f"Supplemental license checksum mismatch: {file.name}")
                bundle.write(file, f"licenses/{package['name']}-{package['version']}/upstream/{file.name}")
        manifest = json.dumps(license_entries, ensure_ascii=False, indent=2)
        bundle.writestr('DEPENDENCY_LICENSES.json', manifest)
    # Stage the exact contents as an ordinary multi-file application directory.
    # The installer consumes the same notices/resources. UI assets are compiled
    # into the executable: no self-extraction happens on application startup.
    portable = output / 'portable' / 'NekoPak'
    portable.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='nekopak-stage-', dir=output) as staging:
        staging = Path(staging)
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(staging)
        if portable.exists():
            resolved = portable.resolve()
            intended = (output.resolve() / 'portable' / 'NekoPak')
            if resolved != intended or portable.is_symlink():
                raise SystemExit('Refusing to replace an unexpected portable staging directory.')
            shutil.rmtree(portable)
        shutil.copytree(staging, portable)
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as bundle:
        for path in sorted(portable.rglob('*')):
            if path.is_file():
                bundle.write(path, f'NekoPak/{path.relative_to(portable).as_posix()}')
    (output / 'DEPENDENCY_LICENSES.json').write_text(manifest, encoding='utf-8')
    windows_tree = run('cargo', 'tree', '--locked', '--offline', '-p', 'nekopak-desktop', '--target', 'x86_64-pc-windows-msvc', '--edges', 'normal,build', '--prefix', 'none')
    windows_packages = {f"{parts[0]}@{parts[1][1:]}" for line in windows_tree.splitlines() if len(parts := line.split()) > 1 and parts[1].startswith('v')}
    unresolved = [f"{p['name']}@{p['version']}" for p in license_entries if not p['license_files'] and not p['supplemental_licenses']]
    windows_gaps = sorted(set(unresolved) & windows_packages)
    if windows_gaps:
        raise SystemExit('Missing Windows dependency license texts: ' + ', '.join(windows_gaps))
    try:
        commit = run('git', 'rev-parse', 'HEAD')
    except subprocess.CalledProcessError:
        commit = None
    source_hash = hashlib.sha256()
    for base in source_roots:
        for file in sorted((ROOT / base).rglob('*')):
            if file.is_file() and 'gen' not in file.relative_to(ROOT).parts:
                source_hash.update(file.relative_to(ROOT).as_posix().encode())
                source_hash.update(file.read_bytes())
    report = {
        'platform': platform.platform(), 'architecture': platform.machine(),
        'rustc': run('rustc', '-V'), 'cargo': run('cargo', '-V'),
        'commit': commit, 'source_sha256': source_hash.hexdigest(),
        'git_worktree_dirty': bool(run('git', 'status', '--porcelain')),
        'cargo_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
        'npm_lock_sha256': hashlib.sha256((ROOT / 'package-lock.json').read_bytes()).hexdigest(),
        'frontend_license_inventory': ui_licenses,
        'profile': 'release, opt-level=s, LTO, codegen-units=1, strip',
        'archive': archive.name, 'compression': 'ZIP DEFLATE level 9; NekoPak directory with executable, notices and resolved dependency licenses',
        'portable_directory': portable.relative_to(ROOT).as_posix(),
        'portable_file_count': sum(p.is_file() for p in portable.rglob('*')),
        'runtime_layout': 'Single native executable with its own renderer (GPU with CPU fallback); no web view, no startup self-extraction.',
        'executable_bytes': executable.stat().st_size, 'archive_bytes': archive.stat().st_size,
        'executable_sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
        'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
        'external_requirements': ['Windows 10 version 1709 or later, x64'],
        'not_bundled': ['PikPak CLI', 'Rclone', 'WinFsp', 'Chromium', 'WebView2', 'JVM', 'FFmpeg', 'mpv'],
        'scope': 'Desktop integration: native accounts/files/upload/cloud/share/archive/MP4-MKV-TS clips and remux/sync/local media/CloudFiles plus optional WinFsp and five-language desktop UX. Tests use synthetic accounts and isolated OS fixtures. Live account, full upstream parity and other-platform acceptance remain separately tracked. No CLI or bundled media runtime/driver.',
        'runtime_measurement': 'Not measured by this script; no memory/startup claim.',
        'license_inventory_scope': 'Cargo.lock resolved graph, including target-specific and build/test dependencies; not a legal certification.',
        'license_text_unresolved': unresolved,
        'windows_dependency_license_text_gaps': windows_gaps,
    }
    (output / 'size-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()

