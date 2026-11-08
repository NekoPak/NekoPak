import { afterEach, expect, it, vi } from "vitest";
import type { ReferenceManifest } from "../../src/lib/contract-proposals";
import { referenceActions, createReferenceArchive } from "../../src/pages/References";
import { api } from "../../src/lib/api";
import { app } from "../../src/state/app";

const base: ReferenceManifest = { manifest_id: "manifest", account_id: "account", created_at: "2026-11-08T00:00:00Z", content_sha256: "a".repeat(64), state: "prepared", roots: ["file"], entries: [{ path: "file", entry: { id: "file", name: "file", kind: "file", size_bytes: "12" }, state: "recorded" }] };
const previous = app.get();
afterEach(() => { vi.restoreAllMocks(); app.replace(previous); });

it("never offers trash before the manifest is verified and requires a failed restore before GCID recreation", () => {
  expect(referenceActions(base)).toEqual({ store: true, archive: false, restore: false, recreate: false });
  expect(referenceActions({ ...base, state: "uploading" }).archive).toBe(false);
  expect(referenceActions({ ...base, state: "verified" }).archive).toBe(true);
  expect(referenceActions({ ...base, state: "archived" }).restore).toBe(true);
  const partial = { ...base, state: "partial" as const, entries: [{ ...base.entries[0], state: "failed" as const }] };
  expect(referenceActions(partial).recreate).toBe(false);
  expect(referenceActions(partial, true).recreate).toBe(true);
  expect(referenceActions({ ...partial, entries: [{ ...base.entries[0], state: "outcome_unknown" }] }, true).recreate).toBe(false);
});

it("does not create an archive from local or shared handles even when native archiving is enabled", async () => {
  const create = vi.spyOn(api, "referenceCreate");
  app.set({ boot: "ready", runtime: "mock", auth: { state: "signed_in", backend: "mock", checked_at: "" }, caps: [{ backend: "mock", status: "ready", operations: { stat: true }, limitations: [], checked_at: "" }], app: { version: "test", contract_version: "0.2", runtime: "tauri", storage: { status: "ok" }, feature_flags: { reference_archive: true } } });
  for (const id of ["local:source:file", "shared:reference:file"]) await createReferenceArchive([{ ...base.entries[0].entry, id }]);
  expect(create).not.toHaveBeenCalled();
});
