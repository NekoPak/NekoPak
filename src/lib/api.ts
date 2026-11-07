/**
 * NekoPak typed IPC client — the only module UI code uses to reach the
 * backend. Signatures follow docs/03_CONTRACTS.md; proposed commands are
 * marked and degrade to UNSUPPORTED when the backend lacks them.
 */
import type { BackendId, ClientError, Page, RemoteEntry, SyncPlan } from "./contract";
import type {
  AppStatus, AuthFlow, AuthState, CommandArgs, CommandName, CommandResult,
  MountCapabilities, MountState, MutateOp, MutateTarget, OperationResult, QuotaReport,
  ScanRef, Settings, SettingsPatch, SyncConfig, SyncEvent, TaskRef, TransferAction, TransferTaskExt,
} from "./contract-proposals";
import { PROPOSED_COMMANDS } from "./contract-proposals";
import { toClientError } from "./errors";
import type { FaultFlags, ScenarioId } from "./mock/fixtures";
import { SCENARIOS } from "./mock/fixtures";
import type { MockTransport } from "./mock/backend";
import { inTauri, TauriTransport } from "./transport/tauri";
import type { AllEvents, AnyEventName, Transport, Unlisten } from "./transport/types";

export class ApiError extends Error {
  readonly error: ClientError;
  readonly command: CommandName;
  constructor(command: CommandName, error: ClientError) {
    super(`${command}: ${error.code} ${error.message}`);
    this.error = error;
    this.command = command;
  }
}

const READ_TIMEOUT_MS = 30_000;
/** Read-only commands may be timed out by the UI; side-effecting ones never are. */
const READ_ONLY = new Set<CommandName>([
  "app_status", "backend_discover", "auth_status", "files_list", "files_stat", "files_path",
  "files_search", "files_recent", "trash_list", "transfers_list", "quota_get",
  "sync_plan", "sync_history", "sync_configs_list", "mount_capabilities", "settings_get",
]);

const STORAGE_KEYS = { scenario: "nekopak.mock.scenario", faults: "nekopak.mock.faults" } as const;

function readLocal<T>(key: string): T | undefined {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : undefined;
  } catch {
    return undefined;
  }
}
export function writeLocal(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable (private mode / preview) — non-critical */
  }
}

function pickTransportKind(): "mock" | "tauri" {
  // A desktop app always uses Rust IPC, even if a browser build set a demo override.
  if (inTauri()) return "tauri";
  const forced = import.meta.env?.VITE_NEKOPAK_TRANSPORT as string | undefined;
  if (forced === "mock" || forced === "tauri") return forced;
  return inTauri() ? "tauri" : "mock";
}

let transport: Transport | null = null;
let initializing: Promise<Transport> | null = null;
let mock: MockTransport | null = null;

async function getTransport(): Promise<Transport> {
  if (transport) return transport;
  if (initializing) return initializing;
  initializing = initializeTransport();
  try { return await initializing; } finally { initializing = null; }
}

async function initializeTransport(): Promise<Transport> {
  if (pickTransportKind() === "tauri") {
    transport = new TauriTransport();
  } else {
    const { MockTransport } = await import("./mock/backend");
    const params = new URLSearchParams(globalThis.location?.search ?? "");
    const fromUrl = params.get("scenario") as ScenarioId | null;
    const valid = (s: unknown): s is ScenarioId => SCENARIOS.some((x) => x.id === s);
    const scenario: ScenarioId = valid(fromUrl) ? fromUrl : valid(readLocal(STORAGE_KEYS.scenario)) ? readLocal<ScenarioId>(STORAGE_KEYS.scenario)! : "free";
    const faults = readLocal<Partial<FaultFlags>>(STORAGE_KEYS.faults) ?? {};
    mock = new MockTransport(scenario, faults);
    transport = mock;
  }
  return transport;
}

export async function transportKind(): Promise<"mock" | "tauri"> {
  return (await getTransport()).kind;
}

async function call<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  const t = await getTransport();
  const p = t.invoke(cmd, args);
  try {
    if (!READ_ONLY.has(cmd)) return await p;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject({ code: "TRANSIENT_NETWORK", message: `请求超时（${READ_TIMEOUT_MS / 1000} 秒）`, retryable: true } satisfies ClientError),
        READ_TIMEOUT_MS,
      );
    });
    try {
      return await Promise.race([p, timeout]);
    } finally {
      clearTimeout(timer);
    }
  } catch (e) {
    const err = toClientError(e);
    if (err.code === "UNSUPPORTED" && PROPOSED_COMMANDS.has(cmd)) {
      err.details = { ...(err.details ?? {}), proposal: "UI-001" };
    }
    throw new ApiError(cmd, err);
  }
}

export function errorOf(e: unknown): ClientError {
  return e instanceof ApiError ? e.error : toClientError(e);
}

export function newIdempotencyKey(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `idk-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/* ------------------------------------------------------------------ */
/* Commands                                                            */
/* ------------------------------------------------------------------ */

export const api = {
  appStatus: (): Promise<AppStatus> => call("app_status", undefined),
  backendDiscover: () => call("backend_discover", undefined),

  authBegin: (backend: BackendId): Promise<AuthFlow> => call("auth_begin", { backend }),
  /** PROPOSAL UI-001 */
  authCancel: (flow_id: string) => call("auth_cancel", { flow_id }),
  authStatus: (): Promise<AuthState> => call("auth_status", undefined),
  authLogout: (backend: BackendId) => call("auth_logout", { backend }),

  filesList: (parent_id: string, cursor?: string, limit = 500): Promise<Page<RemoteEntry>> =>
    call("files_list", cursor ? { parent_id, cursor, limit } : { parent_id, limit }),
  filesStat: (id: string) => call("files_stat", { id }),
  /** PROPOSAL UI-001 */
  filesPath: (id: string) => call("files_path", { id }),
  /** PROPOSAL UI-001 */
  filesSearch: (query: string, cursor?: string) => call("files_search", cursor ? { query, cursor, limit: 200 } : { query, limit: 200 }),
  /** PROPOSAL UI-001 */
  filesRecent: () => call("files_recent", { limit: 100 }),
  /** PROPOSAL UI-001 */
  trashList: (cursor?: string) => call("trash_list", cursor ? { cursor } : {}),

  /** Pass the same key when retrying the same user intent. */
  filesMutate: (op: MutateOp, ids: string[], target: MutateTarget | undefined, idempotency_key: string): Promise<OperationResult> =>
    call("files_mutate", target ? { op, ids, target, idempotency_key } : { op, ids, idempotency_key }),

  cloudTaskCreate: (source_url: string, parent_id: string, idempotency_key: string): Promise<TaskRef> =>
    call("cloud_task_create", { source_url, parent_id, idempotency_key }),

  transfersList: (cursor?: string): Promise<Page<TransferTaskExt>> =>
    call("transfers_list", cursor ? { cursor, limit: 500 } : { limit: 500 }),
  transferStart: (remote_id: string, destination: string): Promise<TaskRef> => call("transfer_start", { remote_id, destination, idempotency_key: newIdempotencyKey() }),
  transferControl: (task_id: string, action: TransferAction) => call("transfer_control", { task_id, action }),

  quotaGet: (): Promise<QuotaReport> => call("quota_get", undefined),

  /** PROPOSAL UI-001 */
  syncConfigsList: (): Promise<SyncConfig[]> => call("sync_configs_list", undefined),
  /** PROPOSAL UI-001 */
  syncConfigSave: (config: SyncConfig): Promise<SyncConfig> => call("sync_config_save", { config }),
  syncScan: (config_id: string): Promise<ScanRef> => call("sync_scan", { config_id }),
  syncPlan: (config_id: string): Promise<SyncPlan> => call("sync_plan", { config_id }),
  syncApply: (plan_id: string, confirm_deletes: boolean): Promise<TaskRef> => call("sync_apply", { plan_id, confirm_deletes }),
  syncHistory: (config_id: string): Promise<Page<SyncEvent>> => call("sync_history", { config_id }),

  mountCapabilities: (): Promise<MountCapabilities> => call("mount_capabilities", undefined),
  mountStart: (config_id: string, read_only: boolean): Promise<MountState> => call("mount_start", { config_id, read_only }),
  mountStop: (id: string) => call("mount_stop", { id }),

  settingsGet: (): Promise<Settings> => call("settings_get", undefined),
  settingsUpdate: (patch: SettingsPatch): Promise<Settings> => call("settings_update", { patch }),
};

/** Load every page of a cursor-paged command (used for small lists only). */
export async function collectAll<T>(first: () => Promise<Page<T>>, next: (cursor: string) => Promise<Page<T>>, max = 50): Promise<T[]> {
  const out: T[] = [];
  let page = await first();
  out.push(...page.items);
  let n = 1;
  while (page.next_cursor && n++ < max) {
    page = await next(page.next_cursor);
    out.push(...page.items);
  }
  if (page.next_cursor) throw new ApiError("transfers_list", { code: "CONFLICT", message: "快照超过分页安全上限，未接受不完整列表。", retryable: false });
  return out;
}

export async function on<E extends AnyEventName>(event: E, handler: (payload: AllEvents[E]) => void): Promise<Unlisten> {
  const t = await getTransport();
  return t.listen(event, handler);
}

/* ------------------------------------------------------------------ */
/* Mock-only dev controls                                              */
/* ------------------------------------------------------------------ */

export interface MockControlsApi {
  scenario(): ScenarioId;
  faults(): FaultFlags;
  setScenario(s: ScenarioId): void;
  setFaults(f: Partial<FaultFlags>): void;
  completeAuth(outcome: "success" | "cancel" | "network"): void;
  simulateRestart(): void;
}

export async function mockControls(): Promise<MockControlsApi | null> {
  await getTransport();
  const m = mock;
  if (!m) return null;
  return {
    scenario: () => m.scenario,
    faults: () => ({ ...m.faults }),
    setScenario: (s) => {
      writeLocal(STORAGE_KEYS.scenario, s);
      m.reset(s);
    },
    setFaults: (f) => {
      m.setFaults(f);
      writeLocal(STORAGE_KEYS.faults, m.faults);
    },
    completeAuth: (o) => m.completeAuth(o),
    simulateRestart: () => m.simulateRestart(),
  };
}

/* ------------------------------------------------------------------ */
/* Desktop helpers                                                     */
/* ------------------------------------------------------------------ */

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

/**
 * Open an external https link. In Tauri this needs @tauri-apps/plugin-opener
 * on Astra's side (PROPOSAL UI-001 §P9); until then the link is copied.
 */
export async function openExternal(url: string): Promise<"opened" | "copied" | "failed"> {
  if (!/^https:\/\//.test(url)) return "failed";
  if (inTauri()) {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: (c: string, a: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    try {
      if (!internals?.invoke) return (await copyText(url)) ? "copied" : "failed";
      await internals.invoke("plugin:opener|open_url", { url });
      return "opened";
    } catch {
      return (await copyText(url)) ? "copied" : "failed";
    }
  }
  const w = window.open(url, "_blank", "noopener,noreferrer");
  return w ? "opened" : (await copyText(url)) ? "copied" : "failed";
}
