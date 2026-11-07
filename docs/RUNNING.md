# Run the M0 backend

This is an offline simulation, not a logged-in PikPak client. It does not download,
upload, delete or mount real files. All displayed quotas and progress are synthetic.

## Windows prerequisites

- Windows x64; Microsoft Edge WebView2 Evergreen installed separately.
- Rust stable MSVC (tested with 1.98.0), Visual Studio C++ desktop tools and Windows SDK.
- Node.js 22+ and npm for the contract checks and Tauri CLI; Python 3.12+ for packaging.

From the repository root:

```powershell
npm ci
cargo test --workspace --locked
npm run check
npm test
cargo run -p nekopak-core --example mock_smoke --locked
cargo run -p nekopak-app --locked
```

The last command opens the minimal acceptance shell. Refresh files, enter a folder,
create a simulated download or URL/magnet task, pause/resume/cancel it, close and
reopen to inspect retained state. Product UI development can replace shell/ by
changing frontendDist; Rust Core and src/lib contracts remain reusable.

```powershell
$env:NEKOPAK_MOCK_SCENARIO = 'disconnect_at37'
cargo run -p nekopak-app --locked
Remove-Item Env:NEKOPAK_MOCK_SCENARIO
```

Valid scenarios: `free`, `premium`, `unauthorized`, `quota_exhausted`, `offline`,
`large_directory`, `name_conflict`, `disconnect_at37`, `integrity_failure`, `disk_full`.
An unknown scenario prevents launch; it is never silently replaced by a default.
The scenario is an application-wide development fixture, including on restart.
The one-time 37% fault resets on process restart.

Storage lives in Tauri's per-user local app data directory for `io.nekopak.desktop`,
in `tasks.sqlite3` plus SQLite WAL/SHM files. A damaged database is left in place and
commands return `INTEGRITY_FAILED`; there is no automatic deletion, reset or repair.
Settings are not implemented and cannot overwrite the task store. Do not delete a
task database to address a configuration problem.

## Build and package

```powershell
npm run tauri -- build --no-bundle
python scripts/package_release.py
```

The output is `dist/NekoPak-0.1.0-windows-x64-mock.zip`, containing the executable,
this guide, notices and dependency licenses. `dist/size-report.json` records bytes,
toolchain, source hash, lockfile hash, compression and external runtime requirements.
It is a portable unsigned development build, not an installer or production release.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p nekopak-core --example export_fixtures --locked
```

The fixture exporter deterministically regenerates tests/fixtures/*.json. The
GitHub Windows workflow repeats tests, type checks, the smoke run and packaging.
Other operating systems have not been built or tested for this milestone.

