import type { CommandMap, TransferEvent, TransferTask } from './api-types';

export type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;

/** Inject @tauri-apps/api/core.invoke or an offline fixture transport from the UI. */
export function createApi(invoke: Invoke) {
  return <K extends keyof CommandMap>(command: K, args: CommandMap[K]['args']): Promise<CommandMap[K]['result']> =>
    invoke<CommandMap[K]['result']>(command, args);
}

/** Events for unknown IDs require a new transfers_list snapshot, never an invented task. */
export function applyTransferEvent(task: TransferTask, event: TransferEvent): TransferTask {
  if (task.id !== event.task_id || BigInt(event.revision) <= BigInt(task.revision)) return task;
  return { ...task, state: event.state, transferred_bytes: event.transferred_bytes,
    total_bytes: event.total_bytes, speed_bytes_per_sec: event.speed_bytes_per_sec,
    error: event.error, revision: event.revision, updated_at: event.updated_at, simulated: event.simulated };
}

