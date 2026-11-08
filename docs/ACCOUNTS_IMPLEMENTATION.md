# Local multi-account sessions

2026-11-08. The native provider implements saved-account listing, adding through
native password sign-in, switching and removal. This is an independent local
session implementation; it does not add a remote account-management protocol,
automatic quota rotation, CLI adapter or third-party source code.

The shared contract is `SavedAccount { id, display_name, active, state }`,
`accounts_list()`, `account_switch({id}) -> AuthState` and
`account_remove({id}) -> void`. `AuthState.account_id` is optional for backward
compatibility and is present when a native session is active. All public names
are masked, including legacy records. Account identifiers are opaque local
digests. Neither passwords, remote subjects nor session tokens cross IPC.

The original per-data-directory/client-ID credential target remains the active
session. New protected index and inactive-session entries are located under that
same Windows Credential Manager target. Existing sessions migrate in place;
the full original session remains intact until its replacement write succeeds.
The protected account index is bounded to eight accounts to fit Windows'
credential blob limit. Unsupported OS credential stores do not fall back to
plaintext files. Upload entries retain their original credential namespace.

An account is archived before another session replaces the active credential.
An index reservation precedes every new credential write. The final active
credential write is the activation point; memory and the signed-in event are
updated synchronously afterward. Failed or cancelled sign-in, failed refresh of
an inactive account, and pending CAPTCHA leave the previous active account in
place. A fresh request supplies the password after CAPTCHA. A token response
without a subject is checked through the existing authenticated user endpoint
before a new account is saved. The remote-account scope used by existing
transfers is unchanged.

Successful sign-in and switching increment the session epoch and invalidate
pagination, share, archive and CAPTCHA handles. Delayed responses from an old
epoch fail. Every successful activation emits `signed_in`, including transitions
from an already signed-in account; the frontend compares `account_id` and resets
its account-specific state. `account_scope_for(id)` verifies an expected account
ID while holding the session lock before returning its stable storage scope.
This supports rejecting delayed preference writes belonging to a prior account.

Logout removes only the active account and does not choose a replacement.
Removal records a protected tombstone before deleting any session and resumes
cleanup after restart. A partial failure cannot silently resurrect that account.
Upload credentials are removed only when their protected owner scope matches
the removed account; other saved accounts and their resumable uploads remain.
Corrupt manifests and missing archived credentials return an integrity error
without resetting the vault. No remote user files are deleted by these actions.

Verification on Windows x64:

- `cargo test -p nekopak-provider-native --locked`: 48 passed, 0 failed, 2 ignored.
- `cargo clippy -p nekopak-provider-native --all-targets --locked -- -D warnings`:
  passed.
- `cargo check -p nekopak-core -p nekopak-provider-native --locked`: passed.
- `cargo test -p nekopak-core --test ui_prefs --locked`: 2 passed. The tests use
  a memory provider to verify isolated persisted preferences and reject delayed
  writes with the previous or an unknown account ID.
- Ten new account tests use only synthetic memory vaults and loopback HTTP.
  They cover same masked names, add/switch failures, preserved old sessions
  during CAPTCHA and refresh, credential migration/corruption, restart recovery,
  scoped upload deletion, late metadata rejection, token rotation and sign-in
  events. The existing native authentication, pagination, upload, download,
  archive and share tests also run.

The ignored live anonymous CAPTCHA probe and Windows credential smoke test were
not run. No real account credentials were read, requested or tested. Real account
acceptance, desktop account UI integration and full-client parity remain separate
verification responsibilities and are not implied by these provider tests.
