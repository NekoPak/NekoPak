/**
 * In-app clipboard for cut / copy of cloud items (Ctrl+X / Ctrl+C, then
 * Ctrl+V in another folder). It never touches the OS clipboard; “复制路径”
 * (Ctrl+Shift+C) is the action that writes text there.
 */
import type { RemoteEntry } from "../lib/contract";
import { Store } from "../lib/store";
import { onSessionReset } from "./session";

export interface ClipState {
  mode: "cut" | "copy" | null;
  entries: RemoteEntry[];
}

export const clip = new Store<ClipState>({ mode: null, entries: [] });

export function setClip(mode: "cut" | "copy", entries: RemoteEntry[]): void {
  clip.set({ mode, entries: entries.slice() });
}

export function clearClip(): void {
  clip.set({ mode: null, entries: [] });
}

onSessionReset(clearClip);

export function isCut(id: string): boolean {
  const c = clip.get();
  return c.mode === "cut" && c.entries.some((e) => e.id === id);
}
