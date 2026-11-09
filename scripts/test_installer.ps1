param([Parameter(Mandatory=$true)][string]$Installer)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:RUNNER_TEMP) {
    throw 'Installer smoke test runs only on the disposable GitHub Actions runner.'
}
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')
$installRoot = [IO.Path]::GetFullPath((Join-Path $runnerRoot ('NekoPakSmoke-' + [guid]::NewGuid().ToString('N'))))
if (-not $installRoot.StartsWith($runnerRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Unexpected installer test target.'
}
if (Test-Path -LiteralPath $installRoot) { throw 'Installer test target already exists.' }
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$process = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/D=$installRoot") -WindowStyle Hidden -PassThru
try {
    if (-not $process.WaitForExit(120000)) { throw 'Installer did not finish in two minutes.' }
    if ($process.ExitCode -ne 0) { throw "Installer exited with code $($process.ExitCode)." }
} finally { $process.Dispose() }
$installedExe = Join-Path $installRoot 'nekopak.exe'
foreach ($name in @('nekopak.exe','uninstall.exe','LICENSE','THIRD_PARTY_NOTICES.md','DEPENDENCY_LICENSES.json','FRONTEND_LICENSES.json')) {
    if (-not (Test-Path -LiteralPath (Join-Path $installRoot $name) -PathType Leaf)) { throw "Installer omitted $name." }
}
$expected = Get-Content -LiteralPath 'dist/size-report.json' -Raw | ConvertFrom-Json
if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected.executable_sha256) {
    throw 'Installed executable differs from the signed portable executable.'
}
$programs = [Environment]::GetFolderPath('Programs')
$shortcut = Join-Path $programs 'NekoPak\NekoPak.lnk'
if (-not (Test-Path -LiteralPath $shortcut)) { throw 'Start menu shortcut was not created.' }
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -WindowStyle Hidden -PassThru
try { if (-not $process.WaitForExit(120000)) { throw 'Uninstaller did not finish in two minutes.' } } finally { $process.Dispose() }
$deadline = [DateTime]::UtcNow.AddSeconds(30)
while ((Test-Path -LiteralPath $installedExe) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 250 }
if (Test-Path -LiteralPath $installedExe) { throw 'Uninstaller left the installed executable behind.' }
if (Test-Path -LiteralPath $shortcut) { throw 'Uninstaller left the Start menu shortcut behind.' }
Write-Output 'Installer smoke passed: files and licenses, executable hash, shortcut, and uninstall.'
