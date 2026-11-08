/**
 * Tiny command bus: the command palette and global shortcuts ask the page
 * that is currently mounted to run one of its own actions.
 */
export type PageCommand =
  | "new-folder" | "add-link" | "paste" | "select-all" | "batch-rename"
  | "find-duplicates" | "space" | "refresh" | "download-selection";

type Handler = (cmd: PageCommand) => boolean;
const handlers = new Set<Handler>();

/** Returns an unsubscribe function. A handler returns true when it handled the command. */
export function onPageCommand(h: Handler): () => void {
  handlers.add(h);
  return () => handlers.delete(h);
}

export function runPageCommand(cmd: PageCommand): boolean {
  for (const h of [...handlers].reverse()) if (h(cmd)) return true;
  return false;
}

/** Which page commands are available right now (for the palette). */
const available = new Set<PageCommand>();
export function setAvailableCommands(cmds: PageCommand[]): void {
  available.clear();
  for (const c of cmds) available.add(c);
}
export function isCommandAvailable(cmd: PageCommand): boolean {
  return available.has(cmd);
}
