import { afterEach, expect, it } from "vitest";
import { api } from "../../src/lib/api";
import { app, fileGate, gate } from "../../src/state/app";
import { route } from "../../src/state/route";

const initial = app.get();
const initialRoute = route.get();
afterEach(() => { app.replace(initial); route.replace(initialRoute); });

it("allows local browsing and playback while signed out without borrowing cloud capabilities", () => {
  app.set({ boot: "ready", auth: { state: "signed_out", checked_at: "" }, caps: [], app: {
    version: "test", contract_version: "0.2", runtime: "tauri", storage: { status: "ok" },
    feature_flags: { local_sources: true, media_session: true, local_media_probe: true },
  } });
  for (const operation of ["list", "stat", "media_probe", "external_player"] as const) {
    expect(fileGate(operation, "local:source:movie.mp4").ok).toBe(true);
    expect(gate(operation).ok).toBe(false);
  }
  for (const operation of ["rename", "move", "copy", "trash", "upload", "share", "cloud_task", "clip_download"] as const) {
    expect(fileGate(operation, "local:source:movie.mp4").ok).toBe(false);
  }
});

it("selects permissions by source rather than whichever tab happens to be active", () => {
  app.set({ boot: "ready", runtime: "mock", auth: { state: "signed_in", backend: "mock", checked_at: "" }, caps: [{
    backend: "mock", status: "ready", operations: { rename: true, copy: true }, limitations: [], checked_at: "",
  }] });
  route.set({ current: { page: "files", folder: "local:source:" } });
  expect(fileGate("rename", "remote-file").ok).toBe(true);
  route.set({ current: { page: "files", folder: "root" } });
  expect(fileGate("rename", "local:source:movie.mp4").ok).toBe(false);
  expect(fileGate("copy", ["remote-file", "local:source:"]).ok).toBe(false);
  expect(fileGate("rename", "share:access:file").ok).toBe(false);
});

it("rejects non-drive handles before any mutation transport invocation", async () => {
  const denied = { error: { code: "PERMISSION_DENIED" } };
  await expect(api.filesMutate("rename", ["local:source:file"], { name: "new" }, "key")).rejects.toMatchObject(denied);
  await expect(api.filesMutate("copy", ["remote-file"], { parent_id: "local:source:" }, "key")).rejects.toMatchObject(denied);
  await expect(api.filesMutate("trash", ["shared:access:file"], undefined, "key")).rejects.toMatchObject(denied);
  await expect(api.shareCreate(["local:source:file"], 7, true, "key")).rejects.toMatchObject(denied);
  await expect(api.cloudTaskCreate("https://example.com/file", "local:source:", "key")).rejects.toMatchObject(denied);
  await expect(api.shareRestore("access", ["shared-file"], "local:source:", "key")).rejects.toMatchObject(denied);
  await expect(api.uploadDirectoryPickAndStart("local:source:")).rejects.toMatchObject(denied);
  await expect(api.uploadDirectoryPickAndStart("shared:access:folder")).rejects.toMatchObject(denied);
});

it("permits only verified original shared-file reads and preserves capability and quota restrictions", () => {
  app.set({ boot: "ready", runtime: "tauri", auth: { state: "signed_in", backend: "rust_compat", checked_at: "" }, caps: [{
    backend: "rust_compat", status: "ready", operations: { ...{ share: true, stat: true, download: true, media_probe: true, external_player: true, quality_download: true, clip_download: true } }, limitations: [], checked_at: "",
  }], app: { version: "test", contract_version: "0.2", mode: "rust_compat_preview", runtime: "tauri", storage: { status: "ok" }, feature_flags: { share_preview: true, media_session: true } } });
  for (const operation of ["stat", "download", "media_probe", "stream_url", "external_player"] as const) expect(fileGate(operation, "shared:opaque:file").ok).toBe(true);
  for (const operation of ["rename", "move", "copy", "trash", "star", "share", "upload", "folder_download", "quality_download", "clip_download"] as const) expect(fileGate(operation, "shared:opaque:file").ok).toBe(false);
  app.set({ quota: { backend: "rust_compat", account_tier: "unknown", checked_at: "", items: [{ dimension: "downstream_daily", scope: "account", unit: "bytes", remaining: "0", exactness: "exact", state: "exhausted" }] } });
  expect(fileGate("download", "shared:opaque:file").ok).toBe(false);
  app.set({ quota: undefined, app: { ...app.get().app!, feature_flags: { media_session: true } } });
  expect(fileGate("download", "shared:opaque:file").ok).toBe(false);
});
