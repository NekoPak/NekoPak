# Verified reference manifests

2026-11-08. The Core implementation is in `reference_archive.rs`, its models in
`nekopak-domain/src/references.rs`, and the additional native protocol reads in
`nekopak-provider-native/src/refs.rs`. Protocol evidence and exact upstream paths
are recorded separately in `REFERENCE_ARCHIVE_PROTOCOL_RESEARCH.md`. No upstream
implementation was copied. This is not a claim of completed upstream parity or
live account acceptance.

Public methods are `reference_create(ids,key)`, `reference_list()`,
`reference_status(manifest_id)`, `reference_store_manifest(manifest_id,key)`,
`reference_archive(manifest_id,confirm_trash,key)` and
`reference_restore(manifest_id,allow_recreate,key)`. The store method returns an
ordinary upload `TaskRef`; the others return a `ReferenceManifest` or list.
Public manifests contain the opaque `account_id`, original file tree and
per-item states, never session tokens or internal account scopes.

Creation is read-only toward the server. It fully enumerates selected folders,
rejects duplicates, invalid paths, missing parents and files without a valid
GCID, then saves and reads back a bounded local record. The immutable document
contains manifest ID/time, selected roots and original path/metadata entries.
Its SHA-256 covers the exact JSON bytes. Mutable operation statuses are separate
from those document bytes. Limits are 8,000 entries and an 8 MiB document.

Storing generates a machine-owned JSON file safely under the application data
directory, creates/reuses the dedicated `NekoPak Reference Manifests` cloud
folder using a durable folder intent, and queues the file through the existing
upload engine. The same manifest always reuses its upload task. No original file
is touched during creation or upload.

Archiving requires `confirm_trash=true` and all of these checks:

1. The ordinary upload task completed with real byte counts, matching backend
   and no simulated flag.
2. The saved upload receipt belongs to the same account and exact local file.
3. Local JSON still matches the immutable manifest, and the remote object has
   matching filename, parent, size and GCID.
4. All cloud bytes are read back through the existing authenticated download
   source and compared with the document and its SHA-256.
5. Every original file still has its recorded ID, parent, name, size and GCID.

Only then does Core trash recorded files one at a time. Directories are never
trashed or permanently deleted. Each file is checked again at its own action
boundary, a durable intent precedes the request, and its actual trash state is
read afterward. Files added to a selected directory after planning remain
untouched. Partial outcomes retain the manifest and per-item error. A restored
manifest cannot be reused to trash originals again; a fresh archive is a new
user action.

Restore checks actual trash metadata using the internal `recovery_metadata`
method, validates content/parent/name, checks available storage, rejects target
name conflicts and restores the original object to its original parent. An
already restored object returns idempotently. A missing object is not recreated
unless `allow_recreate=true` was explicitly supplied. That path uses the verified
GCID resumable-file creation protocol and persists the returned ID before
independent confirmation. A pending-upload ID remains visible and is never
automatically deleted. An unknown creation result is not resent under a new
button retry key. Subsequent reads may confirm a previously pending object,
but no automatic byte upload or destructive cleanup occurs.

The implementation preserves directory structure for original-object restoration
because it does not delete the original directories. It currently stops if an
original parent no longer exists rather than inventing a new destination. It
does not merge manifests into each original directory, import a manifest from
another device, archive without GCID, restore from arbitrary source URLs, or
offer permanent deletion. The cloud JSON is a preserved recovery artifact,
not a guarantee that the service will retain content indefinitely. GCID
recreation can fail and consume service quota; no account rotation is performed.

Tests use an in-memory provider and temporary SQLite/local files. They exercise
upload completion plus whole-document readback, corruption/no-confirm gating,
changed-original rejection, preservation of late children and directories,
idempotent trash restoration, explicit recreation, pending receipts, unknown
write suppression, account isolation, missing GCID and restart/corruption
handling. Native loopback tests check the actual recovery GET/creation POST
shape and ensure pending nodes do not trigger deletion. No live user account
was used.

Executed commands:

- `cargo test -p nekopak-core --locked reference_archive --lib`: 6 passed.
- `cargo test -p nekopak-provider-native --locked refs::tests`: 2 passed.
- `cargo test -p nekopak-provider-native --locked`: 56 passed, 0 failed,
  2 intentionally ignored in the integrated native provider.
- `cargo clippy -p nekopak-core --lib --locked -- -D warnings`: passed.
- `cargo clippy -p nekopak-provider-native --all-targets --locked -- -D warnings`:
  passed.

An independent read-only safety review found two recovery issues, both fixed and
covered by the final targeted tests: an existing creation receipt is now loaded
before target-name preflight after a crash, and recovery metadata explicitly
requires `PHASE_TYPE_COMPLETE` (missing phase is not success). The reviewer
rechecked the fixes and reported no remaining blocking finding in that scope.
