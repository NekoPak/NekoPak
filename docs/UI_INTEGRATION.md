# UI M0 v2 integration

The supplied `NekoPak_UI_M0_v2.zip` was integrated into the workspace on
2026-11-07. It is a Preact + Vite frontend with its own browser Mock transport,
unit tests, and visual/E2E fixtures. No UI source code from Piko was copied;
its own source/asset note is preserved in `docs/UI_SOURCES_AND_DEPENDENCIES.md`.

The integrated desktop path uses `src/lib/transport/tauri.ts` and a small
`src/lib/transport/rust-adapter.ts` mapping the Rust v0.1 wire response into
display-only proposal types. Rust remains the source of truth. Browser preview
uses the supplied Mock implementation. The frontend cannot silently fall back
to browser Mock inside Tauri.

The original Rust IPC contract remains authoritative. Implemented desktop
commands are limited to the actual handlers: status, backend discovery, auth
status, file list/stat/path, transfers, transfer control, cloud task create and
quota. The supplied UI commands for mutations, settings, sync, mount, search,
recent, trash, auth flows, upload, playback and segment download are disabled or
shown as unavailable in Tauri mode. Their browser Mock behavior is for UI
development only. This avoids a decorative control that appears to change the
cloud when Rust has no handler.

The desktop UI now:

- uses a dedicated left-sidebar `任务` navigation item instead of placing tasks
  beside `文件` as a workspace toggle;
- labels the unauthenticated Rust Mock state as “未连接 PikPak / 离线界面预览”;
- keeps Mock capability diagnostics under `设置 → 开发者`, while `设置 → 连接`
  shows only real connection candidates and never calls Mock “installed”;
- displays the official CLI as a supported connection path whose binary/profile
  is externally configured, rather than treating it as optional product scope;
- uses BigInt-compatible string revisions and preserves structured progress errors;
- shows separate disabled reasons for sync, mount, settings, segment download and
  other unimplemented Rust commands.

Build the UI with `npm run build`; output is `ui-dist/`, which Tauri consumes.
The old `shell/` acceptance page remains in the repository as a tiny reference
fixture but is no longer the Tauri frontend. The CI workflow builds the frontend
before the Rust desktop build. Packaging checks frontend assets and license texts
for Preact, Tauri API, Material Symbols and color utilities.

## Validation

Local checks after integration:

- `npm run check`: passed.
- `npm test`: 43 Vitest tests plus 6 wire contract tests passed.
- `npm run build`: passed; generated `ui-dist/` assets.
- `cargo test --workspace --locked --offline`: passed; existing Tauri IPC tests,
  core recovery tests, 13 CLI adapter tests and media-core tests remain green.
- `cargo fmt --all --check`: passed.
- A real Windows WebView2 development process was started in an isolated data
  directory with a loopback CDP endpoint. It loaded the integrated Rust Mock
  directory and returned eight Rust entries, including the exact u64::MAX file;
  a screenshot is retained as `docs/screenshots/desktop-rust-files-light.png`.
  No new window or process is launched by this integration after that check.
- `tests/e2e/desktop.mjs` is an opt-in loopback test for an explicitly launched
  isolated development app. It never launches a process, never logs in, and never
  sends data. `tests/e2e/run.mjs` remains the browser Mock suite; one earlier run
  had 31/32 passes because an auth-failure assertion timed out after the UI copy
  changed. The desktop app was not launched to hide or bypass that failure.

The supplied screenshot/report files are reference evidence from the UI package,
not claims that its proposed backend commands are implemented. Current product
parity status remains governed by `docs/parity-manifest.json`; the final release
gate still fails until upstream capabilities are genuinely implemented.
