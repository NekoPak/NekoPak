import { afterEach, expect, it } from "vitest";
import { activeTab, closeTab, cycleTab, MAX_TABS, navigate, navigateHistory, openTab, route, selectTab, setTabTitle, tabTitle, workspaceTabs } from "../../src/state/route";
import { resetSessionUi } from "../../src/state/session";

afterEach(() => { route.set({ current: { page: "files", folder: "root" } }); resetSessionUi(); });
it("keeps independent per-tab history and titles", () => {
  resetSessionUi(); const first = activeTab().id;
  navigate({ page: "files", folder: "folder-a" });
  setTabTitle({ page: "files", folder: "folder-a" }, "Private folder A");
  expect(tabTitle(activeTab())).toBe("Private folder A");
  openTab({ page: "files", folder: "folder-b" }, "Private folder B");
  const second = activeTab().id;
  navigate({ page: "starred" }); navigateHistory(-1);
  expect(route.get().current).toEqual({ page: "files", folder: "folder-b" });
  selectTab(first);
  expect(route.get().current).toEqual({ page: "files", folder: "folder-a" });
  expect(tabTitle(activeTab())).toBe("Private folder A");
  navigateHistory(-1); expect(route.get().current).toEqual({ page: "files", folder: "root" });
  navigateHistory(1); expect(route.get().current).toEqual({ page: "files", folder: "folder-a" });
  closeTab(first); expect(activeTab().id).toBe(second);
});
it("bounds tabs and clears every private route on account reset", () => {
  resetSessionUi(); for (let i = 0; i < MAX_TABS; i++) openTab({ page: "search", query: `private-${i}` });
  expect(workspaceTabs.get().items).toHaveLength(MAX_TABS);
  expect(openTab()).toBe(false);
  const before = activeTab().id; cycleTab(1); expect(activeTab().id).not.toBe(before);
  resetSessionUi();
  expect(workspaceTabs.get().items).toHaveLength(1);
  expect(JSON.stringify(workspaceTabs.get())).not.toContain("private-");
  expect(route.get().current).toEqual({ page: "files", folder: "root" });
});
