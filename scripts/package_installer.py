"""Create a real NSIS Setup.exe from the already verified portable release."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def main():
    report_path = ROOT / 'dist/size-report.json'
    report = json.loads(report_path.read_text(encoding='utf-8'))
    executable = ROOT / 'target/release/nekopak.exe'
    if hashlib.sha256(executable.read_bytes()).hexdigest() != report['executable_sha256']:
        raise SystemExit('Build/package mismatch: create the portable package first.')
    # The NSIS bundler only packages the already-built native executable
    # (mainBinaryName); no WebView runtime is installed or required.
    # bundle, rather than build, avoids changing the already-measured executable.
    # This is artifact signing, not per-bundle Tauri updater signing.
    subprocess.run([
        'node', str(ROOT / 'node_modules/@tauri-apps/cli/tauri.js'),
        'bundle', '--bundles', 'nsis', '--config', 'src-tauri/tauri.installer.conf.json',
        '--ci', '--no-binary-patching',
    ], cwd=ROOT, check=True)
    version = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))['version']
    installers = list((ROOT / 'target/release/bundle/nsis').glob(f'NekoPak_{version}_x64-setup.exe'))
    if len(installers) != 1:
        raise SystemExit('Expected exactly one current-version x64 NSIS installer.')
    output = ROOT / 'dist' / f'NekoPak-{version}-windows-x64-setup.exe'
    shutil.copy2(installers[0], output)
    if hashlib.sha256(executable.read_bytes()).hexdigest() != report['executable_sha256']:
        raise SystemExit('Bundling unexpectedly changed the portable executable.')
    report['installer'] = {
        'name': output.name, 'bytes': output.stat().st_size,
        'sha256': hashlib.sha256(output.read_bytes()).hexdigest(),
        'type': 'NSIS, per-user install, shortcuts and uninstall support',
        'webview2': 'Not required: the application renders its own native UI',
        'authenticode': False,
        'execution_test': 'Not installed by this packaging script.',
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report['installer'], ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()
