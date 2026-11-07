param([Parameter(Mandatory=$true)][string]$FfmpegPath, [Parameter(Mandatory=$true)][string]$Sha256)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$taskExe = (Resolve-Path -LiteralPath $FfmpegPath).Path
if ((Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash -ne $Sha256) { throw 'FFmpeg checksum differs from supplied reviewed hash.' }
$taskOldExe = $env:NEKOPAK_TEST_FFMPEG
$taskOldHash = $env:NEKOPAK_TEST_FFMPEG_SHA256
Push-Location $taskRoot
try {
    $env:NEKOPAK_TEST_FFMPEG = $taskExe
    $env:NEKOPAK_TEST_FFMPEG_SHA256 = $Sha256
    cargo test -p nekopak-media --test ffmpeg_integration --locked -- --ignored
    if ($LASTEXITCODE -ne 0) { throw 'Media integration tests failed.' }
} finally {
    $env:NEKOPAK_TEST_FFMPEG = $taskOldExe
    $env:NEKOPAK_TEST_FFMPEG_SHA256 = $taskOldHash
    Pop-Location
}
