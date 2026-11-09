import { afterEach, expect, it } from "vitest";
import { app, fileGate, gate, hostSupports, isMobileHost } from "../../src/state/app";
import { statusForUi } from "../../src/lib/transport/rust-adapter";

const initial = app.get();
afterEach(() => app.replace(initial));

it("uses Rust host capabilities even when the provider advertises desktop operations", () => {
  const operations = { upload: true, download: true, external_player: true };
  app.set({ boot: "ready", runtime: "tauri", auth: { state: "signed_in", backend: "rust_compat", checked_at: "" }, caps: [{ backend: "rust_compat", status: "ready", operations, limitations: [], checked_at: "" }], app: statusForUi({ version: "test", contract_version: "0.2", mode: "rust_compat_preview", storage: { status: "ready", schema_version: 1 }, features: { mobile: true, upload: false, media_external: false, transfer_export: true, directory_picker: false, media_session: true } }) });
  expect(isMobileHost()).toBe(true);
  expect(gate("upload").ok).toBe(false);
  expect(gate("external_player").ok).toBe(false);
  expect(fileGate("external_player", "local:test:file").ok).toBe(false);
  expect(gate("download").ok).toBe(true);
  expect(hostSupports("directory_picker")).toBe(false);
  expect(app.get().app?.feature_flags.transfer_export).toBe(true);
});

it("preserves desktop capabilities when older hosts omit additive mobile flags", () => {
  app.set({ app: { version: "test", contract_version: "0.2", runtime: "tauri", storage: { status: "ok" }, feature_flags: {} } });
  expect(isMobileHost()).toBe(false);
  expect(hostSupports("desktop_window")).toBe(true);
  expect(hostSupports("directory_picker")).toBe(true);
});
