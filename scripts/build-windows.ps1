param([switch]$Package)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $taskRoot
try {
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }
    cargo build --locked --release -p nekopak-app
    if ($LASTEXITCODE -ne 0) { throw 'Windows release build failed.' }
    if ($Package) {
        python scripts/package_release.py
        if ($LASTEXITCODE -ne 0) { throw 'Packaging failed.' }
    }
} finally { Pop-Location }

