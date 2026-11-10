/**
 * NekoPak typed IPC client — the only module UI code uses to reach the
 * backend. Signatures follow docs/03_CONTRACTS.md; proposed commands are
 * marked and degrade to UNSUPPORTED when the backend lacks them.
 */
import type { BackendId, ClientError, Page, RemoteEntry, SyncPlan } from "./contract";
import type {
  AppStatus, AuthFlow, AuthState, CommandArgs, CommandName, CommandResult,
  MountCapabilities, MountConfig, MountState, MutateOp, MutateTarget, OperationResult, QuotaReport,
  ScanRef, Settings, SettingsPatch, SyncConfig, SyncEvent, TaskRef, TransferAction, TransferTaskExt,
  TransferOptions, UiPreferences,
} from "./contract-proposals";
import { PROPOSED_COMMANDS } from "./contract-proposals";
import { toClientError } from "./errors";
import type { FaultFlags, ScenarioId } from "./mock/fixtures";
import { SCENARIOS } from "./mock/fixtures";
import type { MockTransport } from "./mock/backend";
import { inTauri, TauriTransport } from "./transport/tauri";
import type { AllEvents, AnyEventName, Transport, Unlisten } from "./transport/types";
import { sessionGeneration } from "../state/session";
import { entrySource } from "./entry-source";
import { tr, tx } from "./i18n";

export class ApiError extends Error {
  readonly error: ClientError;
  readonly command: CommandName;
  constructor(command: CommandName, error: ClientError) {
    super(`${command}: ${error.code} ${error.message}`);
    this.error = error;
    this.command = command;
  }
}

const READ_TIMEOUT_MS = 50_000;
/** Read-only commands may be timed out by the UI; side-effecting ones never are. */
const READ_ONLY = new Set<CommandName>([
  "app_status", "backend_discover", "auth_status", "files_list", "files_stat", "files_path",
  "files_search", "files_recent", "trash_list", "transfers_list", "quota_get",
  "sync_plan", "sync_history", "sync_configs_list", "mount_capabilities", "settings_get",
  "media_probe", "media_align", "media_preview_frame", "files_starred", "duplicates_report",
  "files_space_summary", "archive_list", "jobs_list", "cloud_task_preview", "shares_list",
  "verification_status", "accounts_list", "local_sources_list", "sync_run_status", "ui_prefs_get", "mount_configs_list", "mount_status", "share_preview_entry", "reference_list", "reference_status", "autostart_get",
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
  const generation = sessionGeneration();
  const accountIndependent = ["auth_preferences_get", "auth_preferences_update", "network_reconnect", "network_domains_probe", "auth_status", "auth_sign_in", "auth_verify", "auth_logout", "auth_cancel", "auth_begin", "account_switch", "account_remove", "backend_discover", "app_status", "settings_get", "settings_update", "media_session_close"].includes(cmd);
  // Defense in depth: keyboard, stale menus and async dialogs all pass through
  // this boundary. Local/shared handles must never reach drive write commands.
  const fields = args as Record<string, unknown> | undefined;
  const driveIds: unknown[] = [];
  if (cmd === "files_mutate") {
    driveIds.push(...(fields?.ids as string[] ?? []), (fields?.target as { parent_id?: string } | undefined)?.parent_id);
  } else if (cmd === "share_create" || cmd === "reference_create") driveIds.push(...(fields?.ids as string[] ?? []));
  else if (cmd === "upload_pick_and_start" || cmd === "upload_directory_pick_and_start" || cmd === "cloud_task_create") driveIds.push(fields?.parent_id);
  else if (cmd === "share_restore") driveIds.push(fields?.target_parent_id);
  else if (cmd === "archive_list" || cmd === "archive_unlock" || cmd === "archive_extract") driveIds.push(fields?.file_id, fields?.target_parent_id);
  else if (cmd === "duplicates_scan" || cmd === "files_space_summary") driveIds.push(fields?.folder_id);
  if (driveIds.some(id => typeof id === "string" && entrySource(id) !== "drive")) {
    throw new ApiError(cmd, { code: "PERMISSION_DENIED", message: tr("本地和分享来源不能作为云盘写操作的目标"), retryable: false });
  }
  const t = await getTransport();
  const cancelled = (): ClientError => ({ code: "CANCELLED", message: tr("账号会话已改变，本次结果不会应用到新账号。"), retryable: false });
  if (!accountIndependent && generation !== sessionGeneration()) throw new ApiError(cmd, cancelled());
  const p = t.invoke(cmd, args).then(result => {
    if (!accountIndependent && generation !== sessionGeneration()) throw cancelled();
    return result;
  });
  try {
    if (!READ_ONLY.has(cmd)) return await p;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject({ code: "TRANSIENT_NETWORK", message: tx("请求超时（{0} 秒）", READ_TIMEOUT_MS / 1000), retryable: true } satisfies ClientError),
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
  transferForget: (task_id: string) => call("transfer_forget", { task_id }),
  transferClearFinished: () => call("transfer_clear_finished", undefined),
  authPreferencesGet: () => call("auth_preferences_get", undefined),
  authPreferencesUpdate: (preferences: import("./contract-proposals").AuthPreferences) => call("auth_preferences_update", { preferences }),
  networkReconnect: () => call("network_reconnect", undefined),
  networkDomainsProbe: () => call("network_domains_probe", undefined),
  autostartGet: () => call("autostart_get", undefined),
  autostartSet: (enabled: boolean) => call("autostart_set", { enabled }),
  referenceCreate: (ids: string[], idempotency_key: string) => call("reference_create", { ids, idempotency_key }),
  referenceList: () => call("reference_list", undefined),
  referenceStatus: (manifest_id: string) => call("reference_status", { manifest_id }),
  referenceStoreManifest: (manifest_id: string, idempotency_key: string) => call("reference_store_manifest", { manifest_id, idempotency_key }),
  referenceArchive: (manifest_id: string, idempotency_key: string) => call("reference_archive", { manifest_id, confirm_trash: true, idempotency_key }),
  referenceRestore: (manifest_id: string, allow_recreate: boolean, idempotency_key: string) => call("reference_restore", { manifest_id, allow_recreate, idempotency_key }),
  uiPrefsGet: () => call("ui_prefs_get", undefined),
  uiPrefsUpdate: (prefs: UiPreferences, expected_account_id: string) => call("ui_prefs_update", { prefs, expected_account_id }),
  accountsList: () => call("accounts_list", undefined),
  accountSwitch: (id: string) => call("account_switch", { id }),
  accountRemove: (id: string) => call("account_remove", { id }),
  playerConfigPick: () => call("player_config_pick",undefined),
  shareOpen: (share_id:string,pass_code?:string) => call("share_open",{share_id,...(pass_code?{pass_code}:{})}),
  shareFolder: (access_ref:string,parent_id:string,cursor?:string) => call("share_folder",{access_ref,parent_id,...(cursor?{cursor}:{}),limit:200}),
  shareRestore: (access_ref:string,ids:string[],target_parent_id:string,idempotency_key:string) => call("share_restore",{access_ref,ids,target_parent_id,idempotency_key}),
  sharePreviewEntry: (access_ref: string, file_id: string) => call("share_preview_entry", { access_ref, file_id }),
  localSourcesList: () => call("local_sources_list", undefined),
  localSourcesAdd: (path?: string) => call("local_sources_add", path ? { path } : {}),
  localSourcesRemove: (source_id: string) => call("local_sources_remove", { source_id }),
  mediaSessionOpen: (remote_id: string, quality_id?: string) => call("media_session_open", { remote_id, ...(quality_id ? { quality_id } : {}) }),
  mediaSessionClose: (session_id: string) => call("media_session_close", { session_id }),
  uploadPickAndStart: (parent_id: string) => call("upload_pick_and_start", { parent_id }),
  uploadDirectoryPickAndStart: (parent_id: string) => call("upload_directory_pick_and_start", { parent_id }),
  downloadDirectoryPick: () => call("download_directory_pick",undefined),
  transferOpenDirectory: (task_id:string) => call("transfer_open_directory",{task_id}),
  transferExport: (task_id:string) => call("transfer_export",{task_id}),
  verificationStatus: (verification_id?: string) => call("verification_status", { verification_id }),
  verificationOpen: (verification_id: string) => call("verification_open", { verification_id }),
  verificationComplete: (verification_id: string) => call("verification_complete", { verification_id }),
  verificationCancel: (verification_id: string) => call("verification_cancel", { verification_id }),
  appStatus: (): Promise<AppStatus> => call("app_status", undefined),
  backendDiscover: () => call("backend_discover", undefined),

  authBegin: (backend: BackendId): Promise<AuthFlow> => call("auth_begin", { backend }),
  /** PROPOSAL UI-001 */
  authCancel: (flow_id: string) => call("auth_cancel", { flow_id }),
  /** PROPOSAL UI-001 §P23 — the password is passed through once and not kept by the UI. */
  authSignIn: (username: string, password: string) => call("auth_sign_in", { username, password }),
  authVerify: (challenge_id: string, code: string, password?: string) => call("auth_verify", { challenge_id, code, ...(password === undefined ? {} : { password }) }),
  authResend: (challenge_id: string) => call("auth_resend", { challenge_id }),
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
  filesRecent: (cursor?: string) => call("files_recent", { limit: 100, ...(cursor ? { cursor } : {}) }),
  /** PROPOSAL UI-001 */
  trashList: (cursor?: string) => call("trash_list", cursor ? { cursor } : {}),

  /** Pass the same key when retrying the same user intent. */
  filesMutate: (op: MutateOp, ids: string[], target: MutateTarget | undefined, idempotency_key: string): Promise<OperationResult> =>
    call("files_mutate", target ? { op, ids, target, idempotency_key } : { op, ids, idempotency_key }),

  cloudTaskCreate: (source_url: string, parent_id: string, idempotency_key: string, pick?: { preview_id: string; file_indices: number[] }): Promise<TaskRef> =>
    call("cloud_task_create", pick ? { source_url, parent_id, idempotency_key, ...pick } : { source_url, parent_id, idempotency_key }),
  /** PROPOSAL UI-001 §P20 */
  cloudTaskPreview: (source_url: string) => call("cloud_task_preview", { source_url }),

  transfersList: (cursor?: string): Promise<Page<TransferTaskExt>> =>
    call("transfers_list", cursor ? { cursor, limit: 500 } : { limit: 500 }),
  /** `options` is PROPOSAL UI-001 §P14 (tier / clip); omitted for a plain download. */
  transferStart: (remote_id: string, destination: string, options?: TransferOptions): Promise<TaskRef> =>
    call("transfer_start", { remote_id, destination, idempotency_key: newIdempotencyKey(), ...(options ? { options } : {}) }),
  /** PROPOSAL UI-001 §P15 — several files and/or folders in one request. */
  transferStartBatch: (remote_ids: string[], destination: string, idempotency_key: string, options?: TransferOptions) =>
    call("transfer_start_batch", options ? { remote_ids, destination, options, idempotency_key } : { remote_ids, destination, idempotency_key }),
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
  syncRunStatus: (id: string) => call("sync_run_status", { id }),

  mountCapabilities: (): Promise<MountCapabilities> => call("mount_capabilities", undefined),
  mountConfigsList: () => call("mount_configs_list", undefined),
  mountConfigSave: (config: MountConfig) => call("mount_config_save", { config }),
  mountStatus: (id: string) => call("mount_status", { id }),
  mountStart: (config_id: string, read_only: boolean): Promise<MountState> => call("mount_start", { config_id, read_only }),
  mountStop: (id: string) => call("mount_stop", { id }),

  /* PROPOSAL UI-001 §P14 media */
  mediaProbe: (remote_id: string) => call("media_probe", { remote_id }),
  mediaAlign: (remote_id: string, quality_id: string, start_ms: number, end_ms: number) => call("media_align", { remote_id, quality_id, start_ms, end_ms }),
  mediaPreviewFrame: (remote_id: string, position_ms: number, max_width = 480) => call("media_preview_frame", { remote_id, position_ms, max_width }),
  /** PROPOSAL UI-001 §P21 — the core launches the player; no URL reaches the UI. */
  mediaOpenExternal: (remote_id: string, quality_id?: string) => call("media_open_external", quality_id ? { remote_id, quality_id } : { remote_id }),

  /** PROPOSAL UI-001 §P16 */
  filesStarred: (cursor?: string) => call("files_starred", cursor ? { cursor, limit: 200 } : { limit: 200 }),

  /** PROPOSAL UI-001 §P17 */
  duplicatesScan: (folder_id: string) => call("duplicates_scan", { folder_id }),
  duplicatesReport: (scan_id: string) => call("duplicates_report", { scan_id }),
  duplicatesCancel: (scan_id: string) => call("duplicates_cancel", { scan_id }),

  /** PROPOSAL UI-001 §P18 */
  spaceSummary: (folder_id: string) => call("files_space_summary", { folder_id }),

  /** PROPOSAL UI-001 §P19 */
  archiveList: (file_id: string, path = "", password_ref?: string) => call("archive_list", password_ref ? { file_id, path, password_ref } : { file_id, path }),
  archiveUnlock: (file_id: string, password: string) => call("archive_unlock", { file_id, password }),
  archiveExtract: (file_id: string, paths: string[] | "all", target_parent_id: string, idempotency_key: string, password_ref?: string) =>
    call("archive_extract", password_ref ? { file_id, paths, target_parent_id, idempotency_key, password_ref } : { file_id, paths, target_parent_id, idempotency_key }),
  jobsList: (cursor?: string) => call("jobs_list", cursor ? { cursor, limit: 200 } : { limit: 200 }),

  /** PROPOSAL UI-001 §P22 */
  shareCreate: (ids: string[], expires_in_days: 1 | 7 | 30 | null, with_code: boolean, idempotency_key: string) => call("share_create", { ids, expires_in_days, with_code, idempotency_key }),
  sharesList: (cursor?: string) => call("shares_list", cursor ? { cursor, limit: 200 } : { limit: 200 }),
  sharesCancel: (share_ids: string[], idempotency_key: string) => call("shares_cancel", { share_ids, idempotency_key }),

  settingsGet: (): Promise<Settings> => call("settings_get", undefined),
  settingsUpdate: (patch: SettingsPatch): Promise<Settings> => call("settings_update", { patch }),
};

/** Load every page of a cursor-paged command (used for small lists only). */
export async function collectAll<T>(first: () => Promise<Page<T>>, next: (cursor: string) => Promise<Page<T>>, max = 50): Promise<T[]> {
  const out: T[] = [];
  let page = await first();
  out.push(...page.items);
  let n = 1;
  const seen = new Set<string>();
  while (page.next_cursor && n++ < max) {
    if (seen.has(page.next_cursor)) throw new ApiError("transfers_list", { code: "CONFLICT", message: tr("服务返回重复分页，未接受不完整快照。"), retryable: false });
    seen.add(page.next_cursor);
    page = await next(page.next_cursor);
    out.push(...page.items);
  }
  if (page.next_cursor) throw new ApiError("transfers_list", { code: "CONFLICT", message: tr("快照超过分页安全上限，未接受不完整列表。"), retryable: false });
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
 * Open an external HTTPS link with the registered, HTTPS-scoped Tauri opener.
 * Copying is a visible fallback when the system browser cannot be launched.
 */
export async function openExternal(url: string): Promise<"opened" | "copied" | "failed"> {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" || parsed.username || parsed.password) return "failed";
  } catch { return "failed"; }
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
