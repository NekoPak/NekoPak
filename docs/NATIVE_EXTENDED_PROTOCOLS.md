# Native cloud tasks, shares, archives and uploads

2026-11-08. Independent protocol implementations; no account credentials used in development.
The public/private client protocol is not an official public API. No AGPL code was copied.

## Provenance

The existing file research covers fixed public Web build `v2.30.3-9-gcc540f8e` and
Apache-2.0 pikpaktui commit `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`, MIT Rclone
commit `82cb84b2014bec6a167dc28eb53233b2d4abe1b3`. New reads from pikpaktui were
`src/pikpak/offline.rs`, `src/pikpak/upload.rs`, `src/pikpak/responses.rs`,
`src/cmd/offline.rs`, with Git blob identities verified against downloaded content.
Original URL/path/blob/SHA256 inventory: `target/file-protocol-audit/manifest-extended.json`.
Those caches are ignored research material and are not compiled or shipped.

- [Cloud source](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/offline.rs)
- [Resource shape](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/cmd/offline.rs)
- [Upload source](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/upload.rs)
- [Share source](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/share.rs)
- [Archive wire facts](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/api/types.go)
- Official Web `5492.76e680dd.js` supplies `/share`, `/share/detail`, `share:batchDelete`,
  `/task`, `/tasks`, `/decompress/v1/list`, `/decompress/v1/decompress`, `/decompress/v1/progress`.
  Official `9173.9f7d05c6.js` confirms archive body and status values
  `OK`, `PASS_WORD_EMPTY`, `PASS_WORD_ERROR`. Hashes were recorded in FILE_PROTOCOL_RESEARCH.md.

## Behavior implemented

Cloud creation sends `POST /drive/v1/files` with `UPLOAD_TYPE_URL`, resource URL and selected
parent. It requires a real task ID/phase receipt. Progress polls `GET /drive/v1/tasks/{id}`.
Retry uses `POST /drive/v1/task {id,type:"offline",create_type:"RETRY"}` for that existing
task; cancellation sends DELETE `/drive/v1/tasks?task_ids=…&delete_files=false` and confirms
absence/cancelled state. Pause/resume remain unsupported because no verified protocol was found.
Progress percentage is independent from bytes; missing file size stays unknown, including completion.
Task names/messages that echo source links are not relayed. Durable no-replay intents belong to Core.

Resource preview sends POST `/drive/v1/resource/list {urls,page_size:500,thumbnail_type:"FROM_HASH"}`.
The observed response has `list.resources` (also supports root resources), `file_size` and
optional `dir.resources`. No verified subset-creation argument was found, so current preview
reports the whole resource and `selection_supported=false`; it does not offer fabricated selection.

Share creation validates accessible selected files, sends expiration_days -1/1/7/30 and
pass-code options, validates the returned public share URL and required code. A finite expiration
without service-reported timestamp remains unknown. Cancellation checks its remote asynchronous
task when present and completes the share list to verify inactive/absent entries.
Share browsing saves `pass_code_token` only in an epoch-bound 5-minute Rust-memory reference.
Folder pages retain scoped opaque cursors. Restoration uses service file IDs and waits for a real
task receipt and completion; no empty HTTP200 response is promoted to success.

Permanent deletion requires already-trashed metadata and a separate user-facing confirmation.
It invokes `files:batchDelete` then verifies each ID is NotFound. Ordinary files cannot use it.

Archive listing uses actual `gcid/file_id/path/password`, validates safe contained names,
and preserves password-required states. Unlock returns a random reference, with password held
in epoch-bound Rust memory for 15 minutes. Extraction resolves selected names back to service
indices. The only verified target contract is `default_parent=true`; choosing a different parent
is rejected until Core explicitly moves the completed output. Job polling uses the separate
decompress progress endpoint. Unknown status/unsafe names never become successful empty lists.

Uploads compute GCID as SHA1 of concatenated binary block digests, with service block sizes.
They request `UPLOAD_TYPE_RESUMABLE`, support confirmed instant completion, then OSS multipart
initiate/8MiB PUT parts/complete. OSS requests use only service-issued OSS authorization, never
PikPak API Bearer/CAPTCHA/device headers. Endpoint validation requires public HTTPS with no
redirect; loopback is only available in isolated debug tests. Part ETags are required.
Completion is confirmed by remote phase, exact size and GCID, not HTTP200.

Temporary OSS materials and upload ID are stored under an auxiliary Windows Credential Manager
namespace, indexed before each secret write. Core persists only the opaque upload reference,
file ID, local identity/hash, bytes and ETags. Logout/revocation clears the auxiliary index/items;
no plaintext fallback exists. Temporary authorization expiration does not create another upload.
Completion intent is persisted before OSS complete; after ambiguous result only a metadata check
is allowed. The vault receipt survives until Core persists completion and calls forget, so a crash
between remote completion and local commit can still recover.

No evidence was found for `GET file?usage=UPLOAD` refreshing OSS authorization; no such invented
endpoint was implemented. Long-running uploads can fail explicitly when temporary credentials expire.

## Actual checks

`cargo test -p nekopak-provider-native --locked --offline --lib`: 38 passed, 0 failed,
2 intentionally ignored. Includes native login/file tests plus cloud create/poll/retry identity,
share create/cancel and private token isolation/restore, permanent-delete trash restriction,
archive password/ref/extraction job, and real multipart bytes without API headers.

`cargo test -p nekopak-provider-native --locked --offline windows_vault_roundtrip_and_delete -- --ignored`:
1 passed. Only a fresh synthetic Credential Manager namespace was written, reopened, cleared and
deleted; user credentials were not read or enumerated.

`cargo clippy -p nekopak-provider-native --all-targets --locked --offline -- -D warnings`: passed.
Native protocol tests use synthetic local HTTP servers. They do not prove live service acceptance.

New dependencies: sha1 0.10.7, hmac 0.12.1 (MIT OR Apache-2.0), base64 0.22.1
(MIT OR Apache-2.0), httpdate 1.0.3 (MIT OR Apache-2.0). Exact lock and license inventory
remain release inputs. SHA1 is used for GCID/OSS protocol compatibility, not as a claimed secure
file integrity digest; Core retains SHA256 for local file-integrity checks.
