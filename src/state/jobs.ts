/**
 * Server-side jobs (archive extraction, §P19). Same rule as transfers:
 * load the snapshot (`jobs_list`) first, then apply `job/changed` events,
 * dropping any whose revision is not newer than what we hold.
 */
import type { ClientError } from "../lib/contract";
import type { ServerJob } from "../lib/contract-proposals";
import { PROPOSED_EVENTS } from "../lib/contract-proposals";
import { api, collectAll, errorOf, on } from "../lib/api";
import { Store } from "../lib/store";
import { compareOrder } from "./reconcile";
import { onSessionReset, sessionGeneration } from "./session";

export interface JobsState {
  jobs: ReadonlyMap<string, ServerJob>;
  phase: "idle" | "loading" | "ready" | "error" | "unsupported";
  error?: ClientError;
}

export const jobs = new Store<JobsState>({ jobs: new Map(), phase: "idle" });

type Listener = (job: ServerJob, prev: ServerJob | undefined) => void;
const watchers = new Set<Listener>();

/** Observe state transitions (used to show “解压完成” snackbars). */
export function watchJobs(fn: Listener): () => void {
  watchers.add(fn);
  return () => watchers.delete(fn);
}

let started = false;
let buffering = true;
let buffer: ServerJob[] = [];
let loading: Promise<void> | null = null;
onSessionReset(() => { buffer = []; jobs.set({ jobs: new Map(), phase: "idle", error: undefined }); });

function apply(job: ServerJob): void {
  const cur = jobs.get().jobs.get(job.job_id);
  if (cur && compareOrder(cur, job) !== 1) return;
  const next = new Map(jobs.get().jobs);
  next.set(job.job_id, job);
  jobs.set({ jobs: next });
  for (const w of watchers) w(job, cur);
}

export async function startJobs(): Promise<void> {
  if (started) return;
  started = true;
  await on(PROPOSED_EVENTS.jobChanged, (job) => {
    if (buffering) buffer.push(job);
    else apply(job);
  });
  await loadJobs();
  setInterval(() => {
    if (jobs.get().phase !== "unsupported" && [...jobs.get().jobs.values()].some(j => j.state === "running" || j.state === "queued")) void loadJobs();
  }, 2500);
}

export async function loadJobs(): Promise<void> {
  if (loading) return loading;
  loading = readJobs();
  try { await loading; } finally { loading = null; }
}
async function readJobs(): Promise<void> {
  const generation = sessionGeneration();
  buffering = true;
  jobs.set({ phase: jobs.get().phase === "ready" ? "ready" : "loading" });
  try {
    const rows = await collectAll(() => api.jobsList(), (c) => api.jobsList(c), 10);
    if (generation !== sessionGeneration()) return;
    const map = new Map<string, ServerJob>();
    for (const r of rows) map.set(r.job_id, r);
    const previous = jobs.get().jobs;
    jobs.set({ jobs: map, phase: "ready", error: undefined });
    for (const job of rows) if (previous.get(job.job_id)?.state !== job.state) for (const watch of watchers) watch(job, previous.get(job.job_id));
  } catch (e) {
    if (generation !== sessionGeneration()) return;
    const err = errorOf(e);
    jobs.set({ phase: err.code === "UNSUPPORTED" ? "unsupported" : "error", error: err });
  } finally {
    const q = buffer;
    buffer = [];
    buffering = false;
    for (const j of q) apply(j);
  }
}
