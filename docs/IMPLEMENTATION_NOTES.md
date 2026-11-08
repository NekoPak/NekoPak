# M0 backend implementation and handoff

> 2026-11-08：本文保留历史阶段记录。官方 CLI 路线已移除，当前实现以 [API_ADAPTER.md](API_ADAPTER.md) 和 [API_MIGRATION_REPORT.md](API_MIGRATION_REPORT.md) 为准；下文旧实现/下一步不再代表当前计划。

后续进展：M1 的官方 CLI 只读探测/适配切片已实现，详见
[历史 CLI 记录](history/CLI_ADAPTER.md)。以下保留 M0 历史说明；官方 CLI 的当前版本、
JSON 支持和未授权行为以新文档及对应实测证据为准。

Baseline: supplied NekoPak Development Kit v0.1, 2026-11-07. Scope: backend first
iteration only. Gate 0 source strategy: route B, independent Rust workspace with MIT
license. No third-party client source was copied; no research mirror or remote repo
was created. Real-service feasibility remains open and does not block offline M0.

## Delivered structure

- `nekopak-domain`: contract models, decimal-string u64, structured errors, transfer
  state machine, capability checks and SyncPlan DTOs (no sync execution).
- `nekopak-provider`: async ProviderPort, capability-aware selection, bounded request
  timeout/cancellation. No fallback after a provider has received an operation.
- `nekopak-provider-mock`: deterministic fixtures, 10 scenarios, per-operation
  401/403/404/429/timeout/quota/restriction/disk/integrity injection, 10,000 entries,
  stale-cursor detection after remote rename/size changes, 37% disconnect.
- `nekopak-core`: one owner for task state, transactional SQLite migration, WAL with
  synchronous=FULL, durable idempotency, optimistic task revision checks, simulated
  worker, error-safe recovery. Corrupt databases and semantically invalid task
  records fail read-only validation before migrations or recovery writes. Existing
  zero-byte/version-0 databases and missing v1 tables also fail closed; only a new
  absent database receives the initial migration. A lifetime OS file lock prevents
  concurrent app instances from treating each other's live work as interrupted.
- `src-tauri`: 10 IPC commands, event forwarding, request cancellation registry,
  500 ms mock worker. CSP confines the local acceptance shell. No shell/fs plugins,
  no password UI, no automatic telemetry or external network configuration.
- `shell`: small acceptance UI, not the product frontend; labels all data as mock.
  `src/lib/api-types.ts` and `api.ts` are the frontend handoff, with injectable invoke.

## Contract additions (v0.1 additive freeze)

No original command or field was renamed. Added `revision` and `simulated` to tasks
and transfer events; all byte counts and revisions are decimal strings, including
small values. `app_status`, `quota_get`, TaskRef and action argument shapes are now
concrete. Optional `request_id` supports request_cancel; transfer_start optionally
accepts idempotency_key. Cloud task creation requires a key. Details are appended to
03_CONTRACTS.md, mirrored in TypeScript and Rust-produced fixtures.

The 10 fixture scenarios include normalized task IDs/times, create/control/final
snapshots, ordered and duplicated/reversed events, and relevant second file pages.
The 37% network fault only fires when crossing the threshold from below, so restart
after that point cannot rewind progress.

Snapshots remain authoritative. A UI should read a snapshot, subscribe, reconcile a
second snapshot (closing the subscription gap), then reject duplicate/older event
revisions using BigInt. On event lag, the shell emits resync-required. The minimal
acceptance UI always reloads snapshots after events; product UI can merge deltas.

## Reliability and limits

Task updates commit before events. Concurrent pause/cancel wins against stale worker
updates. Restart preserves queued/paused/terminal tasks; running/retrying/verifying
tasks become paused with recovery context and retain bytes. The mock re-enters
verification on resume. No real `.part` file or content hash is implemented in M0;
simulated verification is explicitly not a real integrity guarantee.

Cloud idempotency hashes the source+parent request and stores only fingerprint, key
and stable fake remote ID. URLs/query tokens are neither persisted nor emitted.
Because only Mock is enabled, repeated provider calls are deterministic and harmless.
A real adapter requires a durable pre-dispatch intent and provider reconciliation
before it can be enabled; the M0 gate rejects non-mock transfers.

Core commands use per-provider-call timeout (15 seconds), cancellation via watch,
and no automatic request replay. Synchronous SQLite operations use a bounded busy
timeout; a committed operation cannot be undone by a late cancellation. Optional
download keys are recommended; omitting one explicitly creates a new simulated task.

## Source observations and Bern decision

Raw observations are in `docs/evidence/source-audit.json`.
The official CLI landing page was retrieved successfully. Neither `pikpak` nor
`pikpakcli` was found on PATH. No CLI executable was downloaded/executed and no
account authorized. JSON, range reads, CLI version and redistribution permission
remain UNKNOWN. No real API claim is made.

pikpaktui at `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c` declares Apache-2.0,
blocking reqwest with rustls, ratatui, syntect, image and self_update. Only manifest
research is complete; authentication/download source audit is deferred to M1. None
of those modules were extracted.

Piko v1.4.0 GitHub release metadata lists x64 MSI at 125,558,784 bytes, universal
APK at 40,302,013 bytes and arm64-v8a APK at 16,545,279 bytes. These are asset metadata,
not local execution measurements; installer compression internals were not inspected.
The product includes different features/runtimes, so these sizes are not an equivalent
benchmark against the M0 portable mock ZIP.

Bern commit `a86bbb85bd525d6c9b0c011ef1571caae4273490` declares 0.1.9, MIT and
Rust 1.99, and calls itself early preview. Its protocol/engine/host separation is
useful; NekoPak keeps the core independent of Tauri. Its README's Windows 4.2 MB /
35 MB figures describe its showcase with platform runtime assumptions and were not
reproduced. Retain Tauri for M0; any Bern UI substitution needs a separate prototype
covering large lists, IME, accessibility, progress updates and full resource accounting.

## Not implemented

Real auth/session and secret vault, official CLI/native/WebDAV/Rclone adapters,
real file operations/download bytes/range reads/checksums, settings service, search,
sync execution, Cloud Files/WinFsp/FUSE, installer/signing/update system, production
UI, clean-machine or live Free/Premium service tests. Unsupported capabilities stay
disabled, never marked ready by guessed backend availability.

## Next actionable issue

`[backend] Official CLI compatibility probe`: with a user-installed, explicitly
authorized CLI, record executable identity/version, help, machine-readable output,
auth lifecycle and error mappings; test read-only listing/pagination first. Keep
download and Range capabilities disabled until observed. Decide the adapter contract
and write fixtures before any live write or cloud-task retry logic is enabled.

## Verification

Commands and measured results are recorded in `docs/TEST_REPORT.md` after execution.
Release measurements are generated in dist/size-report.json. Local warm-start memory
samples include WebView2 descendants and are recorded with their limitations. No production memory,
throughput or clean-machine compatibility claim follows from a mock build.
