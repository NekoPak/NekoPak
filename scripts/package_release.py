"""Package an existing release; measure bytes and collect actual resolved dependency licenses."""
import hashlib
import argparse
import json
from pathlib import Path
import platform
import subprocess
import zipfile

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
        raise SystemExit('Build the Windows release executable first.')
    build_inputs = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock'] + [p for base in ['crates', 'src-tauri', 'shell'] for p in (ROOT / base).rglob('*') if p.is_file() and 'gen' not in p.relative_to(ROOT).parts]
    if any(p.stat().st_mtime > executable.stat().st_mtime for p in build_inputs):
        raise SystemExit('Source files are newer than the executable; rebuild before packaging.')
    output = ROOT / 'dist'
    output.mkdir(exist_ok=True)
    archive = output / 'NekoPak-0.1.0-windows-x64-cli-preview.zip'
    license_entries = []
    supplemental_sources = json.loads((ROOT / 'licenses/sources.json').read_text(encoding='utf-8'))
    # Published crate archives may normalize mtimes to 1970; ZIP starts at 1980.
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as bundle:
        for path in [executable, ROOT / 'LICENSE', ROOT / 'THIRD_PARTY_NOTICES.md', ROOT / 'docs' / 'RUNNING.md', ROOT / 'docs' / 'CLI_ADAPTER.md']:
            bundle.write(path, path.name)
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
    (output / 'DEPENDENCY_LICENSES.json').write_text(manifest, encoding='utf-8')
    windows_tree = run('cargo', 'tree', '--locked', '--offline', '-p', 'nekopak-app', '--target', 'x86_64-pc-windows-msvc', '--edges', 'normal,build', '--prefix', 'none')
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
    for base in ['crates', 'src-tauri', 'shell']:
        for file in sorted((ROOT / base).rglob('*')):
            if file.is_file() and 'gen' not in file.relative_to(ROOT).parts:
                source_hash.update(file.relative_to(ROOT).as_posix().encode())
                source_hash.update(file.read_bytes())
    report = {
        'platform': platform.platform(), 'architecture': platform.machine(),
        'rustc': run('rustc', '-V'), 'cargo': run('cargo', '-V'),
        'commit': commit, 'source_sha256': source_hash.hexdigest(),
        'cargo_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
        'profile': 'release, opt-level=s, LTO, codegen-units=1, strip',
        'archive': archive.name, 'compression': 'ZIP DEFLATE level 9; executable + notices + resolved dependency licenses',
        'executable_bytes': executable.stat().st_size, 'archive_bytes': archive.stat().st_size,
        'executable_sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
        'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
        'external_requirements': ['Windows x64', 'Microsoft Edge WebView2 Evergreen runtime (not bundled)'],
        'not_bundled': ['PikPak CLI', 'Rclone', 'WinFsp', 'Chromium', 'JVM', 'FFmpeg', 'mpv'],
        'scope': 'Offline Mock by default; explicit reviewed official CLI read-only preview. No real transfers or bundled official CLI.',
        'runtime_measurement': 'Not measured by this script; no memory/startup claim.',
        'license_inventory_scope': 'Cargo.lock resolved graph, including target-specific and build/test dependencies; not a legal certification.',
        'license_text_unresolved': unresolved,
        'windows_dependency_license_text_gaps': windows_gaps,
    }
    (output / 'size-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()

