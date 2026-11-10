# Android integration and acceptance

2026-11-10. Pre-alpha Android adaptation; this is not a declaration of upstream
functional parity or live-account acceptance. The Windows ZIP/Setup installer
pipeline has passed; Android has its own build, signing and device checks.

Version 0.1.1 replaces the earlier desktop-shaped phone navigation with a dedicated
Files / Transfers / Me shell, touch rows and settings subpages. Launcher resources
now come from the project's cat-box layers and are checked inside the compiled APK.
Android's native application locale drives the System language preference. Login
preferences, connection recovery, bounded download retries and terminal-history
removal are described in [MOBILE_RECOVERY_REPORT.md](MOBILE_RECOVERY_REPORT.md).
The 0.1.0 checks below remain historical evidence, not verification of the corrected icon.

Version 0.1.2 adds native `.torrent` selection (SAF content URI stays in Rust/native
code), preview and explicit cloud submission. It fixes bottom navigation moving
above the software keyboard: native IME visibility hides the navigation until the
keyboard closes, and Back dismisses the keyboard first. The byte download worker
now runs continuously instead of waiting for every 500ms UI/cloud poll. Current
evidence and constraints are in [SPEED_TORRENT_IME_REPORT.md](SPEED_TORRENT_IME_REPORT.md).

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

```powershell
./scripts/build-android.ps1 -BuildProfile debug -Target x86_64 -CopyNativeLibrary `
  -JavaHome '<JDK21 directory>' -AndroidSdk '<Android SDK directory>'
```

Release uses `-BuildProfile release -Target aarch64` with the four signing inputs
loaded into that process. Never put passwords in shell arguments or source. The
Gradle configuration retains standard Android ProGuard rules as well as the native
JNI keep rules; minification must be tested on an installed Release APK.

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

## Minified ARM64 release runtime check

The signed, non-debuggable ARM64 APK was installed over the same signing identity
on the isolated API35 x86_64 emulator using its ARM translation bridge. The login
page starts without a vault error. R8 initially removed Jackson enum methods used
by Tauri; the build now includes Android default ProGuard rules and retains JNI
classes. The corrected Release starts, remains alive and displays content below
the status bar. This does not replace a physical ARM64 device test.

## Release build evidence

The runtime-tested local ARM64 APK is `dist/android/NekoPak-0.1.0-android-arm64-v8a.apk`
(17,330,797 bytes, 16.53 MiB; SHA256 `059ec2c1397b1d92fa0abb212a7ac0bf90933605a56e31ac4b3d11c0ccb0fcb3`).
Its independent Android certificate and Ed25519 manifest both verify. The package
contains 268 Rust/69 JVM/4 frontend runtime license records with no text gaps.
Evidence: `evidence/android-release-package.json`, `evidence/android-release-smoke.json`.

Canonical source commit: `df2141c0443861a3c1fa5c282394c9c0245ed1eb`.
The [Android preview workflow](https://github.com/NekoPak/NikoPak/actions/runs/42617477353)
passed. The Android job in the [signed release workflow](https://github.com/NekoPak/NikoPak/actions/runs/42617478011)
also passed. Its Linux-built APK has a distinct artifact hash; cross-host byte-identical
reproducibility is not claimed. Release manifests identify exact artifacts.

The downloaded GitHub ARM64 APK was also installed as a same-certificate update
on the isolated emulator and started to the login page with correct system insets.
Its exact digest and independent verification are in
`evidence/android-actions-package.json`; no real credentials were entered.

## 0.1.1 phone and recovery verification (2026-11-10)

The new x86_64 debug APK passed 14 actual Android WebView/Rust/JNI checks on the
dedicated API35 emulator, including native Chinese locale, phone-only navigation,
measured byte throughput, automatic recovery from an injected HTTP503, preserved
manual/background pauses, process recovery, SAF export and durable history removal.
Downloads and exported files have matching SHA256. Account HTTP traffic is synthetic
loopback traffic with an in-memory provider vault; the separate JNI Keystore checks
exercise actual Android secure storage. See `evidence/mobile-recovery-android-native.json`.

Browser phone fixtures passed 12/12 across 360/390/412px and both themes. Packaging
and launcher tests passed 20/20 with actual AAPT2 resources, clean-cache Maven
license inheritance and tampered-metadata rejection. The APK checker verifies
25 PNG variants and four adaptive resource graphs, including round and themed icons.
Screenshots from the installed WebView are named `screenshots/android/phone-*.png`;
browser fixture screenshots are labeled separately. Signed release artifacts and
their runtime checks are recorded in the corresponding package reports.

The final [0.1.1 signed workflow](https://github.com/NekoPak/NikoPak/actions/runs/42770550171)
passed on source `2a99844334c276c1e0a36e0ce4060f065fb54fbe`. Its ARM64 APK is
17,322,049 bytes (16.52 MiB), SHA256
`391c1f723c4d0e8c32b7f387a4a8941dc80a87439ae35619febf105cf47cdb7c`.
After download, the manifest, certificate, native-library hash, 16KiB alignment,
compiled icon pixels and embedded Maven license provenance were verified locally.
That exact APK upgraded the signed 0.1.0 baseline and passed two minified cold boots
with Chinese login and remembered-account controls. It ran through the emulator's
ARM translation bridge; a physical ARM64 phone and real account remain user acceptance.
Evidence: `evidence/mobile-recovery-actions-android.json` and
`evidence/mobile-recovery-release-runtime.json`.
