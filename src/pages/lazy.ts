/**
 * On-demand entry points: these tools are loaded the first time they are
 * used, so they do not weigh on start-up.
 */
import type { RemoteEntry } from "../lib/contract";

type RenameCtx = { siblings: () => string[]; reload: () => void };

export function openBatchRename(entries: RemoteEntry[], ctx: RenameCtx): void {
  void import("./BatchRename").then((m) => m.openBatchRename(entries, ctx));
}

export function openShareDialog(entries: RemoteEntry[]): void {
  void import("./Shares").then((m) => m.openShareDialog(entries));
}

export function createReferenceArchive(entries: RemoteEntry[]): void {
  void import("./References").then(m => m.createReferenceArchive(entries));
}

export function openClipPanel(entry: RemoteEntry): void {
  void import("./Media").then((m) => m.openClipPanel(entry));
}

export function chooseQuality(entries: RemoteEntry[]): void {
  void import("./Media").then((m) => m.chooseQuality(entries));
}

export function playExternal(entry: RemoteEntry): void {
  void import("./Media").then((m) => m.playExternal(entry));
}
export function openPlayer(entry: RemoteEntry): void {
  void import("./Player").then(m => m.openPlayer(entry));
}

export function openMockPanel(): void {
  void import("./DevPanel").then((m) => m.openMockPanel());
}
