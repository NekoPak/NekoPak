import { ROOT_ID } from "../lib/contract-proposals";
import { Store } from "../lib/store";
import { onSessionReset } from "./session";
import { tr } from "../lib/i18n";

export type Route =
  | { page: "files"; folder: string }
  | { page: "search"; query: string }
  | { page: "recent" }
  | { page: "transfers"; tab: "active" | "done" | "failed" }
  | { page: "cloud" }
  | { page: "sync"; config?: string }
  | { page: "mount" }
  | { page: "trash" }
  | { page: "starred" }
  | { page: "shares" }
  | { page: "references"; manifest?: string }
  | { page: "local" }
  | { page: "gallery"; folder: string; mode: "images" | "videos" }
  | { page: "duplicates"; folder: string }
  | { page: "space"; folder: string }
  | { page: "archive"; file: string; path: string }
  | { page: "settings"; section: SettingsSection }
  | { page: "about" };
// Local source ids are minted by Rust; the UI never constructs filesystem paths.

export type SettingsSection = "general" | "account" | "backends" | "quota" | "transfers" | "network" | "shortcuts" | "developer" | "about";

const SECTIONS: SettingsSection[] = ["general", "account", "backends", "quota", "transfers", "network", "shortcuts", "developer", "about"];

export function parseHash(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  });
  switch (parts[0]) {
    case "files": return { page: "files", folder: parts[1] || ROOT_ID };
    case "search": return { page: "search", query: parts.slice(1).join("/") };
    case "recent": return { page: "recent" };
    case "transfers": {
      const tab = parts[1] === "done" || parts[1] === "failed" ? parts[1] : "active";
      return { page: "transfers", tab };
    }
    case "cloud": return { page: "cloud" };
    case "sync": return parts[1] ? { page: "sync", config: parts[1] } : { page: "sync" };
    case "mount": return { page: "mount" };
    case "trash": return { page: "trash" };
    case "starred": return { page: "starred" };
    case "shares": return { page: "shares" };
    case "references": return { page: "references", ...(parts[1] ? { manifest: parts[1] } : {}) };
    case "local": return { page: "local" };
    case "gallery": return { page: "gallery", folder: parts[1] || ROOT_ID, mode: parts[2] === "videos" ? "videos" : "images" };
    case "duplicates": return { page: "duplicates", folder: parts[1] || ROOT_ID };
    case "space": return { page: "space", folder: parts[1] || ROOT_ID };
    case "archive": return parts[1] ? { page: "archive", file: parts[1], path: parts.slice(2).join("/") } : { page: "files", folder: ROOT_ID };
    case "settings": {
      const s = SECTIONS.includes(parts[1] as SettingsSection) ? (parts[1] as SettingsSection) : "general";
      return { page: "settings", section: s };
    }
    case "about": return { page: "about" };
    default: return { page: "files", folder: ROOT_ID };
  }
}

export function routeHash(r: Route): string {
  switch (r.page) {
    case "files": return r.folder === ROOT_ID ? "#/files" : `#/files/${encodeURIComponent(r.folder)}`;
    case "search": return `#/search/${encodeURIComponent(r.query)}`;
    case "transfers": return r.tab === "active" ? "#/transfers" : `#/transfers/${r.tab}`;
    case "sync": return r.config ? `#/sync/${encodeURIComponent(r.config)}` : "#/sync";
    case "references": return r.manifest ? `#/references/${encodeURIComponent(r.manifest)}` : "#/references";
    case "duplicates": return `#/duplicates/${encodeURIComponent(r.folder)}`;
    case "space": return r.folder === ROOT_ID ? "#/space" : `#/space/${encodeURIComponent(r.folder)}`;
    case "archive": return `#/archive/${encodeURIComponent(r.file)}${r.path ? `/${r.path.split("/").map(encodeURIComponent).join("/")}` : ""}`;
    case "gallery": return `#/gallery/${encodeURIComponent(r.folder)}/${r.mode}`;
    case "settings": return r.section === "general" ? "#/settings" : `#/settings/${r.section}`;
    default: return `#/${r.page}`;
  }
}

export const route = new Store<{ current: Route }>({ current: parseHash(globalThis.location?.hash ?? "") });
export interface WorkspaceTab { id: string; history: Route[]; at: number; title?: string; titleRoute?: string }
let tabSequence = 1;
const initialTab = (): WorkspaceTab => ({ id: String(tabSequence++), history: [route.get().current], at: 0 });
const firstTab = initialTab();
/** Navigation metadata is session-only. It is never reused across account changes. */
export const workspaceTabs = new Store<{ items: WorkspaceTab[]; activeId: string }>({ items: [firstTab], activeId: firstTab.id });
export const MAX_TABS = 12;
export function activeTab(): WorkspaceTab { const s = workspaceTabs.get(); return s.items.find(t => t.id === s.activeId) ?? s.items[0]; }

function writeHash(r: Route, replace: boolean): void {
  if (typeof history === "undefined" || typeof location === "undefined") return;
  const h = routeHash(r);
  if (replace) history.replaceState(null, "", h);
  else if (location.hash !== h) location.hash = h;
}
function recordNavigation(r: Route, replace: boolean): void {
  const current = activeTab();
  if (routeHash(current.history[current.at]) === routeHash(r)) return;
  const history = replace ? [...current.history] : current.history.slice(0, current.at + 1);
  if (replace) history[current.at] = r; else history.push(r);
  // Bounded session memory; dropping old history never changes remote data.
  if (history.length > 100) history.shift();
  const next = { ...current, history, at: replace ? current.at : history.length - 1, title: undefined, titleRoute: undefined };
  workspaceTabs.set(s => ({ items: s.items.map(t => t.id === next.id ? next : t) }));
}
export function openTab(r: Route = { page: "files", folder: ROOT_ID }, title?: string): boolean {
  const s = workspaceTabs.get();
  if (s.items.length >= MAX_TABS) return false;
  const tab: WorkspaceTab = { id: String(tabSequence++), history: [r], at: 0, title, titleRoute: title ? routeHash(r) : undefined };
  workspaceTabs.set({ items: [...s.items, tab], activeId: tab.id });
  writeHash(r, true); route.set({ current: r });
  return true;
}
export function selectTab(id: string): void {
  const next = workspaceTabs.get().items.find(t => t.id === id);
  if (!next) return;
  workspaceTabs.set({ activeId: id });
  writeHash(next.history[next.at], true); route.set({ current: next.history[next.at] });
}
export function closeTab(id = activeTab().id): void {
  const s = workspaceTabs.get(), at = s.items.findIndex(t => t.id === id);
  if (at < 0) return;
  if (s.items.length === 1) { navigate({ page: "files", folder: ROOT_ID }, true); return; }
  const items = s.items.filter(t => t.id !== id);
  workspaceTabs.set({ items });
  if (s.activeId === id) selectTab(items[Math.min(at, items.length - 1)].id);
}
export function cycleTab(direction: 1 | -1): void {
  const s = workspaceTabs.get(), at = s.items.findIndex(t => t.id === s.activeId);
  selectTab(s.items[(at + direction + s.items.length) % s.items.length].id);
}
export function navigateHistory(direction: 1 | -1): void {
  const tab = activeTab(), at = tab.at + direction;
  if (at < 0 || at >= tab.history.length) return;
  workspaceTabs.set(s => ({ items: s.items.map(t => t.id === tab.id ? { ...t, at, title: undefined, titleRoute: undefined } : t) }));
  writeHash(tab.history[at], true); route.set({ current: tab.history[at] });
}
export function setTabTitle(r: Route, title: string): void {
  const key = routeHash(r), tab = activeTab();
  if (key !== routeHash(tab.history[tab.at]) || (tab.title === title && tab.titleRoute === key)) return;
  workspaceTabs.set(s => ({ items: s.items.map(t => t.id === tab.id ? { ...t, title, titleRoute: key } : t) }));
}
export function tabTitle(tab: WorkspaceTab): string {
  const r = tab.history[tab.at];
  if (tab.title && tab.titleRoute === routeHash(r)) return tab.title;
  const label: Record<Route["page"], string> = { files: "网盘", search: "搜索", recent: "最近", transfers: "下载", cloud: "云下载", sync: "自动同步", mount: "挂载", trash: "回收站", starred: "星标", shares: "我的分享", references: "引用归档", local: "本地文件夹", gallery: "媒体库", duplicates: "查找重复", space: "空间分布", archive: "压缩包", settings: "设置", about: "关于" };
  return r.page === "search" ? `${tr(label[r.page])}: ${r.query}` : tr(label[r.page]);
}
onSessionReset(() => {
  const r = route.get().current;
  const keep: Route = r.page === "settings" || r.page === "about" ? r : { page: "files", folder: ROOT_ID };
  route.set({ current: keep });
  const tab = initialTab(); workspaceTabs.set({ items: [tab], activeId: tab.id });
  writeHash(keep, true);
});

export function navigate(r: Route, replace = false): void {
  recordNavigation(r, replace);
  writeHash(r, replace);
  route.set({ current: r });
}

export function initRouter(): void {
  window.addEventListener("hashchange", () => {
    const r = parseHash(location.hash);
    recordNavigation(r, false); route.set({ current: r });
  });
}
