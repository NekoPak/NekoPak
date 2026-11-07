# Official CLI slice verification — 2026-11-07

Scope: M1 read-only preview/probe. Windows x64, Rust/cargo 1.98.0,
Node 24.18.0. Original M0 results remain in TEST_REPORT.md.

## Executed checks

- `cargo test --workspace --locked --offline`: 39 test entries passed,
  zero failures (includes three subprocess helper entries across the suite).
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed without warnings.
- `cargo fmt --all --check`: formatting verified after applying rustfmt.
- `npm run check`: strict TypeScript check passed.
- `npm test`: five tests passed, including the redacted actual CLI logged-out
  fixture, byte-safe event merging and the existing Mock scenarios.
- `cargo run -p nekopak-core --example export_fixtures --locked --offline`:
  regenerated ten existing Mock scenarios with the additive real_transfers flag.
- `cargo run -p nekopak-provider-cli --example cli_probe --locked --offline --
  <reviewed absolute exe> <isolated profile>`: succeeded against the actual
  official v0.6.0 executable; authenticated=false and needs_login. Evidence in
  docs/evidence/official-cli-adapter-probe.json.

The 13 new CLI test entries cover raw u64::MAX, display-size rejection, UTC
normalization, omitted raw URLs/account details, duplicate/stale/oversized pages,
unsafe ID arguments, option-like page tokens, recognized/unrecognized error mapping,
opt-in selection, version drift, capability reports, process hash validation,
bounded stdout and timeout/cancellation killing an actually started child.

Version, `ls/get/auth` help, logged-out auth status and unauthorized list/get errors
were executed against the checksum-verified binary. `--raw` alone was observed to
leave errors as human text, so the adapter uses `--raw -F json` together. The probe
created only an isolated vendor profile/device identity; it never read an existing
user profile or authorized an account. Vendor diagnostics were removed from saved
evidence; no credentials are present.

## Standards/security review

Review baseline f05854f, initial implementation 0f5b747. Two presentation defects
were found: CSS flex display could override a hidden cloud-task form, and native
window title remained Offline Mock in real-provider mode. Both were fixed using
explicit hidden/disabled controls and a backend-specific title. Focused read-only
recheck confirmed zero unresolved original findings. No concrete shell injection,
credential environment leakage or raw-diagnostic IPC disclosure was found.

## Spec review

Two defects were found: the native mode title above and failure to enforce the
requested page size. The parser now accepts the requested limit and rejects an
overfull response; a regression uses eleven distinct entries for limit ten.
Focused review confirmed zero unresolved original findings. Successful metadata
fixtures remain explicitly synthetic; no authorized live success is claimed.

## Remaining verification

Real authorized root/paginated browsing and stat still require user authorization
through the external CLI. In-app credential storage/login/logout, real downloads,
cloud writes, quota, Range and mounts are not implemented. No account token was
requested or collected. The vendor CLI credential files are external and are not
represented as encrypted OS storage by NekoPak.

Native release builds, process startup and package sizes are recorded in the final
evidence files for this slice. Actual GUI-click E2E remains unverified because the
desktop automation helper was unavailable in this session. IPC roundtrip tests and
native title/process checks do not substitute for browser rendering verification.

Final rebuilt `npm run tauri -- build --no-bundle`: passed after review fixes.
`scripts/measure_runtime.ps1 -Runs 1` then succeeded once in each mode:

- Mock: correct `NekoPak — Offline Mock` title, 272 ms to window handle,
  95,375,360 bytes private working set for the complete seven-process tree.
- CLI preview with the isolated unauthorized profile: correct
  `NekoPak — Official CLI Read-only Preview` title, 144 ms to window handle,
  90,456,064 bytes private working set for the complete seven-process tree.

These are single samples taken five seconds after handle availability, not startup
frame timing or performance comparisons. Both trees include WebView2 descendants.
Raw results: cli-slice-mock-runtime.json and cli-preview-runtime.json under evidence/.
