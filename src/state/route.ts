import { ROOT_ID } from "../lib/contract-proposals";
import { Store } from "../lib/store";

export type Route =
  | { page: "files"; folder: string }
  | { page: "search"; query: string }
  | { page: "recent" }
  | { page: "transfers"; tab: "active" | "done" | "failed" }
  | { page: "cloud" }
  | { page: "sync"; config?: string }
  | { page: "mount" }
  | { page: "trash" }
  | { page: "settings"; section: SettingsSection }
  | { page: "about" };

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
    case "settings": return r.section === "general" ? "#/settings" : `#/settings/${r.section}`;
    default: return `#/${r.page}`;
  }
}

export const route = new Store<{ current: Route }>({ current: parseHash(globalThis.location?.hash ?? "") });

export function navigate(r: Route, replace = false): void {
  const h = routeHash(r);
  if (replace) history.replaceState(null, "", h);
  else if (location.hash !== h) location.hash = h;
  route.set({ current: r });
}

export function initRouter(): void {
  window.addEventListener("hashchange", () => route.set({ current: parseHash(location.hash) }));
}
