# M0 backend verification — 2026-11-07

Host: Windows 11 x64, build 10.0.26300. Rust/cargo 1.98.0 MSVC,
Visual Studio C++ 14.51.36231, Node 24.18.0, npm 11.16.0.
All results below were executed locally; the GitHub Actions job is configured
but has not been dispatched to a hosted runner.

## Executed checks

- `cargo test --workspace --locked --offline`: PASS, 26 test entries,
  0 failures (includes one subprocess helper entry). Final raw output is in
  `docs/evidence/test-output.txt`.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  PASS, no warnings.
- `cargo fmt --all --check`: PASS.
- `npm run check`: PASS, strict TypeScript checking.
- `npm test`: PASS, 4 tests including real Rust fixture/error data and reordered
  revisions above JavaScript's safe integer boundary.
- `cargo run --locked --offline -p nekopak-core --example export_fixtures`:
  PASS, 10 deterministic scenario files generated; repeated export was byte-identical.
- `cargo run --locked --offline -p nekopak-core --example mock_smoke`:
  PASS, 8 root entries and a simulated 100,000,000-byte task completed through the
  core state machine, with no destination file or network transfer.
- `npm run tauri -- build --no-bundle`: PASS, native Windows x64 release executable.
  Final core fixes were rebuilt successfully; executable SHA-256:
  `1b3976495652d5616980ed065c5001fac0ceb7e131e26a4e8c60c6a4bc680f63`.

## Tested behavior

Decimal u64 transport (including u64::MAX), legal/illegal task transitions,
401/403/404/429 mapping, bounded timeout and cancellation, capability selection,
no post-dispatch fallback, complete 10,000-item pagination without duplicates,
cursor invalidation after remote changes, structured error IPC, all 10 Tauri
command roundtrips, request-ID duplicate rejection and cleanup, durable progress,
pause/resume/retry/cancel, commit-before-event revisions, stale-worker race handling,
concurrent/durable idempotency, no URL query token in database bytes,
Windows dangerous-path rejection and credential-URL rejection.

Recovery tests cover restart during running work, restart beyond the 37% injected
network failure, an actual child-process kill while an SQLite transaction is open,
binary corruption, zero-byte truncation, malformed task payload, future schema,
missing idempotency table, separate malformed settings, and a second process/store
owner being rejected before it can recover live work.

The kill test is process-crash simulation, not a hardware power-cut or filesystem
durability certification. Mock verification does not validate real download bytes.

## Windows process/resource observations

`scripts/measure_runtime.ps1` successfully launched and stopped the release three
times. All three had a main window titled `NekoPak — Offline Mock` and six WebView2
descendants. Each sample was taken at least five seconds after a window handle
became available. Raw evidence is in `docs/evidence/runtime-report.json`.

- Window handle availability: 258, 138, 146 ms; median 146 ms. This is not the first
  rendered frame, a cold-start metric or a p95 estimate.
- Whole process-tree private working set: 100,380,672 / 97,316,864 / 95,096,832 bytes;
  approximately 95.73 / 92.81 / 90.69 MiB, median 92.81 MiB.
- Main Rust process private working set: approximately 4.06 / 4.11 / 4.24 MiB.
  This small number alone is not the application's memory footprint.
- Total summed working set: 402,509,824 / 389,775,360 / 387,846,144 bytes; includes
  shared pages that can be counted more than once. Use the private figures above
  for the comparable process-tree baseline.

Packaging bytes, archive SHA-256, toolchain, source/lock hashes and dependency
inventory are generated in `dist/size-report.json` and retained as evidence.
The executable is 7,345,664 bytes (7.01 MiB). The final ZIP is 4,057,881 bytes
(3.87 MiB), including notices and the resolved license inventory (726 archive
entries, 422 dependency records). No WebView2 runtime is bundled. This meets the
5 MiB challenge for the M0 simulation only, not the future real-service client.
Archive SHA-256: `923d88786534e1d0315fbc58f519751dad95be3e8003635982937ce2cc250cc0`.
The final archive is integrity-tested and its executable hash checked against the
built file. A dependency timestamp before 1980 exposed a Python ZIP limitation;
packaging now clamps unsupported timestamps and preserves file contents.
Windows normal/build dependency license-text gaps: zero after pinned supplements.
Six unused other-platform dependency records still lack verified license text;
they remain explicit in the all-target inventory and must be resolved before those
platforms are distributed. Measurements refer to packaging commit `bb37b3e` and
core-source hash `ccee8d3947e3be398c9d0a29d6aac3b0e031b0b3ac972a76717e4dc8536f82e9`.

## Review and remaining limits

Two read-only review agents checked documented standards and M0 requirements.
Four standards findings and three spec findings (overlapping) were repaired;
focused rechecks confirmed all original findings resolved. See CODE_REVIEW.md.

Actual desktop click/UI accessibility E2E could not run: the bundled computer-use
helper returned `Computer Use native pipe is unavailable ... os error 2` on initial,
retry and fresh-session attempts. Native process startup and Tauri mock-runtime
IPC were verified; visual rendering and button click automation were not.

No real PikPak account/CLI, network byte transfer, throughput, physical power-cut,
clean Windows VM, installer/signing, Linux/macOS build, or mounting test was run.
Cargo dependency license files are packaged, but cargo-deny/RustSec advisory
certification and final distribution legal review remain outside this M0 result.
