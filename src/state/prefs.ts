/**
 * Device-local UI preferences (per-viewer conveniences only). Everything
 * here survives a missing/blocked localStorage by falling back to defaults.
 */
import type { ComponentChildren } from "preact";
import { Store } from "../lib/store";
import type { AccentId } from "../styles/accents.generated";
import { onSessionReset, sessionGeneration } from "./session";
import type { UiPreferences } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { tr } from "../lib/i18n";
import { toast } from "./ui";

export interface PinnedFolder {
  id: string;
  name: string;
}

export type GridSize = "s" | "m" | "l";

export interface Prefs {
  accent: AccentId;
  view: "list" | "grid";
  density: "comfortable" | "compact";
  /** Card size of the grid view (Ctrl + wheel steps list → s → m → l). */
  gridSize: GridSize;
  /** Folders before files regardless of the sort key. */
  foldersFirst: boolean;
  /** Offer to add a magnet / http(s) link found on the clipboard when the window regains focus. Off by default. */
  clipboardWatch: boolean;
  sidebarCollapsed: boolean;
  reduceMotion: boolean;
  pins: PinnedFolder[];
  sort: NonNullable<UiPreferences["sort"]>;
}

const KEY = "nekopak.prefs.v2";
const DEFAULTS: Prefs = {
  accent: "mist", view: "list", density: "comfortable", gridSize: "m", foldersFirst: true, clipboardWatch: false,
  sidebarCollapsed: false, reduceMotion: false, pins: [],
  sort: { key: "name", dir: "asc" },
};

const SCOPED_KEYS = new Set<keyof Prefs>(["pins", "view", "sort", "density", "gridSize", "foldersFirst"]);
let nativePrefs = false;
let scopedReady = false;
let loadedGeneration = -1;
let accountId: string | undefined;
let pendingRead: { generation: number; promise: Promise<void> } | undefined;
let saveQueue: Promise<void> = Promise.resolve();
let suspended = false;

function scopedValues(value: Prefs): UiPreferences {
  return { pins: value.pins.map(pin => ({ ...pin })), view: value.view, sort: { ...value.sort }, density: value.density, gridSize: value.gridSize, foldersFirst: value.foldersFirst };
}

function storeDevicePreferences(): void {
  try {
    const value = { ...prefs.get() } as Partial<Prefs>;
    if (nativePrefs) for (const key of SCOPED_KEYS) delete value[key];
    localStorage.setItem(KEY, JSON.stringify(value));
  } catch { /* Optional first-paint cache. */ }
}

export function configureNativePreferences(enabled: boolean): void {
  nativePrefs = enabled;
  scopedReady = !enabled;
  loadedGeneration = -1;
  accountId = undefined;
  if (enabled) { prefs.set(scopedValues(DEFAULTS)); storeDevicePreferences(); }
}

export function loadAccountPreferences(): Promise<void> {
  if (!nativePrefs) return Promise.resolve();
  const generation = sessionGeneration();
  if (loadedGeneration === generation) return Promise.resolve();
  if (pendingRead?.generation === generation) return pendingRead.promise;
  const promise = (async () => {
    try {
      const result = await api.uiPrefsGet();
      if (generation !== sessionGeneration()) return;
      const value = result.prefs;
      const normalized: UiPreferences = scopedValues(DEFAULTS);
      if (Array.isArray(value.pins)) normalized.pins = value.pins.filter(pin => typeof pin.id === "string" && typeof pin.name === "string");
      if (value.view === "list" || value.view === "grid") normalized.view = value.view;
      if (value.density === "comfortable" || value.density === "compact") normalized.density = value.density;
      if (value.gridSize === "s" || value.gridSize === "m" || value.gridSize === "l") normalized.gridSize = value.gridSize;
      if (typeof value.foldersFirst === "boolean") normalized.foldersFirst = value.foldersFirst;
      if (value.sort && ["name", "size", "modified", "kind"].includes(value.sort.key) && ["asc", "desc"].includes(value.sort.dir)) normalized.sort = value.sort;
      prefs.set(normalized);
      accountId = result.account_id;
      scopedReady = true;
      loadedGeneration = generation;
    } catch (e) {
      if (generation === sessionGeneration()) toast(errorOf(e).message, "error");
    } finally {
      if (pendingRead?.generation === generation) pendingRead = undefined;
    }
  })();
  pendingRead = { generation, promise };
  return promise;
}

/** Before switching account, finish any preference write already in flight. */
export function flushAccountPreferences(): Promise<void> { return saveQueue; }

/** Prevent new writes from racing the native account switch. */
export async function pauseAccountPreferences(): Promise<() => void> {
  suspended = true;
  await flushAccountPreferences();
  return () => { suspended = false; };
}

/** One zoom step: list → small → medium → large cards (and back). */
export function zoomView(dir: 1 | -1): void {
  const p = prefs.get();
  const steps: { view: Prefs["view"]; size: GridSize }[] = [
    { view: "list", size: p.gridSize }, { view: "grid", size: "s" }, { view: "grid", size: "m" }, { view: "grid", size: "l" },
  ];
  const at = p.view === "list" ? 0 : 1 + ["s", "m", "l"].indexOf(p.gridSize);
  const next = steps[Math.max(0, Math.min(steps.length - 1, at + dir))];
  if (next.view !== p.view) setPref("view", next.view);
  if (next.view === "grid" && next.size !== p.gridSize) setPref("gridSize", next.size);
}

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
  if (nativePrefs && SCOPED_KEYS.has(key) && (!scopedReady || suspended)) {
    toast(tr("账号偏好尚未读取，请连接账号后重试"), "info"); return;
  }
  prefs.set({ [key]: value } as Partial<Prefs>);
  if (nativePrefs && SCOPED_KEYS.has(key)) {
    const generation = sessionGeneration();
    const expectedAccount = accountId;
    const snapshot = scopedValues(prefs.get());
    saveQueue = saveQueue.then(async () => {
      if (generation !== sessionGeneration() || !expectedAccount) return;
      try { await api.uiPrefsUpdate(snapshot, expectedAccount); }
      catch (e) { if (generation === sessionGeneration()) toast(errorOf(e).message, "error"); }
    });
  }
  try {
    storeDevicePreferences();
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
  return prefs.get().pins.some(pin => pin.id === f.id);
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

/**
 * A panel with unsaved edits (e.g. a chosen clip range) registers a guard.
 * The guard returns true when it is fine to close (it may ask the user).
 */
let guard: (() => Promise<boolean>) | null = null;

export function setSideGuard(fn: (() => Promise<boolean>) | null): void {
  guard = fn;
}

export async function openSide(panel: SidePanel): Promise<void> {
  const cur = side.get().panel;
  if (cur && cur.key !== panel.key && guard && !(await guard())) return;
  guard = null;
  side.set({ panel });
}

export async function closeSide(): Promise<void> {
  if (guard && !(await guard())) return;
  guard = null;
  side.set({ panel: null });
}

/** Close without asking (after the panel finished its own work). */
export function dismissSide(): void {
  guard = null;
  side.set({ panel: null });
}

onSessionReset(() => {
  loadedGeneration = -1;
  accountId = undefined;
  scopedReady = !nativePrefs;
  prefs.set(nativePrefs ? scopedValues(DEFAULTS) : { pins: [] });
  storeDevicePreferences();
  dismissSide();
});
