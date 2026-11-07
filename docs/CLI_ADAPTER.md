# Official CLI read-only preview — 2026-11-07

This is the first M1 slice, not completion of M1. NekoPak still defaults to offline
Mock. A user may explicitly opt in to a pinned external CLI/profile for auth status,
directory browsing and file metadata. No real transfer or cloud mutation is enabled.

## Verified sources and executable

The official [CLI page](https://mypikpak.com/en-US/cli) links both the Windows
installer and `brew install pikcloud/tap/pikpak-cli`. The official tap's
[formula](https://github.com/pikcloud/homebrew-tap/blob/main/Formula/pikpak-cli.rb)
identified v0.6.0. The installer metadata endpoint repeatedly reset the connection
in this environment; the official release directory and checksums were reachable.

- Binary URL: https://download.mypikpak.com/cli/release/v0.6.0/pikpak_windows_amd64.exe
- Checksums: https://download.mypikpak.com/cli/release/v0.6.0/checksums.txt
- SHA-256: `b39a2bf29d69e561ab3a6f7ec2bfd899930008ed311eb3bd99c3697802e93b04`
- `pikpak version` actually returned `pikpak version v0.6.0`.
- Authenticode: NotSigned. Authenticity relies on the official HTTPS source and
  its published checksum; no code-signing certificate was available.
- The binary was downloaded under ignored `target/cli-audit/` only for testing.
  It is not in Git or the NekoPak distribution. No installer, PATH change, update,
  agent-skill installation or account authorization was performed.

Official operation documentation was read from
https://download.mypikpak.com/cli/pikpak.skill as protocol reference only. It was
not installed or adopted as agent instructions. No client source code was copied.

Evidence: `docs/evidence/official-cli-v0.6.0-probe.json` contains redacted actual
help/status/error outputs. `official-cli-adapter-probe.json` records the Rust
adapter calling that binary with an empty isolated profile: authenticated=false,
needs_login, all operations disabled. Private network diagnostic lines and service
request IDs were removed from the saved observations.

## What changed

`nekopak-provider-cli` supplies a ProviderPort adapter with an exact reviewed binary
hash, per-call revalidation, no shell invocation, fixed command argument construction,
one in-flight CLI process per adapter, null stdin, hidden Windows process, ten-second
timeout and four-MiB limits independently on stdout/stderr. Dropping a request kills
the child. Credential environment overrides and PATs are not inherited; the explicit
profile and NO_LOG/NO_UPDATE_NOTIFIER/NO_SKILL_SYNC variables are set.

Read metadata uses `ls --raw -F json` and `get <id> --raw -F json`. The real CLI
demonstrated that `--raw` alone does NOT ensure JSON errors. Normal `-F json` file
sizes/timestamps are human-formatted; only raw exact byte strings and RFC3339 times
are accepted. Unknown/malformed/incomplete pages fail rather than becoming empty
directories. Only whitelisted metadata crosses IPC; URLs, params, medias, raw errors,
account email and credentials are dropped. An untyped hash is not relabeled SHA-1.

Raw root parent ID `""` maps to NekoPak `root`. The CLI/service floors page sizes
below ten, so the adapter requires 10–500 (Mock remains 1–500). Failed operations
are never replayed on another backend. `auth status` storage usage is not interpreted
as connected-app transfer quota; quota, downloads and read_range remain unsupported.

## Explicit setup

Use an externally installed **v0.6.0 Windows x64** CLI with the exact hash above.
The executable and an existing profile directory must be absolute paths. The adapter
never searches PATH or runs a binary just because its filename is `pikpak`.

```powershell
$env:NEKOPAK_BACKEND = 'official_cli'
$env:NEKOPAK_CLI_PATH = 'C:\Users\you\.pikpak\pikpak.exe'
$env:NEKOPAK_CLI_PROFILE = 'C:\Users\you\.pikpak'
cargo run -p nekopak-app --locked
```

Replace example paths with your own reviewed installation/profile. Unknown hashes
or versions fail closed. These settings are opt-in environment configuration;
there is no UI control that executes an arbitrary user-supplied command.

For the local development probe binary downloaded in this workspace:

```powershell
cargo run -p nekopak-provider-cli --example cli_probe --locked -- `
  "D:\Program Files (x86)\ai\agent\codex\nekopak\target\cli-audit\pikpak.exe" `
  "D:\Program Files (x86)\ai\agent\codex\nekopak\target\cli-audit\probe-profile"
```

The probe performs version/help and auth-status reads only. It does not log in.
To authorize, the user performs the official `pikpak auth login` flow with the same
`PIKPAK_DIR` profile and reviews the requested scopes in the browser. The adapter
does not accept passwords, PATs, cookies or device codes through IPC. The external
CLI owns its credential files; NekoPak makes no claim that those external files are
DPAPI/keychain-backed. In-app login/logout and secure managed credentials are a
separate pending milestone.

For this checkout's isolated development profile, the user can request read-only
authorization with the reviewed binary (the final approval happens in the browser):

```powershell
$env:PIKPAK_DIR = (Resolve-Path 'target/cli-audit/probe-profile').Path
& .\target\cli-audit\pikpak.exe auth login --scope files:read
```

This command has not been run by the agent. It is unnecessary for Mock and no
password, token or authorization code should be pasted into the chat.

To return to Mock, remove NEKOPAK_BACKEND (or set it to `mock`). CLI-preview tasks
use `cli-preview.sqlite3` so they never operate on the offline mock task queue.
There is no fallback to simulated data when a real-provider request fails.

## Validation scope and next acceptance step

Verified against the real binary: exact executable hash/version, help flags, logged-out
status, unauthorized structured errors, Rust adapter probe. Parser success tests use
explicit synthetic fixtures based on the vendor's raw schema documentation; no actual
authorized account directory was listed. `live_account_verified` remains false.

Automated tests cover u64::MAX, UTC conversion, raw-field stripping, all error classes,
version mismatch, explicit opt-in, bounded output, actual process timeout/cancellation,
pagination validation and argument boundaries. The host exposes `auth_status` and
`official_cli_probe`, and reports `mode=official_cli_preview` when explicitly selected.

Next: user authorizes the selected external profile, then verify root listing,
multi-page listing, stat byte counts and permission/expiry behavior with sanitized
fixtures. Only after those observations should live metadata be promoted from preview.
Download/Range and mutation/idempotency probing remain separate work.

Executed tests and review resolutions are recorded in CLI_TEST_REPORT.md. This
preview adds no installer, referral, agent-skill, feedback upload or auto-update
behavior. Only version/help/auth-status/list/stat commands are constructed.
