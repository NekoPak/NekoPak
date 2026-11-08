# v3 UI integration with the native API direction

2026-11-08. Source: user-supplied `nekopak-ui-v3-src.zip`, SHA-256
`86be86d3e7ab594ec73e2f10fac841b14966eb84121659e771c894df70b03e27`.
The supplied Preact UI, pure helpers and Mock tests were integrated; Rust wire types,
transport gating, precise revisions and durable snapshot reconciliation were retained.
The old v2 frontend report is historical; current checks are in API_MIGRATION_REPORT.md.

The desktop always uses Tauri IPC. Its native sign-in form calls auth_sign_in,
auth_verify and auth_logout. Configuration availability comes from the Rust feature
flags; an unconfigured build disables credential entry. All response paths clear the
password field; CAPTCHA continuation requests a new password. Browser Mock still
supports deterministic sign-in/challenge demos and labels them as demonstration data.

Native backend discovery returns rust_compat only; explicit Rust Mock returns mock
only. No connection selector, CLI status, installation flow or subprocess exists.
Legacy BackendId strings remain readable, not usable. Display-level backend names and
Connected Apps explanations are removed from ordinary product flows; technical notes
remain in developer documentation and notices.

The v3 import adds clip/quality selection, batch rename, duplicates, space analysis,
archives, shares, command palette, drag/drop, clipboard and undo interactions. These
are UI/Mock coverage, not claims of desktop backend support. The actual Rust handler
allowlist and capabilities disable unimplemented operations. Native metadata paths
are a read-only preview, with no live account success yet. Unsupported optional clip,
quality or selected-file arguments are explicitly rejected rather than ignored.

Existing task revisions use lossless decimal strings and BigInt comparisons, including
the new server-job reconciliation. Transfer snapshots subscribe/buffer/reload and
handle storage/resync events. Mock tasks remain labeled simulation; a real connection
cannot execute them as real transfers. Settings/sync/mount backends remain disabled.

Session transitions clear cloud pins/paths, clipboard, undo, dialogs, side panels and
old read results while keeping appearance preferences. Native startup clears legacy
unscoped pins; persistent account-scoped pins await an opaque account-identity contract.
Saved-session refresh failures have explicit retry and local-signout recovery controls.

The supplied FEATURE_PARITY.md proposed dropping several upstream features. Those
rows are now pending, not removed. UPSTREAM_PARITY.md and parity-manifest.json retain
the mandatory final product scope, including video segments, player/feed, archive
references, local-folder mapping, workspaces and languages.

Build output remains `ui-dist/`, separately from packaged files in `dist/`.
Browser E2E screenshots are written to `docs/screenshots/ui-v3/` to retain older
Windows evidence. No package-local historical test report substitutes for current tests.
