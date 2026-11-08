# M0 review and resolutions

> 2026-11-08：本文保留历史阶段记录。官方 CLI 路线已移除，当前实现以 [API_ADAPTER.md](API_ADAPTER.md) 和 [API_MIGRATION_REPORT.md](API_MIGRATION_REPORT.md) 为准；下文旧实现/下一步不再代表当前计划。

Review baseline: `f3583a1` imported development-kit documents.
Initial implementation: `3dca23f`. Two independent, read-only agents used the
code-review skill's Standards and Spec axes. No full product/frontend review was
claimed; the user requested the backend M0 slice.

## Standards

Four actionable findings were raised and repaired:

- Existing zero-byte databases could be initialized as empty queues. Existing
  schema-version 0 now fails read-only validation; first initialization is limited
  to a previously absent path under exclusive process ownership.
- A second app instance could pause another live worker by invoking recovery.
  An OS file lock is held throughout Store lifetime and acquired before inspection.
- The 37% fault rearmed after restart and could report decreasing progress.
  Injection now requires crossing the threshold from below.
- Missing idempotency schema could be reconstructed empty. Existing v1 table
  definitions are checked against the expected schema; migrations do not run on v1.

All four have regression tests. Focused follow-up: all resolved, no remaining
defect found in these fixes. No material heuristic code-smell finding was added.

## Spec

Three actionable findings were raised and repaired:

- The same 37% restart regression violated persisted-progress recovery.
- The same missing-idempotency schema issue violated durable replay semantics.
- The first fixtures omitted task/control/event/error flows and the second page
  containing a filename collision. The exporter now includes stable normalized
  task snapshots, controls, ordered/reversed/duplicate events and relevant pages.

Focused follow-up inspected generated JSON and confirmed all three resolved.
Command names and Rust/TypeScript shapes align; real-service and M1+ omissions are
explicitly documented. No material scope creep was found.

The reviewer also identified a clean-CI risk from offline Cargo metadata without
all-platform manifests. Packaging now fetches the locked graph, checks source
freshness and records the executable SHA-256. The acceptance shell now reads status
independently of quota/list failures so faults do not hide the storage status.

Final focused review: Standards 0 unresolved; Spec 0 unresolved. Reviews inspected
code; actual test/build results were executed separately and are in TEST_REPORT.md.
