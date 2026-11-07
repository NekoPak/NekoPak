import type { CommandArgs, CommandName, CommandResult } from "../contract-proposals";
import type { AllEvents, AnyEventName, Transport, Unlisten } from "./types";
import { adaptResult, RUST_COMMANDS, unsupported } from "./rust-adapter";

/** True inside a Tauri v2 WebView. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

type CoreModule = typeof import("@tauri-apps/api/core");
type EventModule = typeof import("@tauri-apps/api/event");

/**
 * Real IPC. Modules are imported lazily so the browser/mock build never
 * evaluates Tauri globals. Errors are passed through untouched; api.ts
 * normalises them into ClientError.
 */
export class TauriTransport implements Transport {
  readonly kind = "tauri" as const;
  private core: Promise<CoreModule> | null = null;
  private events: Promise<EventModule> | null = null;

  private loadCore(): Promise<CoreModule> {
    return (this.core ??= import("@tauri-apps/api/core"));
  }

  private loadEvents(): Promise<EventModule> {
    return (this.events ??= import("@tauri-apps/api/event"));
  }

  /**
   * Argument style (PROPOSAL UI-001 §P7):
   *  - "flat"    → invoke("files_list", { parent_id, cursor })  — literal reading of 03_CONTRACTS §3.
   *               Rust handlers then need #[tauri::command(rename_all = "snake_case")],
   *               otherwise Tauri v2 expects camelCase keys.
   *  - "wrapped" → invoke("files_list", { args: { parent_id, cursor } }) for `args: FilesListArgs` handlers.
   */
  static argStyle: "flat" | "wrapped" =
    (import.meta.env?.VITE_NEKOPAK_IPC_ARGS as "flat" | "wrapped" | undefined) ?? "flat";

  async invoke<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
    if (!RUST_COMMANDS.has(cmd)) throw unsupported(cmd);
    const { invoke } = await this.loadCore();
    const payload =
      args === undefined ? {} : TauriTransport.argStyle === "wrapped" ? { args } : (args as Record<string, unknown>);
    return adaptResult(cmd, await invoke<unknown>(cmd, payload)) as CommandResult<K>;
  }

  async listen<E extends AnyEventName>(event: E, handler: (payload: AllEvents[E]) => void): Promise<Unlisten> {
    const { listen } = await this.loadEvents();
    return listen<AllEvents[E]>(event, (e) => handler(e.payload));
  }
}
