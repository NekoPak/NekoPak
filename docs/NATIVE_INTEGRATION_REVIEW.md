# Native integration review

2026-11-08, independent read-only review of root-authored catalog/cloud/media/local/streams/uploads/jobs
and Tauri integration while implementation was active. Protocol and sync author also rechecked their
own crossing points, but that is not represented as independent review of those modules.

Actionable findings sent to root as discovered:

1. **P1: account scope can change between check and write dispatch.** A sync run and batch download
   hold the original account scope, but `files_mutate`, `upload_start`, `transfer_start` originally
   re-read whichever account is current. A logout/login between check and dispatch can create work
   in the new account using old IDs. Fixed-scope variants now exist; sync uses them, and root migrated
   catalog enqueue and file-picker dispatch. Native protocol writes also bind the original epoch.
2. **P1: cached media could serve stale representation bytes.** Cache-hit reads validated local
   SHA256 but not the remote representation. Same-size transcodes could reuse the original hash.
   Root added metadata revalidation before cache reads and requires strong representation ETag,
   places it in identity and uses If-Match for chunks. Live ETag availability is still unverified.
3. **P1: unbounded media cache could fill the application drive.** Originally all touched 1MiB
   blocks accumulated without an effective budget. Root added a serialized hard budget check.
   This is a bounded-cache mitigation, not evidence of a fully tuned LRU policy.
4. **P2: archive replay could regress completed jobs, and empty selection meant extract all.**
   `archive_extract` recreated queued/revision1 records on idempotent replay; `[]` mapped to the
   backend's all-files shape. Root was asked to preserve existing job records and reject empty
   selections unless the explicit string `all` was provided.
5. **P1: native file-picker and stream registration account race.** `upload_pick_and_start` could
   finish a dialog after an account change and dispatch each file under a new account. Stream
   registration could race logout after sources were prepared. Root added fixed picker scope and
   stream epoch checks under the stream registry lock, incremented on logout/mapping removal.
6. **P1: old account task could starve current account workers.** Media/upload workers selected the
   first active task and returned on scope mismatch. With an old account's running task ahead of
   the new account, no new work could run. Root was notified to skip mismatched tasks while selecting.

Sync-specific fixes from implementation self-review: baseline reset deferred until valid config save;
first-run scans ignore old baseline; links fail scan rather than appearing deleted; local and remote
nested roots rejected; application data/cache cannot be a sync root; all side-effect dispatch receives
the run scope; completed child task receipts replay before aggregate rechecks; target backups use the
existing non-overwriting Windows handle rename; post-run snapshots cannot absorb unrelated changes.

Actual sync tests at this point: 3 unit and 3 integration tests passed. Integration includes byte-exact
downloads, old-target backup, baseline only after child completion, restart between child completion
and aggregate commit, network failure without a plan, stale local-file rejection, account isolation,
and opt-in deletion confirmation that moves the local file to backup. These are local tests, not
live-service acceptance. Core clippy initially flagged two compact `}if` lines in streams.rs; root
formatting resolves them, and final workspace checks belong to the integrating agent.

No live credentials, external account requests, or user's existing files were used. No security
certificate or full feature-parity claim is made by this review.
