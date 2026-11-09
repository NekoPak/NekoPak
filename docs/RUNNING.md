# Run NekoPak desktop integration

Current integrated capabilities and actual tests: [DESKTOP_INTEGRATION_REPORT.md](DESKTOP_INTEGRATION_REPORT.md).
Windows packaging and installer lifecycle are verified. Android instructions and acceptance are in
[ANDROID.md](ANDROID.md); Linux/macOS desktop acceptance remains pending. Current connections may require
the user's proxy; absence of Hosts blocking is not evidence of proxy-free connectivity.

2026-11-08: the official PikPak CLI adapter has been removed. Desktop uses the
supplied v3 UI and the Rust `rust_compat` connection. This is an unsigned pre-alpha:
real Free/Premium service acceptance is incomplete. DRIVE verification, native file
operations, read-only share listing, account quota and original-file downloads are
implemented and tested against an isolated HTTP service. Upload, cloud tasks, shares, archives, native media, safe sync and optional mounts are now wired;
see the integration report for source/format/runtime limitations.

## Run the provided Windows application

Extract `NekoPak-0.1.0-windows-x64-portable.zip` and double-click `NekoPak/nekopak.exe`.
Keep the directory's notices and licenses alongside the executable. Alternatively, run
`NekoPak-0.1.0-windows-x64-setup.exe` for per-user installation, shortcuts and an uninstaller.
The native application embeds its frontend resources and does not self-extract them on startup.
The required runtime is Windows x64 with Microsoft Edge WebView2 Evergreen.
No compiler, terminal command, CLI installation, client registration or environment
variable is needed to run the provided application. Enter the account email and
password in NekoPak; the login button is enabled on normal startup.

If the drive requests human verification, click **完成安全验证**, finish the browser
page, return and click **继续**. The signed-in session stays available; DRIVE verification
does not need the password again. Set an existing local download folder under
**设置 → 下载与传输**. Original-file downloads support pause, resume and restart recovery;
cancelled/failed partial files are retained. Existing destination files are never replaced.
Click the download path or **选择文件夹** to open the Windows directory picker. Cancelling
keeps the previous folder. **下载时询问保存位置** enables a per-download picker. The task
menu opens the containing folder recorded for that task, including after changing the default.
Task rows, the footer and sidebar show measured download throughput.

**设置 → 外观** offers system/Simplified Chinese/Traditional Chinese/English/Japanese/Korean,
and **关闭窗口时最小化到托盘**. Language changes apply without reloading. Tray actions show/hide
the window, open Downloads, or quit. Closing to
tray is off by default; when enabled the downloader keeps running after the window closes.

**设置 → 网络 → 网络增强** is off by default. It supports the system proxy, direct
connections, or a user-provided HTTP(S)/SOCKS5(H) proxy without embedded credentials. Optional
TLS 1.2 compatibility affects authentication/drive API connections. Certificate checks remain
enabled. Settings apply only to NekoPak and load before initial authentication requests.

The website/browser development preview uses simulated data. Real account testing
must use the Windows executable, which sends requests through the Rust backend.

## Build from source (development only)

- Windows x64, separately installed Microsoft Edge WebView2 Evergreen.
- Rust 1.98+ MSVC, Visual Studio C++ desktop tools and Windows SDK.
- Node 22+, npm; Python 3.12+ for packaging. The Tauri build CLI is a development
  tool and is unrelated to the removed PikPak CLI backend.

```powershell
npm ci
npm run build
cargo run -p nekopak-app --locked
```

Default startup discovers exactly one native connection and offers email/password
sign-in immediately. The verified public Web client identifier is bundled; no
developer registration or environment variable is required. No client secret or
account credential is bundled. Real account sign-in and human verification belong
to the user; do not paste passwords or tokens into chat.
Protocol/client configuration is the developer's responsibility. The user performs
real sign-in and human verification in NekoPak once a testable build is delivered;
the user is not expected to discover client parameters or provide credentials in chat.
Details and live verification gaps: API_ADAPTER.md. API origins are fixed HTTPS,
not environment-configurable. This remains a login test preview, not a feature-complete release.

For the offline persistent Rust Mock instead:

```powershell
$env:NEKOPAK_BACKEND = 'mock'
$env:NEKOPAK_MOCK_SCENARIO = 'free'
cargo run -p nekopak-app --locked
Remove-Item Env:NEKOPAK_BACKEND
Remove-Item Env:NEKOPAK_MOCK_SCENARIO
```

Scenarios: free, premium, unauthorized, quota_exhausted, offline, large_directory,
name_conflict, disconnect_at37, integrity_failure, disk_full. Invalid modes/scenarios
fail startup. Browser `npm run dev` uses independent UI-only demonstration fixtures;
Tauri always uses Rust IPC and never falls back to the browser mock.

## Storage and credentials

Per-user app data for `io.nekopak.desktop`: `api-preview.sqlite3` for native mode,
`tasks.sqlite3` for explicit Mock, WAL/SHM and OS-locked sidecars. The former CLI
preview DB is left untouched and is never reused as a simulated queue. Only one
process owns each store; corrupt, empty or malformed existing databases fail closed
and are preserved. Do not delete databases to fix configuration problems.

Tokens are held in Windows Credential Manager, scoped by app-data directory and
client ID. Passwords never persist. CAPTCHA continuation requires password re-entry
because the initial password has been discarded. Logout deletes the credential item;
no remote revoke endpoint is claimed. Android uses Android Keystore to encrypt session records in
app-private no-backup storage. Linux/macOS secret stores are not implemented.

## Checks and packaging

```powershell
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p nekopak-core --example export_fixtures --locked
cargo run -p nekopak-provider-native --example export_fixture --locked
npm run check
npm test
npm run build
npm run e2e
python scripts/check_parity.py
npm run tauri -- build --no-bundle
python scripts/package_release.py
```

The browser E2E suite uses no real account. It supports Playwright Chromium or the
installed Microsoft Edge with `E2E_BROWSER_CHANNEL=msedge`. The isolated real
WebView2 login-entry check is `node tests/e2e/native-startup.mjs` after a debug
desktop build; it submits only an empty form and never account credentials. The
Windows vault integration test is opt-in and writes/deletes a unique synthetic
credential only; it never enumerates or reads user credentials:

```powershell
cargo test -p nekopak-provider-native windows_vault_roundtrip_and_delete -- --ignored
```

Portable package: `dist/NekoPak-0.1.0-windows-x64-portable.zip`. It includes the binary,
notices, full dependency license inventory and current usage notes, not historical
CLI installation instructions. `dist/size-report.json` records build/source hashes,
bytes, runtime requirements and license gaps. No driver, media runtime, browser,
PikPak CLI or Rclone is bundled. Formal parity release remains blocked until all
mandatory upstream capabilities pass actual acceptance.

Current functional evidence: DESKTOP_INTEGRATION_REPORT.md and evidence/native-functional-e2e.json.
Prior native file implementation: DRIVE_FILES_FIX_REPORT.md.
Earlier login/migration evidence: LOGIN_FIX_REPORT.md, API_MIGRATION_REPORT.md. Windows signed manifests, portable ZIP and installer install/uninstall have passed Actions checks
(RELEASE_SIGNING.md). Linux/macOS desktop, clean consumer Windows10/11, live throughput
and production readiness remain unverified.

The auxiliary `.actions.sqlite3` file stores remote operation intents, download receipts
and settings. Keep it alongside the main DB. A missing initialized journal stops writes
instead of silently rebuilding an empty deduplication history. Transfer history is
filtered by account; native events request a fresh filtered snapshot without including
another account's task metadata. Download throughput is volatile Rust state and can change
without advancing the persisted task revision; resumed bytes do not count as new traffic.

To replace the application icon later, edit `public/brand/app-icon.svg`, run
`npm run gen:app-icons`, then rebuild the frontend and Windows executable. The same asset
supplies the UI mark, EXE/window icon and tray icon. No external icon download is needed.
