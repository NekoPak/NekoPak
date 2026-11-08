/**
 * Undo journal for reversible cloud operations (move, rename, trash, copy,
 * star). Each entry knows how to issue the inverse files_mutate calls.
 * Undo is best-effort: the inverse can fail if the tree changed meanwhile,
 * and the failure is reported like any other operation.
 */
import { Store } from "../lib/store";
import { onSessionReset } from "./session";

export interface UndoEntry {
  id: number;
  /** e.g. "移动 3 项" — shown as “撤销移动 3 项”. */
  label: string;
  run: () => Promise<boolean>;
  at: number;
}

export const undoLog = new Store<{ entries: UndoEntry[]; busy: boolean }>({ entries: [], busy: false });
onSessionReset(() => undoLog.set({ entries: [], busy: false }));
let seq = 0;

export function pushUndo(label: string, run: () => Promise<boolean>): UndoEntry {
  const e: UndoEntry = { id: ++seq, label, run, at: Date.now() };
  undoLog.set((s) => ({ entries: [...s.entries.slice(-19), e] }));
  return e;
}

/** Undo a specific entry (from a snackbar) or the latest one (Ctrl+Z). */
export async function undo(id?: number): Promise<boolean> {
  const s = undoLog.get();
  if (s.busy) return false;
  const e = id === undefined ? s.entries[s.entries.length - 1] : s.entries.find((x) => x.id === id);
  if (!e) return false;
  undoLog.set({ busy: true, entries: s.entries.filter((x) => x.id !== e.id) });
  try {
    return await e.run();
  } finally {
    undoLog.set({ busy: false });
  }
}

export function canUndo(): boolean {
  return undoLog.get().entries.length > 0;
}
