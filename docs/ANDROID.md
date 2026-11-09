# Android integration and acceptance

2026-11-09. Pre-alpha Android adaptation; this is not a declaration of upstream
functional parity or live-account acceptance. The Windows ZIP/Setup installer
pipeline has passed; Android has its own build, signing and device checks.

## Implemented host

- Tauri mobile library entry shares the Rust Core/native API provider with desktop.
  Android uses the same audited public Web client identity, not copied Android
  client secrets or a replacement protocol. Passwords stay request-only.
- AndroidKeyStore AES-256-GCM seals sessions, account indexes and temporary remote
  authorization records in noBackupFilesDir. Keys are not exported. Corrupt records
  and missing keys fail closed. Backup is disabled. Hardware-backed/StrongBox
  storage is not guaranteed on every device.
- Mobile navigation, touch file activation/selection, system back, keyboard/system
  bar insets, small-screen dialogs, explicit platform capability flags.
- App-private Downloads for original and media downloads. Completed files can be
  exported with Android's system document picker. content URIs stay in native code.
  Export validates ownership/completion/length and stops on account changes; it
  preserves the original file. A failing remote document provider can leave a
  partial export and arbitrary providers are not promised atomic/power-loss-safe.
- Activity suspension gates workers and durably pauses local transfers, including
  scope-await and cancellation-registration races. The user resumes after returning.
  Server-side cloud jobs are not cancelled when backgrounding the app.

Android upload/SAF directory import, shared-directory sync, local mappings, external
player integration and a foreground download service remain disabled. Windows tray,
startup registration, Cloud Files and WinFsp are desktop features. These boundaries
do not delete any eventual upstream parity requirements. Uninstalling Android removes
the app's private downloads and credentials; export wanted files first.

## Build

Minimum Android API24; primary package ARM64, emulator package x86_64. The project
pins JDK21, SDK/target36, Build Tools36.0.0, NDK27.2.12479018, AGP8.11.0,
Gradle8.14.3 and Kotlin2.2.10. Exact upstream template differs; see
[ANDROID_RESEARCH.md](ANDROID_RESEARCH.md). No global system settings are changed.

```powershell
npm ci
$env:JAVA_HOME = '<JDK21 directory>'
$env:ANDROID_HOME = '<Android SDK directory>'
$env:NDK_HOME = Join-Path $env:ANDROID_HOME 'ndk/27.2.12479018'
rustup target add aarch64-linux-android x86_64-linux-android
cargo fetch --locked
npm run tauri -- android init --ci --skip-targets-install
python scripts/android_project.py
npm run tauri -- android build --debug --target aarch64 --apk --ci
python scripts/package_android.py package --profile debug --abi arm64-v8a
```

Owned Kotlin lives in `src-tauri/android`; the preparation script copies it into the
ignored generated project, applies SDK/Gradle/signing/security settings, and records
source hashes. Re-run after changing host sources. Never edit generated sources as
the source of truth. `.cargo/config.toml` sets 16KiB ELF page alignment; packaging
also verifies ZIP alignment and every native ELF LOAD segment.

Windows without symbolic-link privileges uses the explicit copy build path in
`scripts/build-android.ps1`; it compiles the current Rust sources before copying
the library. Do not disable Windows security or silently enable Developer Mode.

## Signing and Actions

Android release identity is independent of Windows Authenticode, Tauri updater and
the existing Ed25519 release manifest. Protected local backup contains the Android
PKCS12 and password metadata; no private key is tracked. Four `ANDROID_*` Secrets
are configured on NekoPak/NikoPak. Reuse the same identity for future upgrades.

`android.yml` builds an explicitly debug-signed ARM64 preview without secrets.
`release.yml` builds and verifies separate Windows and Android signed artifacts;
Android release fails when keystore inputs are missing. The release report records
APK SHA256, signing certificate, ABI/SDK and license/16KiB validation. Public GitHub
Release publication is separate from artifact upload. `parity-*` remains gated.

## Verification status

Actual AndroidKeyStore instrumentation on an isolated Android15/API35 x86_64 emulator:
3 passed, 0 failed. It covers roundtrip/overwrite/delete, plaintext-buffer clearing,
tamper rejection and target-bound ciphertext. Evidence:
`evidence/android-vault-tests.json`. The full APK Rust JNI path is covered by the native tests below.

Frontend phone checks cover 360/390/412 widths, native capability-disabled controls,
navigation and export interaction with IPC fixtures. Core regression tests cover
background gating and persistence, account isolation, completion/length export
checks and pausing during awaited scope/I/O. Real account login, browser CAPTCHA,
Free/Premium behavior, device codecs, weak networks and physical ARM64 devices still
need acceptance in NekoPak. Passwords or tokens are never requested in chat.

## Actual full APK acceptance (2026-11-09)

`tests/e2e/android-native.mjs` passed 8/8 on the isolated API35 x86_64 emulator:
Rust JNI Keystore roundtrip/accounts/auxiliary records; synthetic native sign-in and
file list; background pause with no further Range requests; process restart retaining
bytes and explicit resume; original SHA256; actual SAF picker cancel and successful
shared-Downloads export with equal SHA256; no WebView page overflow. Evidence:
`evidence/android-native-e2e.json`. No real account was used.

The device test found and fixed Android SELinux rejecting hard links: Android now
publishes using `renameat2(RENAME_NOREPLACE)` and syncs the destination directory.
Startup creates its WebView only after managed Rust state is ready. Host regression
keeps the existing Windows vault namespace and Common Controls manifest intact.

Mobile browser fixtures: 5/5; desktop browser regression: 57/57; native Windows
startup: 4/4. Synthetic fixture screenshots are labeled separately from the Android
WebView capture. Platform-wide production readiness is still not claimed.
