import { expect, it, vi } from "vitest";
import { api } from "../../src/lib/api";
import { resetSessionUi } from "../../src/state/session";

it("does not send an old mutation after awaiting transport initialization across a session reset", async () => {
  const invoke = vi.fn();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: { invoke } });
  try {
    const pending = api.filesMutate("trash", ["private-file-a"], undefined, "intent-a");
    resetSessionUi();
    await expect(pending).rejects.toMatchObject({ error: { code: "CANCELLED" } });
    expect(invoke).not.toHaveBeenCalled();
  } finally { vi.unstubAllGlobals(); }
});
