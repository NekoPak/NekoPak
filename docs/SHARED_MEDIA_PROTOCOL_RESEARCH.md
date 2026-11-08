# Unrestored shared originals, directory uploads and reference archives

2026-11-08. Protocol facts only; independent implementation. No third-party
implementation was copied, no user credentials were read, and no live account
or cloud file was used. Public website scripts are not treated as licensed
reusable source or as an official public API contract.

## Direct share reads: verified source path

The ordinary drive build `v2.30.3-9-gcc540f8e` has `/share` and `/share/detail`
wrappers but does not establish that a folder listing contains a download URL.
The separate public sharing site provides the missing evidence. A request for
the deliberately nonexistent route
`https://mypikpak.com/s/nekopak-protocol-audit` returned its public application
shell, release `pikpak-share-production-11-5-2026`. Only its static scripts were
read. No actual share identifier, pass code or authorization was submitted.

The source chain is:

- [DInvZON2.js](https://mypikpak.com/s/pikcloud/DInvZON2.js), `wX`, exported as
  `a4`: GET `/drive/v1/share/file_info`, with `share_id`, `file_id` and
  `pass_code_token` query fields. `global_file_token` is removed from the query
  and, when relevant to archive contents, passed as `x-global-file-token`.
  SHA-256: `e8cb532bf08fb703dc38852390b3e36a39163653c9aa9a4f0921aab43b6b3214`.
- [DSaTV4K1.js](https://mypikpak.com/s/pikcloud/DSaTV4K1.js), line 110: calls that
  wrapper with the share authorization, checks `share_status === "OK"`, and
  passes `file_info` to preview handlers. Its torrent preview reads
  `file_info.web_content_link`. SHA-256:
  `640222816c97d67c824785e9f2897cad234de0d8ebb180cdb5a263eef8a004d9`.
- [BEKRRzer.js](https://mypikpak.com/s/pikcloud/BEKRRzer.js), function `Tb`, line
  50: the media preview's refresh function calls the same endpoint using
  `shareId`, `passCodeToken`, `file_id` and optional archive parameters. It
  returns `detail: response.file_info`, the separate `shareStatus`, and uses
  the detail's `web_content_link`/`medias`. The player respects
  `is_visible`/`need_more_quota` rather than treating every link as readable.
  SHA-256: `ce1a59945ce3aa4d34ce31effd18e2a6660c5ed409d37a673fb8d96e5baafde0`.
- [Dxyw7JhB.js](https://mypikpak.com/s/pikcloud/Dxyw7JhB.js): the share store
  checks `share_status` on both initial and folder pages. Failure is not an
  empty directory. SHA-256:
  `3a3b7f2da44522bd7c60173e4c5dd9e4ce4b0f652f6809adfa56c4d6e8e30a06`.

The precise URL/file/length/SHA-256 inventory is in the ignored research cache
`target/file-protocol-audit/manifest-share-web.json`. The cached scripts are
research inputs and must not be included in a distribution.

Implemented ProviderPort surface:

```rust,ignore
share_source_info(access_ref, id) -> DownloadInfo
share_chunk(access_ref, id, identity, scope, start, length, observer) -> DownloadChunk
share_retain(access_ref) -> String
share_release(access_ref) -> ()
```

`DownloadInfo` carries only normal public file metadata, content identity and
account scope. The signed URL and pass-code token remain inside Rust. The
implementation reads `file_info.web_content_link` for the original file only;
it rejects restricted originals and missing links rather than substituting a
transcode. It requires a hash or modification time plus exact size as its
content version and validates ID/kind/phase. Chunk reads refresh the metadata,
compare account and content identity, then reuse the existing bounded CDN Range
reader. API authorization, CAPTCHA and device headers are not forwarded to the
CDN. This path makes no restore/instant-upload/cloud-creation request.

The native ordinary read bridge accepts the explicit ID form
`shared:<opaque-access-reference>:<remote-file-id>`. It is routed only by
`stat`, `download_info/chunk`, `media_info/chunk` and `media_options`.
Metadata retains that encoded ID and exposes no navigable parent from the
share owner's drive. Media options contain the original representation;
requesting a different quality fails. The normal mutation ID validator rejects
these encoded IDs, so a shared selection cannot accidentally mutate a same-ID
file in the active account.

Transient share preview references are epoch-bound memory handles. A task may
call `share_retain` to create a separate opaque `share-<uuid>` source reference.
The share ID, authorization token, title and owning scope are saved only in the
OS credential vault. They are never saved in SQLite. Retained references work
after process restart or switching back to the owning account; another account
cannot use or release them. Account removal/logout clears only that account's
protected sources. Each task/group must retain its own reference and release it
only after its durable task state no longer needs it. Expired or revoked server
authorization fails explicitly; no undocumented renewal is attempted.

Archive-internal shared reads and shared transcode choice are separate work:
the new endpoint evidence includes their parameters, but the current original
reader does not claim those features.

## Share restore compatibility correction

MIT [pikpak-kotlin ShareEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/ShareEndpoint.kt)
documents and implements `parent_id`, `specify_parent_id: true`, optional
`ancestor_ids`, and the response fields `restore_status`, `restore_task_id`.
The website's DSaTV4K1.js also polls `restore_task_id` and distinguishes
`RESTORE_START` from `RESTORE_COMPLETE`. The native restore path now sends the
verified destination fields and accepts/polls `restore_task_id` alongside the
previous `task_id` receipt. HTTP success without a verifiable receipt remains
unknown, never a fabricated success or automatic retry.

Pinned source: `8eaf5587a883ed4ea9bcca7b43a9194118719e63`, license MIT (read from
that commit). Original paths, Git blob IDs and content SHA-256 values are in
`target/file-protocol-audit/manifest-shared-media.json`. Additional independently
read sources include `ShareEndpointMockTest.kt`, `Models.kt`,
`RangeDownloadEndpoint.kt`, `PikPakFileHandle.kt` and `ShareProbeTest.kt`.
The upstream probe was read, not executed; its live claims are upstream
observations, not NekoPak test results. No authentication constants were reused.

## Directory upload composition and archive constraints

[FolderEndpoints.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/FolderEndpoints.kt)
and the previously audited official drive wrapper establish ordinary folder
creation via `POST /drive/v1/files {kind:"drive#folder",parent_id,name}`.
[UploadEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/UploadEndpoint.kt)
uses the existing resumable-file creation/OSS flow for each child. No separate
directory-upload endpoint is needed. Core should scan a bounded local tree,
reject reparse/symlink traversal, record a stable plan, create each folder with
a persisted no-replay intent, and enqueue children using the existing
`upload_start_at` account scope. Unknown folder creation cannot be blindly
repeated; incomplete progress must remain resumable and visible.

`crates/nekopak-core/src/upload_tree.rs` now implements this composition through
`upload_directory_start(path,parent,key,ctx)` and the picker-safe
`upload_directory_start_at(path,parent,key,expected_scope,ctx)`. A plan includes
the selected directory itself, empty directories and all files. It is bounded
to 8,000 entries, 64 path components and an 8 MiB serialized plan. It rejects
symlinks/reparse points, nonportable filenames, duplicate paths after case
folding and a preexisting same-name destination entry before cloud writes.
Two complete identity scans precede the first mutation. Every mkdir has its own
durable user-intent key, each child upload uses a stable key and fixed account
scope, and retries reload the original plan. Changes to an unfinished local
tree fail rather than silently becoming different work. Restored folder
receipts are checked before new children are queued.

Core serializes directory creation with `upload_tree_gate`. It does not perform
cloud rollback or delete partially created folders when another action fails.
The UI/host must retain and reuse the same operation key to resume an interrupted
plan. A new user key is a new action, subject to the destination conflict check.
This is queued upload behavior, not proof that all children have completed.

Reference archives are distinct from ZIP/RAR extraction. A safe initial archive
can preserve a fully enumerated file manifest and confirm its persistence before
explicitly trashing originals. Restoration from trash uses the recorded file
IDs and verifies content/parent/name before acting. Permanent deletion, content
recreation and fallback to another source need their own user intent and
verification. A newly created share of the same originals is not proof of
recoverability after deletion: the upstream share protocol documents a share
becoming `DELETED` when its files are gone. Do not trash originals merely because
their newly created share opens successfully.

## Executed checks

- `cargo test -p nekopak-provider-native --locked`: 55 passed, 0 failed,
  2 intentionally ignored.
- `cargo clippy -p nekopak-provider-native --all-targets --locked -- -D warnings`:
  passed.
- `cargo test -p nekopak-core --locked upload_tree --lib`: 6 passed on Windows;
  the separate Unix symlink test is not compiled on this platform.
- `cargo clippy -p nekopak-core --lib --locked -- -D warnings`: passed.
- Six new synthetic tests exercise actual 206 bytes without a restore call,
  no API credentials on CDN requests, changed/revoked share rejection,
  restricted/missing original links, retained-source restart/account isolation,
  the verified restore destination/task response shape, and encoded read-only
  source IDs without quality substitution.

Tests use loopback HTTP and memory vaults. They do not establish live share
access, live CDN Range compatibility, frontend integration or complete parity.
