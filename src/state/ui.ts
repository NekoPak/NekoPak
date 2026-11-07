import type { ComponentChildren } from "preact";
import type { ClientError } from "../lib/contract";
import { Store } from "../lib/store";

export interface Toast {
  id: number;
  tone: "info" | "success" | "warning" | "error";
  text: string;
  error?: ClientError;
  action?: { label: string; run: () => void };
}

export const toasts = new Store<{ items: Toast[] }>({ items: [] });
let toastSeq = 0;

export function toast(text: string, tone: Toast["tone"] = "info", extra: Partial<Toast> = {}): void {
  const id = ++toastSeq;
  toasts.set((s) => ({ items: [...s.items.slice(-3), { id, tone, text, ...extra }] }));
  const ttl = tone === "error" ? 9000 : 4200;
  setTimeout(() => dismissToast(id), ttl);
}

export function dismissToast(id: number): void {
  toasts.set((s) => ({ items: s.items.filter((t) => t.id !== id) }));
}

/* ------------------------------------------------------------------ */
/* Modal dialogs: one at a time, promise based                         */
/* ------------------------------------------------------------------ */

export interface DialogSpec {
  title: string;
  render: (close: (value?: unknown) => void) => ComponentChildren;
  /** Width in px. */
  width?: number;
  /** Close on Esc / backdrop (default true). */
  dismissable?: boolean;
}

interface DialogEntry extends DialogSpec {
  id: number;
  resolve: (v: unknown) => void;
}

export const dialogs = new Store<{ stack: DialogEntry[] }>({ stack: [] });
let dialogSeq = 0;

export function openDialog<T = unknown>(spec: DialogSpec): Promise<T | undefined> {
  return new Promise((resolve) => {
    const id = ++dialogSeq;
    dialogs.set((s) => ({ stack: [...s.stack, { ...spec, id, resolve: resolve as (v: unknown) => void }] }));
  });
}

export function closeDialog(id: number, value?: unknown): void {
  const entry = dialogs.get().stack.find((d) => d.id === id);
  dialogs.set((s) => ({ stack: s.stack.filter((d) => d.id !== id) }));
  entry?.resolve(value);
}

/* ------------------------------------------------------------------ */
/* Context menu                                                        */
/* ------------------------------------------------------------------ */

export interface MenuItem {
  id: string;
  label: string;
  shortcut?: string;
  icon?: string;
  danger?: boolean;
  disabled?: boolean;
  /** Shown as tooltip and to screen readers when disabled. */
  reason?: string;
  run?: () => void;
  separatorBefore?: boolean;
  /** Shown as an icon in the menu's top quick row (at most five). */
  quick?: boolean;
}

export interface MenuState {
  open: boolean;
  x: number;
  y: number;
  items: MenuItem[];
  /** Element to refocus when the menu closes. */
  returnFocus?: HTMLElement | null;
  label?: string;
}

export const menu = new Store<MenuState>({ open: false, x: 0, y: 0, items: [] });

export function openMenu(x: number, y: number, items: MenuItem[], label?: string): void {
  menu.set({ open: true, x, y, items, label, returnFocus: document.activeElement as HTMLElement | null });
}

export function closeMenu(): void {
  const rf = menu.get().returnFocus;
  menu.set({ open: false, items: [] });
  rf?.focus?.();
}

/** Global "search box" focus request (Ctrl+F). */
export const focusRequests = new Store<{ search: number }>({ search: 0 });
