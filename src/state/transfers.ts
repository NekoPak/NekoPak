import type { TransferChangedEvent, TransferProgressEvent } from "../lib/contract";
import { EVENTS } from "../lib/contract";
import type { TransferTaskExt } from "../lib/contract-proposals";
import { api, collectAll, errorOf, on } from "../lib/api";
import type { ClientError } from "../lib/contract";
import { Store } from "../lib/store";
import { applyChanged, applyProgress, mergeSnapshot } from "./reconcile";
import { onSessionReset, sessionGeneration } from "./session";

export interface TransfersState {
  tasks: ReadonlyMap<string, TransferTaskExt>;
  /** Newest first, as returned by transfers_list. */
  order: readonly string[];
  phase: "idle" | "loading" | "ready" | "error";
  error?: ClientError;
  snapshot_at?: string;
  /** Count of stale/duplicate events discarded — surfaced in diagnostics. */
  dropped_events: number;
}

type Pending =
  | { kind: "progress"; ev: TransferProgressEvent }
  | { kind: "changed"; ev: TransferChangedEvent };

export const transfers = new Store<TransfersState>(
  { tasks: new Map(), order: [], phase: "idle", dropped_events: 0 },
  true,
);

let started = false;
let buffering = true;
let buffer: Pending[] = [];
let reloadTimer: ReturnType<typeof setTimeout> | null = null;
let loading: Promise<void> | null = null;
let reloadAgain = false;
onSessionReset(() => {
  buffer = [];
  transfers.replace({ tasks: new Map(), order: [], phase: "idle", dropped_events: 0 });
});

function apply(p: Pending): void {
  const s = transfers.get();
  const cur = s.tasks.get(p.ev.task_id);
  if (!cur) {
    // Unknown task: the snapshot is the source of truth — fetch it.
    scheduleReload();
    return;
  }
  const next = p.kind === "progress" ? applyProgress(cur, p.ev) : applyChanged(cur, p.ev);
  if (!next) {
    transfers.set({ dropped_events: s.dropped_events + 1 });
    return;
  }
  const tasks = new Map(s.tasks);
  tasks.set(next.id, next);
  transfers.set({ tasks });
}

function onEvent(p: Pending): void {
  if (buffering) buffer.push(p);
  else apply(p);
}

export function scheduleReload(delay = 300): void {
  if (reloadTimer) return;
  reloadTimer = setTimeout(() => {
    reloadTimer = null;
    void loadSnapshot();
  }, delay);
}

/**
 * Snapshot first, then events (03_CONTRACTS §4). Listeners are attached
 * before the snapshot request so nothing emitted in between is lost;
 * buffered events are replayed against the snapshot and stale ones dropped.
 */
export async function startTransfers(): Promise<void> {
  if (started) return;
  started = true;
  await on(EVENTS.transferProgress, (ev) => onEvent({ kind: "progress", ev }));
  await on(EVENTS.transferChanged, (ev) => onEvent({ kind: "changed", ev }));
  await on("nekopak://transfer/resync-required", () => scheduleReload(0));
  await on("nekopak://storage/error", error => transfers.set({ error }));
  await loadSnapshot();
  // Rates are volatile within a persisted revision; refresh while receiving chunks.
  setInterval(()=>{if([...transfers.get().tasks.values()].some(t=>t.backend==="rust_compat"&&t.state==="running"))scheduleReload(0);},500);
}

export async function loadSnapshot(): Promise<void> {
  if (loading) { reloadAgain = true; return loading; }
  loading = readSnapshot();
  try { await loading; } finally {
    loading = null;
    if (reloadAgain) { reloadAgain = false; scheduleReload(0); }
  }
}

async function readSnapshot(): Promise<void> {
  const generation = sessionGeneration();
  buffering = true;
  const prev = transfers.get();
  transfers.set({ phase: prev.phase === "ready" ? "ready" : "loading" });
  try {
    const items = await collectAll(() => api.transfersList(), (c) => api.transfersList(c), 20);
    if (generation !== sessionGeneration()) return;
    const tasks = new Map<string, TransferTaskExt>();
    for (const t of items) tasks.set(t.id, mergeSnapshot(prev.tasks.get(t.id), t));
    transfers.set({
      tasks,
      order: items.map((t) => t.id),
      phase: "ready",
      error: undefined,
      snapshot_at: new Date().toISOString(),
    });
  } catch (e) {
    if (generation !== sessionGeneration()) return;
    transfers.set({ phase: prev.tasks.size ? "ready" : "error", error: errorOf(e) });
  } finally {
    const queued = buffer;
    buffer = [];
    buffering = false;
    for (const p of queued) apply(p);
  }
}

/** Optimistically reflect a just-created task until the snapshot arrives. */
export function noteCreated(): void {
  scheduleReload(50);
}
