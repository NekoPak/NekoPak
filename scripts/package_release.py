"""Package an existing release; measure bytes and collect actual resolved dependency licenses."""
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding='utf-8').strip()

def main():
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
    archive = output / 'NekoPak-0.1.0-windows-x64-mock.zip'
    license_entries = []
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for path in [executable, ROOT / 'LICENSE', ROOT / 'THIRD_PARTY_NOTICES.md', ROOT / 'docs' / 'RUNNING.md']:
            bundle.write(path, path.name)
        for package in metadata['packages']:
            if package['source'] is None:
                continue
            base = Path(package['manifest_path']).parent
            files = sorted({p for pattern in ['LICENSE*', 'LICENCE*', 'COPYING*', 'NOTICE*', 'UNLICENSE*', 'license*', 'licenses/**/*'] for p in base.glob(pattern) if p.is_file()})
            record = {'name': package['name'], 'version': package['version'], 'license': package.get('license'), 'source': package['source'], 'license_files': [str(p.relative_to(base)) for p in files]}
            license_entries.append(record)
            for file in files:
                bundle.write(file, f"licenses/{package['name']}-{package['version']}/{file.relative_to(base).as_posix()}")
        manifest = json.dumps(license_entries, ensure_ascii=False, indent=2)
        bundle.writestr('DEPENDENCY_LICENSES.json', manifest)
    (output / 'DEPENDENCY_LICENSES.json').write_text(manifest, encoding='utf-8')
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
        'scope': 'Offline M0 mock backend; no live network provider or actual file downloads.',
        'runtime_measurement': 'Not measured by this script; no memory/startup claim.',
        'license_inventory_scope': 'Cargo.lock resolved graph, including target-specific and build/test dependencies; not a legal certification.',
    }
    (output / 'size-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()

