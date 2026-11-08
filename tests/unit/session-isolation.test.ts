import { afterEach, expect, it, vi } from "vitest";
import { prefs, side } from "../../src/state/prefs";
import { clip, setClip } from "../../src/state/clipboard";
import { pushUndo, undoLog } from "../../src/state/undo";
import { dialogs, openDialog, menu, toasts } from "../../src/state/ui";
import { palette } from "../../src/state/palette";
import { route } from "../../src/state/route";
import { resetSessionUi, sessionGeneration } from "../../src/state/session";

afterEach(() => { resetSessionUi(); vi.unstubAllGlobals(); });

it("account resets clear cloud metadata and pending actions while retaining appearance", async () => {
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", { setItem: (k: string, v: string) => storage.set(k, v) });
  const before = sessionGeneration();
  prefs.set({ accent: "mist", density: "compact", pins: [{ id: "account-a-folder", name: "Account A private" }] });
  setClip("cut", [{ id: "a-file", name: "Private.txt", kind: "file", size_bytes: "12" }]);
  pushUndo("private rename", async () => true);
  side.set({ panel: { key: "private", title: "Private.txt", render: () => null } });
  menu.set({ open: true, items: [{ id: "private", label: "Private.txt" }] });
  palette.set({ open: true, seed: "private account query" });
  route.set({ current: { page: "search", query: "private account query" } });
  toasts.set({ items: [{ id: 1, tone: "info", text: "Private.txt" }] });
  const pending = openDialog({ title: "Private.txt", render: () => null });
  resetSessionUi();
  expect(sessionGeneration()).toBe(before + 1);
  expect(prefs.get().pins).toEqual([]);
  expect(prefs.get().density).toBe("compact");
  expect(storage.get("nekopak.prefs.v2")).not.toContain("Account A");
  expect(clip.get().entries).toEqual([]);
  expect(undoLog.get().entries).toEqual([]);
  expect(side.get().panel).toBeNull();
  expect(dialogs.get().stack).toEqual([]);
  expect(menu.get().items).toEqual([]);
  expect(palette.get()).toEqual({ open: false, seed: "" });
  expect(route.get().current).toEqual({ page: "files", folder: "root" });
  expect(toasts.get().items).toEqual([]);
  await expect(pending).resolves.toBeUndefined();
});
