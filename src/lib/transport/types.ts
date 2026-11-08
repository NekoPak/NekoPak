import type { EventPayloads } from "../contract";
import type { AuthChangedEvent, CommandArgs, CommandName, CommandResult, DuplicatesProgressEvent, ServerJob } from "../contract-proposals";

export interface AllEvents extends EventPayloads {
  "nekopak://desktop/navigate": "transfers";
  "nekopak://auth/changed": AuthChangedEvent;
  "nekopak://transfer/resync-required": null;
  "nekopak://storage/error": import("../contract").ClientError;
  "nekopak://duplicates/progress": DuplicatesProgressEvent;
  "nekopak://job/changed": ServerJob;
}
export type AnyEventName = keyof AllEvents;

export type Unlisten = () => void;

/**
 * The only seam between UI and backend. `TauriTransport` forwards to
 * `@tauri-apps/api`; `MockTransport` answers from deterministic fixtures.
 * Business code must never branch on which one is active.
 */
export interface Transport {
  readonly kind: "mock" | "tauri";
  invoke<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>>;
  listen<E extends AnyEventName>(event: E, handler: (payload: AllEvents[E]) => void): Promise<Unlisten>;
}
