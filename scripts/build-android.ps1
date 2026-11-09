[CmdletBinding()]
param(
    [Alias('Profile')][ValidateSet('debug', 'release')][string]$BuildProfile = 'debug',
    [ValidateSet('aarch64', 'x86_64')][string]$Target = 'aarch64',
    [string]$AndroidSdk = $env:ANDROID_HOME,
    [string]$JavaHome = $env:JAVA_HOME,
    [switch]$PrepareOnly,
    [switch]$CopyNativeLibrary
)

$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $taskRoot
try {
    if (-not $AndroidSdk -or -not (Test-Path -LiteralPath $AndroidSdk -PathType Container)) {
        throw 'Pass -AndroidSdk or set ANDROID_HOME to an installed Android SDK.'
    }
    if (-not $JavaHome -or -not (Test-Path -LiteralPath (Join-Path $JavaHome 'bin/java.exe') -PathType Leaf)) {
        throw 'Pass -JavaHome or set JAVA_HOME to an installed JDK 21.'
    }
    $env:JAVA_HOME = [System.IO.Path]::GetFullPath($JavaHome)
    $env:ANDROID_HOME = [System.IO.Path]::GetFullPath($AndroidSdk)
    $env:NDK_HOME = Join-Path $env:ANDROID_HOME 'ndk/27.2.12479018'
    $env:PATH = (Join-Path $env:JAVA_HOME 'bin') + [IO.Path]::PathSeparator + $env:PATH
    $taskJavaVersion = (& (Join-Path $env:JAVA_HOME 'bin/java.exe') -version 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or $taskJavaVersion -notmatch 'version "21[.\"]') {
        throw 'The selected JAVA_HOME must actually run JDK 21; directory names are not version evidence.'
    }
    foreach ($taskComponent in @('platforms/android-36/android.jar', 'build-tools/36.0.0/apksigner.bat', 'ndk/27.2.12479018/source.properties')) {
        if (-not (Test-Path -LiteralPath (Join-Path $env:ANDROID_HOME $taskComponent) -PathType Leaf)) {
            throw "Missing Android SDK component: $taskComponent"
        }
    }
    if ($BuildProfile -eq 'release') {
        foreach ($taskVariable in @('ANDROID_KEYSTORE_PATH', 'ANDROID_KEYSTORE_PASSWORD', 'ANDROID_KEY_ALIAS', 'ANDROID_KEY_PASSWORD')) {
            if (-not [Environment]::GetEnvironmentVariable($taskVariable, 'Process')) {
                throw "Missing Android release signing input: $taskVariable"
            }
        }
        if (-not (Test-Path -LiteralPath $env:ANDROID_KEYSTORE_PATH -PathType Leaf)) {
            throw 'Android release keystore file is missing.'
        }
    }
    if (-not (Test-Path -LiteralPath 'node_modules/@tauri-apps/cli/tauri.js' -PathType Leaf)) {
        & npm ci
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed.' }
    }
    $taskRustTarget = if ($Target -eq 'aarch64') { 'aarch64-linux-android' } else { 'x86_64-linux-android' }
    $taskAbi = if ($Target -eq 'aarch64') { 'arm64-v8a' } else { 'x86_64' }
    & rustup target add $taskRustTarget
    if ($LASTEXITCODE -ne 0) { throw 'Android Rust target installation failed.' }
    & cargo fetch --locked
    if ($LASTEXITCODE -ne 0) { throw 'Locked Rust dependency fetch failed.' }
    if (-not (Test-Path -LiteralPath 'src-tauri/gen/android/app/build.gradle.kts' -PathType Leaf)) {
        & npm run tauri -- android init --ci --skip-targets-install
        if ($LASTEXITCODE -ne 0) { throw 'Tauri Android initialization failed.' }
    }
    & python scripts/android_project.py
    if ($LASTEXITCODE -ne 0) { throw 'Android project preparation failed.' }
    if ($PrepareOnly) { return }

    $taskFlagsName = 'CARGO_TARGET_' + $taskRustTarget.ToUpperInvariant().Replace('-', '_') + '_RUSTFLAGS'
    $taskExistingFlags = [Environment]::GetEnvironmentVariable($taskFlagsName, 'Process')
    [Environment]::SetEnvironmentVariable($taskFlagsName, "$taskExistingFlags -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384", 'Process')
    if ($CopyNativeLibrary) {
        # Tauri CLI's Windows symlink step requires Developer Mode or elevated
        # privileges. Rebuild the same cdylib and copy it instead; no OS changes.
        & npm run build
        if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }
        $taskProject = Join-Path $taskRoot 'src-tauri/gen/android'
        $taskKotlin = Join-Path $taskProject 'app/src/main/java/io/nekopak/desktop/generated'
        [void][IO.Directory]::CreateDirectory($taskKotlin)
        $env:TAURI_ANDROID_PROJECT_PATH = $taskProject
        $env:WRY_ANDROID_PACKAGE = 'io.nekopak.desktop'
        $env:TAURI_ANDROID_PACKAGE_UNESCAPED = 'io.nekopak.desktop'
        $env:WRY_ANDROID_LIBRARY = 'nekopak_app'
        $env:WRY_ANDROID_KOTLIN_FILES_OUT_DIR = $taskKotlin
        $taskNdkBin = Join-Path $env:NDK_HOME 'toolchains/llvm/prebuilt/windows-x86_64/bin'
        $taskClang = Join-Path $taskNdkBin ($taskRustTarget + '24-clang.cmd')
        $taskTargetKey = $taskRustTarget.ToUpperInvariant().Replace('-', '_')
        [Environment]::SetEnvironmentVariable("CARGO_TARGET_${taskTargetKey}_LINKER", $taskClang, 'Process')
        [Environment]::SetEnvironmentVariable(('CC_' + $taskRustTarget.Replace('-', '_')), $taskClang, 'Process')
        [Environment]::SetEnvironmentVariable(('AR_' + $taskRustTarget.Replace('-', '_')), (Join-Path $taskNdkBin 'llvm-ar.exe'), 'Process')
        $taskCargoArgs = @('build', '-p', 'nekopak-app', '--lib', '--target', $taskRustTarget, '--locked')
        if ($BuildProfile -eq 'release') { $taskCargoArgs += '--release' }
        & cargo @taskCargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Android native library build failed.' }
        $taskMetadata = & cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
        $taskNative = Join-Path $taskMetadata.target_directory "$taskRustTarget/$BuildProfile/libnekopak_app.so"
        $taskNativeDestination = Join-Path $taskProject "app/src/main/jniLibs/$taskAbi/libnekopak_app.so"
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $taskNativeDestination))
        Copy-Item -LiteralPath $taskNative -Destination $taskNativeDestination -Force
        if ((Get-FileHash -LiteralPath $taskNative).Hash -ne (Get-FileHash -LiteralPath $taskNativeDestination).Hash) {
            throw 'Copied Android native library hash differs from the fresh Cargo output.'
        }
        $taskFlavor = if ($Target -eq 'aarch64') { 'Arm64' } else { 'X86_64' }
        $taskBuildType = if ($BuildProfile -eq 'debug') { 'Debug' } else { 'Release' }
        $taskFlavorPath = if ($Target -eq 'aarch64') { 'arm64' } else { 'x86_64' }
        $taskApk = Join-Path $taskProject "app/build/outputs/apk/$taskFlavorPath/$BuildProfile/app-$taskFlavorPath-$BuildProfile.apk"
        # Reusing Zipflinger output may leave large unused gaps after replacing a
        # native library. Delete only this exact generated output, never user data.
        if (Test-Path -LiteralPath $taskApk -PathType Leaf) {
            $taskResolvedApk = (Resolve-Path -LiteralPath $taskApk).Path
            $taskExpectedApk = [IO.Path]::GetFullPath($taskApk)
            if ($taskResolvedApk -ne $taskExpectedApk -or -not $taskExpectedApk.StartsWith($taskProject + [IO.Path]::DirectorySeparatorChar)) {
                throw 'Refusing to replace an unexpected APK output path.'
            }
            Remove-Item -LiteralPath $taskApk
        }
        & (Join-Path $taskProject 'gradlew.bat') -p $taskProject "assemble$taskFlavor$taskBuildType" -x "rustBuild$taskFlavor$taskBuildType" --no-daemon
        if ($LASTEXITCODE -ne 0) { throw 'Android Gradle assembly failed.' }
        & python scripts/package_android.py package --profile $BuildProfile --abi $taskAbi --apk $taskApk
    } else {
        $taskArguments = @('run', 'tauri', '--', 'android', 'build', '--target', $Target, '--apk', '--ci')
        if ($BuildProfile -eq 'debug') { $taskArguments += '--debug' }
        & npm @taskArguments
        if ($LASTEXITCODE -ne 0) { throw 'Android APK build failed. On Windows without symlink privileges, use -CopyNativeLibrary.' }
        & python scripts/package_android.py package --profile $BuildProfile --abi $taskAbi
    }
    if ($LASTEXITCODE -ne 0) { throw 'Android APK verification or packaging failed.' }
} finally {
    Pop-Location
}
