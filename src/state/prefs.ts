/**
 * Device-local UI preferences (per-viewer conveniences only). Everything
 * here survives a missing/blocked localStorage by falling back to defaults.
 */
import type { ComponentChildren } from "preact";
import { Store } from "../lib/store";
import type { AccentId } from "../styles/accents.generated";

export interface PinnedFolder {
  id: string;
  name: string;
}

export interface Prefs {
  accent: AccentId;
  view: "list" | "grid";
  density: "comfortable" | "compact";
  sidebarCollapsed: boolean;
  reduceMotion: boolean;
  pins: PinnedFolder[];
}

const KEY = "nekopak.prefs.v2";
const DEFAULTS: Prefs = { accent: "mist", view: "list", density: "comfortable", sidebarCollapsed: false, reduceMotion: false, pins: [] };

function load(): Prefs {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Prefs>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

export const prefs = new Store<Prefs>(load());

export function setPref<K extends keyof Prefs>(key: K, value: Prefs[K]): void {
  prefs.set({ [key]: value } as Partial<Prefs>);
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs.get()));
    if (key === "accent") localStorage.setItem("nekopak.accent", String(value));
  } catch {
    /* non-critical */
  }
  applyPrefs();
}

export function applyPrefs(): void {
  const p = prefs.get();
  const root = document.documentElement;
  root.setAttribute("data-accent", p.accent);
  root.toggleAttribute("data-reduce-motion", p.reduceMotion);
}

export function togglePin(f: PinnedFolder): boolean {
  const pins = prefs.get().pins;
  const has = pins.some((p) => p.id === f.id);
  setPref("pins", has ? pins.filter((p) => p.id !== f.id) : [...pins, f]);
  return !has;
}

export function isPinned(id: string): boolean {
  return prefs.get().pins.some((p) => p.id === id);
}

/* ------------------------------------------------------------------ */
/* Side island (non-modal panel to the right of the canvas)            */
/* ------------------------------------------------------------------ */

export interface SidePanel {
  key: string;
  title: string;
  render: (close: () => void) => ComponentChildren;
}

export const side = new Store<{ panel: SidePanel | null }>({ panel: null });

export function openSide(panel: SidePanel): void {
  side.set({ panel });
}

export function closeSide(): void {
  side.set({ panel: null });
}
