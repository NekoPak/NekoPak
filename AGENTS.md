# NekoPak Agent Instructions

Applies to the entire NekoPak repository. Read `README.md` first, then read all relevant files under `docs/` before modifying code. This project is still in planning/pre-alpha; do not claim features are implemented unless actually built and tested.

## Project intent

Build a lightweight Rust-powered, Tauri v2 desktop PikPak client. Windows x64 first. P0: safe login/session, files, cloud download tasks, crash-safe transfer queue; P1: one-way then two-way safe sync; P2: optional mount/Cloud Files. The ≤5 MiB Windows compressed Lite package is a stretch target, not grounds to disable TLS, integrity checks, or reliable storage.

## Mandatory design constraints

1. **License gate:** Read `docs/04_SOURCES_LICENSES.md`; do not copy code from AGPL Piko into an independent Apache/MIT Rust repository. If building a true Piko fork/derivative, preserve applicable AGPL obligations. Record upstream SHA, original file paths and modifications.
2. **Direct API only (2026-11-08):** Remove the official CLI architecture entirely: no adapter, probe, install flow, subprocess, or fallback. Primary connection is Rust `rust_compat` with native sign-in. Keep `official_cli` only as a legacy serialized BackendId, never discover/register it. Do not call community client protocols official public APIs; verify exact behavior before enabling capabilities. See `docs/API_ADAPTER.md` and `docs/BACKEND_API_ADAPTER.md`.
3. **Contract-first:** `docs/03_CONTRACTS.md` is the backend/frontend shared contract. Coordinate breaking changes, fixtures, and errors with both agents.
4. **One source of truth:** Rust Core owns tasks, network, sync, quota policy, and persistence; frontend only owns display/input and typed IPC.
5. **Safety first:** No default deletion in sync, no silent reset on corrupt task DB, no secret leakage, no blind retry of side-effecting remote requests.
6. **Install footprint:** No bundled JVM, Chromium, mpv/FFmpeg, Rclone, WinFsp, or official CLI in Lite. Optional features must declare prerequisites accurately.
7. **Mount semantics:** Cloud Files sync root is distinct from WinFsp drive letter. Mount requires actually verified random reads; otherwise disable it.
8. **Experimental networking:** Default off. Never silently edit system Hosts, bypass account controls, or secretly change global OS settings.
9. **Test honestly:** Execute relevant tests and provide commands and results. If a platform build is unavailable, report the limitation.
10. **No false UI:** Do not create decorative buttons that appear functional. Unimplemented functionality must be disabled and labeled.
11. **Upstream functional parity (user requirement, 2026-11-07):** Final NekoPak must not lose published upstream functions, including video time-segment download. Follow `docs/UPSTREAM_PARITY.md` and `docs/parity-manifest.json`; M0/M1 previews are intermediate, not parity completion. Priorities and Lite size limits do not remove requirements. Review current upstream stable releases before declaring parity.
12. **Third-party APIs are the primary route:** Research and independently adapt community/third-party client protocols. Audit source licenses, authorization, exact behavior and actual tests; do not copy secrets, bypass controls or relabel private protocols as official APIs. WebDAV/Rclone are optional sync/mount extensions, not normal file-browser/download backends. Passwords are request-only; persist sessions in the OS credential vault, never plaintext. Imported UI reports do not remove upstream parity requirements.
13. **Live account testing belongs to the user (2026-11-08):** The agent prepares a genuinely testable native login build, completes protocol/client configuration and offline checks, and fixes defects. The user enters their own credentials and completes verification in NekoPak. Do not ask for passwords/tokens in chat or treat missing developer-side configuration as work for the user. Missing live acceptance is a verification status, not a reason to defer otherwise independent implementation work.

## Role assignment

- **Astra:** `crates/*`, `src-tauri`, backend, persistence, build CI, Rust tests, compliance audit. Use `prompts/ASTRA_BACKEND.md`.
- **Opus 5.5:** `src`, frontend, CSS, interaction, mock fixtures and IPC client; avoid changing Rust contracts without coordination. Use `prompts/OPUS_FRONTEND.md` and `docs/07_UI_SPEC.md`.

## First iteration

Complete Gate 0 then M0 from `docs/05_ROADMAP_AND_TESTS.md`: runnable Tauri mock flow, persistent task schema, API types, file explorer UI, tasks UI, capability-driven disabled states. Finish with changed files, real test results, unimplemented pieces, license impact and next actionable issue.

## Platform and release direction (2026-11-08)

The owner confirmed Windows desktop stability first, then Linux/macOS and Android adaptation.
Android is an explicit future target, not a removed requirement. Release repository:
https://github.com/NekoPak/NikoPak. Keep signing private keys in protected local backups and
GitHub Actions Secrets, never source control. Actions should build and verify tagged releases;
Tauri update signatures, Windows Authenticode, and Android signing are distinct credentials.
