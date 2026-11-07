/**
 * PROPOSAL UI-001 — NOT FROZEN.
 *
 * docs/03_CONTRACTS.md §3 names these return types but does not define
 * them. The shapes below are the UI's proposal to Astra (see
 * docs/CONTRACT_PROPOSAL_UI_001.md). Everything here is additive: no
 * frozen field in contract.ts is renamed or re-purposed.
 *
 * When Astra freezes a shape, move it into contract.ts and delete it here.
 */
import type {
  BackendId,
  CapabilityReport,
  ClientError,
  Page,
  RemoteEntry,
  SyncPlan,
  TaskState,
  TransferTask,
} from "./contract";

/** §P0 Root folder sentinel accepted by `files_list.parent_id`. */
export const ROOT_ID = "root";

/** §P1 `app_status` */
export interface AppStatus {
  version: string;
  contract_version: string;
  /** "mock" when the Mock adapter answers, "tauri" when real IPC answers. */
  runtime: "mock" | "tauri";
  /** Integrated display model; the frozen Rust wire type remains in api-types.ts. */
  mode?: "mock" | "official_cli_preview";
  storage: {
    status: "ok" | "degraded" | "corrupt_readonly";
    message?: string;
  };
  feature_flags: Record<string, boolean>;
}

/** §P1 `auth_begin` — the UI never collects passwords. */
export interface AuthFlow {
  flow_id: string;
  backend: BackendId;
  kind: "external_browser" | "external_cli";
  /** Human-readable hint, e.g. "已在系统浏览器打开官方授权页". Must not contain tokens. */
  instructions: string;
  expires_at?: string;
}

/** §P1 `auth_status` */
export interface AuthState {
  state: "signed_out" | "pending" | "signed_in" | "expired" | "demo";
  message?: string;
  credential_owner?: string;
  backend?: BackendId;
  account?: {
    /** Already redacted by Rust, e.g. "k***@gmail.com". */
    display_name: string;
    tier: "free" | "premium" | "unknown";
  };
  checked_at: string;
}

/** §P2 `files_mutate.op` — enumerated so both sides agree. */
export type MutateOp =
  | "mkdir" | "rename" | "move" | "copy" | "trash"
  | "restore" | "delete_forever";

/** §P2 `files_mutate.target` */
export interface MutateTarget {
  parent_id?: string;
  name?: string;
}

/** §P2 `files_mutate` result */
export interface OperationResult {
  op: MutateOp;
  succeeded: string[];
  failed: { id: string; error: ClientError }[];
  created?: RemoteEntry[];
  /** true when this idempotency_key was already applied and nothing ran again. */
  replayed: boolean;
}

/** §P1 generic task handle */
export interface TaskRef {
  task_id: string;
}

export type TransferAction = "pause" | "resume" | "cancel" | "retry";

/** §P3 additive optional fields the transfer list needs to render. */
export interface TransferTaskExt extends TransferTask {
  /** Display name (file name, or redacted source label for cloud tasks). */
  name?: string;
  /** Local destination path for downloads, remote folder for uploads/cloud. */
  destination?: string;
  /** Monotonic per-task revision for discarding stale/out-of-order events. */
  revision?: string | number;
}

/** §P1 `quota_get` */
export type QuotaDimension =
  | "downstream_monthly"
  | "upload_monthly"
  | "cloud_download_monthly"
  | "downstream_daily";

export interface QuotaItem {
  dimension: QuotaDimension;
  scope: "connected_apps" | "account";
  unit: "bytes" | "count";
  /** decimal u64 strings when unit = bytes */
  limit?: string;
  used?: string;
  remaining?: string;
  /** exact = reported by backend; estimated = local estimate; unavailable = unknown */
  exactness: "exact" | "estimated" | "unavailable";
  state: "ok" | "near_limit" | "exhausted" | "unknown";
  resets_at?: string;
  note?: string;
}

export interface QuotaReport {
  backend: BackendId;
  account_tier: "free" | "premium" | "unknown";
  items: QuotaItem[];
  checked_at: string;
}

/** §P5 sync configuration (no command exists yet in §3 to list/save). */
export interface SyncConfig {
  config_id: string;
  name: string;
  mode: SyncPlan["mode"];
  local_path: string;
  remote_id: string;
  remote_path: string;
  exclude_globs: string[];
  allow_deletes: boolean;
  /** true until a first successful apply established a baseline. */
  first_run: boolean;
  last_applied_at?: string;
}

/** §P1 `sync_scan` */
export interface ScanRef {
  scan_id: string;
}

/** §P1 `sync_history` */
export interface SyncEvent {
  id: string;
  config_id: string;
  at: string;
  kind: "scan" | "plan" | "apply" | "error";
  summary: string;
  error?: ClientError;
}

/** §P1 `mount_capabilities` */
export type MountMode = "cloud_files_sync_root" | "winfsp_drive";

export interface MountCheck {
  id: string;
  label: string;
  status: "pass" | "fail" | "unknown";
  detail: string;
  /** What the user must install/enable themselves. Never auto-installed. */
  requirement?: string;
}

export interface MountModeReport {
  mode: MountMode;
  available: boolean;
  checks: MountCheck[];
}

export interface MountCapabilities {
  platform: "windows" | "linux" | "macos" | "unknown";
  modes: MountModeReport[];
  checked_at: string;
}

/** §P1 `mount_start` */
export interface MountState {
  id: string;
  mode: MountMode;
  status: "mounting" | "mounted" | "unmounting" | "stopped" | "error";
  read_only: boolean;
  mount_point?: string;
  error?: ClientError;
}

/** §P1 `settings_get` / `settings_update` */
export interface Settings {
  schema_version: 1;
  theme: "system" | "light" | "dark";
  download_dir: string;
  max_concurrent_transfers: number;
  /** 0 = unlimited */
  bandwidth_limit_kib_s: number;
  cache: {
    dir: string;
    /** decimal u64 */
    max_bytes: string;
  };
  experimental_network: {
    enabled: boolean;
    proxy_mode: "system" | "none" | "user_provided";
    /** Never returned in clear text; only whether one is stored. */
    user_proxy_configured: boolean;
    custom_dns_mode: false;
    connection_diagnostics: boolean;
  };
  telemetry: false;
}

export type SettingsPatch = Partial<Omit<Settings, "schema_version" | "cache" | "experimental_network">> & {
  cache?: Partial<Settings["cache"]>;
  experimental_network?: Partial<Omit<Settings["experimental_network"], "user_proxy_configured">>;
};

/** §P4 auth event (no event for auth completion exists in §4). */
export interface AuthChangedEvent {
  state: AuthState["state"];
  flow_id?: string;
  error?: ClientError;
}

export const PROPOSED_EVENTS = {
  authChanged: "nekopak://auth/changed",
} as const;

/** Full command map: name -> [args, result]. Frozen names from §3 + §P commands. */
export interface CommandMap {
  // §3 frozen names
  app_status: [undefined, AppStatus];
  backend_discover: [undefined, CapabilityReport[]];
  auth_begin: [{ backend: BackendId }, AuthFlow];
  auth_status: [undefined, AuthState];
  auth_logout: [{ backend: BackendId }, void];
  files_list: [{ parent_id: string; cursor?: string; limit?: number }, Page<RemoteEntry>];
  files_stat: [{ id: string }, RemoteEntry];
  files_mutate: [{ op: MutateOp; ids: string[]; target?: MutateTarget; idempotency_key: string }, OperationResult];
  cloud_task_create: [{ source_url: string; parent_id: string; idempotency_key: string }, TaskRef];
  transfers_list: [{ cursor?: string; limit?: number }, Page<TransferTaskExt>];
  transfer_start: [{ remote_id: string; destination: string; idempotency_key?: string }, TaskRef];
  transfer_control: [{ task_id: string; action: TransferAction }, void];
  quota_get: [undefined, QuotaReport];
  sync_scan: [{ config_id: string }, ScanRef];
  sync_plan: [{ config_id: string }, SyncPlan];
  sync_apply: [{ plan_id: string; confirm_deletes: boolean }, TaskRef];
  sync_history: [{ config_id: string; cursor?: string }, Page<SyncEvent>];
  mount_capabilities: [undefined, MountCapabilities];
  mount_start: [{ config_id: string; read_only: boolean }, MountState];
  mount_stop: [{ id: string }, void];
  settings_get: [undefined, Settings];
  settings_update: [{ patch: SettingsPatch }, Settings];

  // §P proposals (UI degrades to "UNSUPPORTED" if the backend lacks them)
  auth_cancel: [{ flow_id: string }, void];
  files_search: [{ query: string; parent_id?: string; cursor?: string; limit?: number }, Page<RemoteEntry>];
  files_recent: [{ cursor?: string; limit?: number }, Page<RemoteEntry>];
  files_path: [{ id: string }, RemoteEntry[]];
  trash_list: [{ cursor?: string; limit?: number }, Page<RemoteEntry>];
  sync_configs_list: [undefined, SyncConfig[]];
  sync_config_save: [{ config: SyncConfig }, SyncConfig];
}

export type CommandName = keyof CommandMap;
export type CommandArgs<K extends CommandName> = CommandMap[K][0];
export type CommandResult<K extends CommandName> = CommandMap[K][1];

/** Commands the UI uses that are NOT in 03_CONTRACTS §3 yet. */
export const PROPOSED_COMMANDS: ReadonlySet<CommandName> = new Set<CommandName>([
  "auth_cancel",
  "files_search",
  "files_recent",
  "files_path",
  "trash_list",
  "sync_configs_list",
  "sync_config_save",
]);

export const ACTIVE_STATES: ReadonlySet<TaskState> = new Set<TaskState>([
  "queued", "running", "paused", "retrying", "verifying",
]);
