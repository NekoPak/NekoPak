# NekoPak — Opus 5.5 / Claude Project Context

**Read `AGENTS.md` and `README.md` before editing.** The full Opus 5.5 execution prompt lives in `prompts/OPUS_FRONTEND.md`. Technical contracts are in `docs/03_CONTRACTS.md`; UI specification is `docs/07_UI_SPEC.md`.

## Non-negotiable frontend boundaries

- UI frontend only. Rust Core/backend ownership belongs to Astra; request new IPC commands via contract proposal rather than improvising incompatible endpoints.
- Use Tauri v2 + minimal TypeScript/CSS; React/Vite is optional when justified. No heavyweight UI kit or large bundled image/font sets without measurement.
- Build a functional file explorer, transfer dashboard, dry-run sync and mount-capability UI against Mock; disable unimplemented real operations with clear reasons.
- At startup load state snapshot, then subscribe to progress events; never assume events alone are persistent truth.
- Keep sources/credits accurate. Piko is AGPL-3.0-or-later; do not copy Piko components into an Apache/MIT codebase. See `docs/04_SOURCES_LICENSES.md`.
- Network compatibility controls default off, are transparent to users, and never edit system Hosts.
- Do not pretend CLI exposes public REST API or mount random-read support.
- Every response should identify files changed, build commands run, test results, bundle size, known limitations and whether contract changed.

## Immediate task

Implement the M0 UI shell with mocked data and typed integration. Consult Astra for stable IPC contract after review. Do not redesign backend.
