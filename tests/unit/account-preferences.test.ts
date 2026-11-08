import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { api } from "../../src/lib/api";
import { configureNativePreferences, flushAccountPreferences, loadAccountPreferences, pauseAccountPreferences, prefs, setPref } from "../../src/state/prefs";
import { resetSessionUi } from "../../src/state/session";

let stored: Map<string, string>;
beforeEach(() => {
  stored = new Map();
  vi.stubGlobal("localStorage", { getItem: (key: string) => stored.get(key), setItem: (key: string, value: string) => stored.set(key, value) });
  vi.stubGlobal("document", { documentElement: { setAttribute: () => {}, toggleAttribute: () => {} } });
  configureNativePreferences(true);
});
afterEach(async () => {
  await flushAccountPreferences();
  configureNativePreferences(false); resetSessionUi();
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});

it("restores scoped pins and sorting from Rust while keeping cloud metadata out of localStorage", async () => {
  vi.spyOn(api, "uiPrefsGet").mockResolvedValue({ account_id: "account-a", prefs: { pins: [{ id: "private-a", name: "Private account folder" }], sort: { key: "modified", dir: "desc" }, density: "compact" } });
  const save = vi.spyOn(api, "uiPrefsUpdate").mockImplementation(async value => value);
  await loadAccountPreferences();
  expect(prefs.get().pins[0].id).toBe("private-a");
  expect(prefs.get().sort).toEqual({ key: "modified", dir: "desc" });
  setPref("view", "grid"); await flushAccountPreferences();
  expect(save.mock.calls[0][0]).toMatchObject({ pins: [{ id: "private-a", name: "Private account folder" }], view: "grid" });
  expect(save.mock.calls[0][1]).toBe("account-a");
  const local = JSON.parse(stored.get("nekopak.prefs.v2")!);
  expect(local.pins).toBeUndefined(); expect(local.sort).toBeUndefined();
  expect(stored.get("nekopak.prefs.v2")).not.toContain("Private");
});

it("serializes pending preference writes and drops queued data when the account changes", async () => {
  vi.spyOn(api, "uiPrefsGet").mockResolvedValue({ account_id: "account-a", prefs: { pins: [] } });
  await loadAccountPreferences();
  let finish!: () => void;
  const wait = new Promise<void>(resolve => { finish = resolve; });
  const save = vi.spyOn(api, "uiPrefsUpdate").mockImplementation(async value => { await wait; return value; });
  setPref("pins", [{ id: "account-a", name: "A" }]);
  await Promise.resolve();
  setPref("pins", [{ id: "account-a", name: "A" }, { id: "account-a-2", name: "A2" }]);
  resetSessionUi();
  finish(); await flushAccountPreferences();
  expect(save).toHaveBeenCalledTimes(1);
  expect(prefs.get().pins).toEqual([]);
  expect(prefs.get().sort).toEqual({ key: "name", dir: "asc" });
});

it("rejects late reads from the previous account and blocks writes during account switching", async () => {
  let finish!: (value: Awaited<ReturnType<typeof api.uiPrefsGet>>) => void;
  const read = new Promise<Awaited<ReturnType<typeof api.uiPrefsGet>>>(resolve => { finish = resolve; });
  const get = vi.spyOn(api, "uiPrefsGet").mockReturnValueOnce(read).mockResolvedValue({ account_id: "account-b", prefs: { pins: [] } });
  const oldRead = loadAccountPreferences(); resetSessionUi();
  finish({ account_id: "account-a", prefs: { pins: [{ id: "old-secret", name: "Old account" }] } });
  await oldRead; await loadAccountPreferences();
  expect(get).toHaveBeenCalledTimes(2); expect(prefs.get().pins).toEqual([]);
  const save = vi.spyOn(api, "uiPrefsUpdate").mockImplementation(async value => value);
  const resume = await pauseAccountPreferences();
  setPref("pins", [{ id: "blocked", name: "blocked" }]);
  expect(prefs.get().pins).toEqual([]); expect(save).not.toHaveBeenCalled();
  resume();
  setPref("pins", [{ id: "new", name: "New account" }]); await flushAccountPreferences();
  expect(save).toHaveBeenCalledTimes(1);
  expect(save.mock.calls[0][1]).toBe("account-b");
});
