/**
 * Pure reconciliation rules for transfer events vs. the persisted snapshot.
 * 03_CONTRACTS §4: duplicate / out-of-order events are dropped using a
 * monotonic revision (preferred, PROPOSAL UI-001 §P3) or updated_at.
 */
import type { TaskState, TransferChangedEvent, TransferProgressEvent } from "../lib/contract";
import type { TransferTaskExt } from "../lib/contract-proposals";
import { parseU64 } from "../lib/format";

const TERMINAL: ReadonlySet<TaskState> = new Set<TaskState>(["completed", "failed", "cancelled"]);

type Ordered = { revision?: string | number; updated_at?: string };

function revision(value: string | number | undefined): bigint | null {
  if (typeof value === "number") return Number.isSafeInteger(value) && value >= 0 ? BigInt(value) : null;
  return typeof value === "string" ? parseU64(value) : null;
}

/** -1 = event older, 0 = same, 1 = newer, null = no ordering information. */
export function compareOrder(task: Ordered, ev: Ordered): -1 | 0 | 1 | null {
  const newer = revision(ev.revision), older = revision(task.revision);
  if (newer !== null && older !== null) {
    return newer < older ? -1 : newer === older ? 0 : 1;
  }
  if (ev.updated_at && task.updated_at) {
    const a = Date.parse(task.updated_at);
    const b = Date.parse(ev.updated_at);
    if (!Number.isNaN(a) && !Number.isNaN(b)) return b < a ? -1 : b === a ? 0 : 1;
  }
  return null;
}

export function applyProgress(task: TransferTaskExt, ev: TransferProgressEvent): TransferTaskExt | null {
  const order = compareOrder(task, ev);
  if (order === -1 || order === 0) return null;
  if (order === null) {
    // Heuristic fallback when the backend sends no ordering info.
    if (TERMINAL.has(task.state)) return null;
    const cur = parseU64(task.transferred_bytes);
    const next = parseU64(ev.transferred_bytes);
    if (cur !== null && next !== null && ev.state === task.state && next < cur) return null;
  }
  const out: TransferTaskExt = {
    ...task,
    state: ev.state,
    transferred_bytes: ev.transferred_bytes,
    total_bytes: ev.total_bytes ?? task.total_bytes,
    speed_bytes_per_sec: ev.speed_bytes_per_sec,
  };
  delete out.received_bytes;
  if (revision(ev.revision) !== null) out.revision = ev.revision;
  if (ev.updated_at) out.updated_at = ev.updated_at;
  if (ev.error) out.error = ev.error;
  else if (ev.state !== "failed" && ev.state !== "retrying") delete out.error;
  if (ev.simulated !== undefined) out.simulated = ev.simulated;
  return out;
}

export function applyChanged(task: TransferTaskExt, ev: TransferChangedEvent): TransferTaskExt | null {
  const order = compareOrder(task, ev);
  if (order === -1 || order === 0) return null;
  const out: TransferTaskExt = { ...task, state: ev.state };
  delete out.received_bytes;
  if (ev.error) out.error = ev.error;
  else delete out.error;
  if (ev.state !== "running") delete out.speed_bytes_per_sec;
  if (revision(ev.revision) !== null) out.revision = ev.revision;
  if (ev.transferred_bytes !== undefined) out.transferred_bytes = ev.transferred_bytes;
  if (ev.total_bytes !== undefined) out.total_bytes = ev.total_bytes;
  if (ev.simulated !== undefined) out.simulated = ev.simulated;
  if (ev.updated_at) out.updated_at = ev.updated_at;
  return out;
}

/** Snapshot rows replace local rows unless the local row is provably newer. */
export function mergeSnapshot(local: TransferTaskExt | undefined, snap: TransferTaskExt): TransferTaskExt {
  if (!local) return snap;
  const order = compareOrder(local, snap);
  return order === -1 ? local : snap;
}
