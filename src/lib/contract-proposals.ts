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
  system_locale?: string;
  version: string;
  contract_version: string;
  /** "mock" when the Mock adapter answers, "tauri" when real IPC answers. */
  runtime: "mock" | "tauri";
  /** Actual Rust execution mode, independent of the browser transport. */
  mode?: "mock" | "rust_compat_preview";
  storage: {
    status: "ok" | "degraded" | "corrupt_readonly";
    message?: string;
  };
  feature_flags: Record<string, boolean>;
}

/**
 * §P1 `auth_begin` — kept for backends with a browser authorisation flow
 * (WebDAV / Rclone). PikPak itself signs in natively, see §P23.
 */
export interface AuthFlow {
  flow_id: string;
  backend: BackendId;
  kind: "external_browser";
  /** Human-readable hint, e.g. "已在系统浏览器打开官方授权页". Must not contain tokens. */
  instructions: string;
  expires_at?: string;
}

/**
 * §P23 Native sign-in. The UI sends the account and password once; the
 * core exchanges them for tokens kept in the OS credential store and never
 * persists the password. A login may need a one-time code (SMS / e-mail).
 */
export type SignInChallenge =
  | {
      challenge_id: string;
      /** PikPak's human check: open `url`, finish it, then call auth_verify with an empty code. */
      kind: "captcha";
      url: string;
      /** Native flow discards the first password and asks for a fresh submission. */
      requires_password?: boolean;
      expires_at?: string;
    }
  | {
      challenge_id: string;
      /** One-time code (sms / e-mail). Not yet verified against PikPak. */
      kind: "verify_code";
      channel: "sms" | "email";
      /** Masked by the core, e.g. "+86 138****0000". */
      target: string;
      code_length: number;
      resend_after_s?: number;
      expires_at?: string;
    };

export interface SignInResult {
  state: "signed_in" | "challenge";
  challenge?: SignInChallenge;
}

/** DRIVE challenge is separate from signing in; contains no secret URL or token. */
export interface VerificationState {
  verification_id: string;
  state: "pending" | "verified" | "expired";
  expires_at: string;
}

/** §P1 `auth_status` */
export interface AuthState {
  state: "signed_out" | "pending" | "signed_in" | "expired" | "demo";
  account_id?: string;
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

export interface SavedAccount {
  id: string;
  display_name: string;
  active: boolean;
  state: "signed_in" | "expired";
}

/** Rust stores these fields within the active account's scope. */
export interface UiPreferences {
  pins?: { id: string; name: string }[];
  view?: "list" | "grid";
  sort?: { key: "name" | "size" | "modified" | "kind"; dir: "asc" | "desc" };
  density?: "comfortable" | "compact";
  gridSize?: "s" | "m" | "l";
  foldersFirst?: boolean;
}

/**
 * §P2 `files_mutate.op` — enumerated so both sides agree.
 * §P16 adds "star" / "unstar" (server-side starred flag).
 */
export type MutateOp =
  | "mkdir" | "rename" | "move" | "copy" | "trash"
  | "restore" | "delete_forever" | "star" | "unstar";

/**
 * §P13 Extra capability keys a backend may report in
 * `CapabilityReport.operations` (additive JSON keys; the frozen §2
 * `Operation` union is untouched). A missing key means "not supported".
 */
export type ExtOperation =
  | "media_probe"      // §P14 duration, qualities, keyframe index
  | "clip_download"    // §P14 download a time range of a video
  | "quality_download" // §P14 download a transcoded tier
  | "folder_download"  // §P15 expand folders into download tasks
  | "star"             // §P16
  | "duplicates"       // §P17
  | "space_summary"    // §P18
  | "archive"          // §P19 list / extract archives server-side
  | "link_preview"     // §P20 inspect a link before saving
  | "external_player"  // §P21 hand a stream to the system player
  | "share";           // §P22

export type AnyOperation = import("./contract").Operation | ExtOperation;

/** §P16 RemoteEntry may carry `starred` (additive, optional). */
export type EntryExt = RemoteEntry & { starred?: boolean };

export function isStarred(e: RemoteEntry): boolean {
  return (e as EntryExt).starred === true;
}

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

/** Per-item outcome for bulk commands that are not `files_mutate`. */
export interface BulkResult {
  succeeded: string[];
  failed: { id: string; error: ClientError }[];
  replayed: boolean;
}

/** §P1 generic task handle */
export interface TaskRef {
  task_id: string;
}

export type TransferAction = "pause" | "resume" | "cancel" | "retry";

/** §P3 additive optional fields the transfer list needs to render. */
export interface TransferTaskExt extends TransferTask {
  /** Server-reported percent, independent of unknown cloud byte counts. */
  progress_percent?: number;
  /** Display name (file name, or redacted source label for cloud tasks). */
  name?: string;
  /** Local destination path for downloads, remote folder for uploads/cloud. */
  destination?: string;
  /** Monotonic per-task revision for discarding stale/out-of-order events. */
  revision?: string | number;
  /** §P14 transcoded tier label when not the original file, e.g. "1080P". */
  quality_label?: string;
  /** §P14 present when this task downloads a time range of a video. */
  clip?: { start_ms: number; end_ms: number };
  /** §P15 tasks expanded from one folder download share this id. */
  group_id?: string;
}

/* ------------------------------------------------------------------ */
/* §P14 Media: probe, keyframe alignment, preview, clip / tier download */
/* ------------------------------------------------------------------ */

export interface QualityOption {
  /** "original" or a tier id such as "1080p". */
  id: string;
  label: string;
  width?: number;
  height?: number;
  /** decimal u64; absent when the backend cannot tell yet */
  size_bytes?: string;
  /** Transcoded tiers are saved as MP4; the original keeps its container. */
  container: "original" | "mp4";
  /** false when the transcode for this tier cannot be read right now. */
  readable: boolean;
  unreadable_reason?: string;
}

export interface MediaProbe {
  tracks?: { id: number; kind: string; codec: string; language?: string; duration_ms: number; width?: number; height?: number }[];
  warnings?: string[];
  remote_id: string;
  duration_ms: number;
  container?: string;
  video_codec?: string;
  width?: number;
  height?: number;
  qualities: QualityOption[];
  /**
   * indexed: the backend read the container's keyframe index;
   * estimated: keyframes are located by scanning near the cut;
   * unavailable: a valid clip cannot be produced.
   */
  keyframes: "indexed" | "estimated" | "unavailable";
  /** Whether `media_preview_frame` can return frames for this file. */
  preview: "available" | "unavailable";
  preview_reason?: string;
  probed_at: string;
}

export interface ClipRange {
  start_ms: number;
  end_ms: number;
}

/** `media_align` result: the range the backend will actually cut. */
export interface ClipAlignment extends ClipRange {
  quality_id: string;
  requested_start_ms: number;
  /** decimal u64 */
  estimated_bytes?: string;
  exactness: "exact" | "estimated";
}

export interface PreviewFrame {
  position_ms: number;
  mime: "image/jpeg" | "image/webp" | "image/svg+xml";
  data_base64: string;
  /** true for Mock's generated pictures (never real video frames). */
  synthetic?: boolean;
}

/** Optional `transfer_start.options` (additive). */
export interface TransferOptions {
  /** Exact tier chosen in “下载指定画质…”. */
  quality_id?: string;
  /** Upper bound for ordinary downloads: best tier not above this. */
  quality_cap?: QualityCap;
  clip?: ClipRange & { align: "previous_keyframe" };
}

export type QualityCap = "original" | "2160p" | "1080p" | "720p" | "480p";

/* ------------------------------------------------------------------ */
/* §P15 Batch / folder download                                        */
/* ------------------------------------------------------------------ */

export interface BatchRef {
  task_ids: string[];
  /** Items that were not queued, with a reason (e.g. too many files). */
  skipped: { id: string; name?: string; reason: string }[];
  /** Number of files found inside folders. */
  expanded_files: number;
  group_id?: string;
}

/* ------------------------------------------------------------------ */
/* §P17 Duplicates                                                     */
/* ------------------------------------------------------------------ */

export interface DuplicateEntry extends EntryExt {
  /** Cloud path, e.g. "/影视/纪录片 S01E01.mp4". */
  path: string;
  /** true when the account cannot trash this copy (e.g. shared with me). */
  read_only?: boolean;
}

export interface DuplicateGroup {
  group_id: string;
  /** Only byte-identical content (same hash and size) is grouped. */
  match: "hash";
  hash?: RemoteEntry["hash"];
  size_bytes: string;
  entries: DuplicateEntry[];
}

export interface DuplicateReport {
  scan_id: string;
  folder_id: string;
  folder_path: string;
  state: "running" | "completed" | "failed" | "cancelled";
  groups: DuplicateGroup[];
  files_scanned: number;
  folders_scanned: number;
  /** Sum over groups of (n-1) × size, decimal u64. */
  reclaimable_bytes: string;
  /** Files without a hash cannot be compared and are listed here. */
  unhashed_files: number;
  skipped: { path: string; reason: string }[];
  completed_at?: string;
  error?: ClientError;
}

export interface DuplicatesProgressEvent {
  scan_id: string;
  state: DuplicateReport["state"];
  files_scanned: number;
  folders_scanned: number;
  error?: ClientError;
}

/* ------------------------------------------------------------------ */
/* §P18 Space usage                                                    */
/* ------------------------------------------------------------------ */

export interface SpaceNode {
  id: string;
  name: string;
  kind: "file" | "folder";
  /** decimal u64, recursive for folders */
  size_bytes: string;
  file_count: number;
  mime_type?: string;
}

export interface SpaceSummary {
  folder_id: string;
  total_bytes: string;
  file_count: number;
  folder_count: number;
  children: SpaceNode[];
  /** partial: some subfolders could not be read; sizes are lower bounds. */
  exactness: "exact" | "partial";
  note?: string;
  computed_at: string;
}

/* ------------------------------------------------------------------ */
/* §P19 Archives (server-side listing and extraction)                  */
/* ------------------------------------------------------------------ */

export interface ArchiveEntry {
  /** Path inside the archive, "/"-separated, no leading slash. */
  path: string;
  name: string;
  kind: "file" | "folder";
  size_bytes: string;
  modified_at?: string;
}

export interface ArchiveListing {
  file_id: string;
  /** Folder inside the archive being listed ("" = top level). */
  path: string;
  format: "zip" | "rar" | "7z" | "unknown";
  /** false for multi-volume or unsupported formats; `reason` explains. */
  supported: boolean;
  reason?: string;
  encrypted: boolean;
  /** true until a valid password_ref is supplied for an encrypted archive. */
  needs_password: boolean;
  entries: ArchiveEntry[];
}

/** The password goes to the backend once; the UI only keeps this opaque ref. */
export interface ArchiveUnlock {
  password_ref: string;
}

/* ------------------------------------------------------------------ */
/* §P19 Server-side jobs (extraction etc.) — snapshot + events          */
/* ------------------------------------------------------------------ */

export interface ServerJob {
  job_id: string;
  kind: "archive_extract";
  state: "queued" | "running" | "completed" | "failed" | "cancelled";
  title: string;
  /** 0–100 when the server reports it */
  progress?: number;
  target_parent_id?: string;
  target_path?: string;
  error?: ClientError;
  created_at: string;
  updated_at: string;
  revision: string | number;
}

/* ------------------------------------------------------------------ */
/* §P20 Link preview before saving                                     */
/* ------------------------------------------------------------------ */

export interface LinkFile {
  index: number;
  /** Path inside the resource, "/"-separated. */
  path: string;
  size_bytes: string;
}

export interface LinkPreview {
  /** false means the service can save only the entire resource. Missing preserves old Mock support. */
  selection_supported?: boolean;
  preview_id: string;
  source_kind: "magnet" | "http";
  title: string;
  total_bytes?: string;
  files: LinkFile[];
  expires_at: string;
}

/* ------------------------------------------------------------------ */
/* §P21 External player                                                */
/* ------------------------------------------------------------------ */

export interface ExternalPlayResult {
  /** The stream URL never crosses IPC; the core launches the player. */
  launched: boolean;
  player?: string;
  note?: string;
}
export interface MediaSession {
  session_id: string;
  /** Rust-owned loopback proxy only; never the upstream signed URL. */
  url: string;
  mime_type: string;
  name?: string;
  expires_at?: string;
}
export interface LocalSource { source_id: string; root_id?: string; name: string; path: string }

/* ------------------------------------------------------------------ */
/* §P22 Shares                                                         */
/* ------------------------------------------------------------------ */

export interface ShareInfo {
  expires_known?: boolean;
  share_id: string;
  title: string;
  item_count: number;
  /** Public share page link (not a signed download URL). */
  url: string;
  /** Extraction code when one was requested. */
  code?: string;
  created_at: string;
  expires_at?: string;
  state: "active" | "expired" | "cancelled" | "unavailable";
  view_count?: number;
  save_count?: number;
}
export interface SharedFolder { access_ref:string;share_id:string;title:string;parent_id:string;entries:RemoteEntry[];next_cursor?:string }

export interface ReferenceManifest {
  manifest_id: string;
  account_id: string;
  created_at: string;
  content_sha256: string;
  state: "prepared" | "uploading" | "verified" | "partial" | "archived" | "restored";
  roots: string[];
  entries: { path: string; entry: RemoteEntry; state: "recorded" | "trashed" | "restored" | "pending_upload" | "outcome_unknown" | "failed"; current_id?: string; error?: ClientError }[];
  manifest_task_id?: string;
  remote_manifest_id?: string;
}

/** §P1 `quota_get` */
export type QuotaDimension =
  | "storage"
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
  auto_enabled?: boolean;
  interval_seconds?: number;
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
  notice?: string;
  notice_url?: string;
}

export interface MountConfig {
  config_id: string;
  mode: MountMode;
  remote_id: string;
  local_path?: string;
  drive_letter?: string;
}

export interface MountCapabilities {
  platform: "windows" | "linux" | "macos" | "unknown";
  modes: MountModeReport[];
  checked_at: string;
}

/** §P1 `mount_start` */
export interface MountState {
  id: string;
  mode?: MountMode;
  status: "mounting" | "mounted" | "unmounting" | "stopped" | "error";
  read_only: boolean;
  mount_point?: string;
  error?: ClientError;
}

/** §P1 `settings_get` / `settings_update` */
export interface Settings {
  media?: { player_path: string };
  ask_download_directory?: boolean;
  close_to_tray?: boolean;
  language?: "system" | "zh-CN" | "zh-TW" | "en" | "ja-JP" | "ko-KR";
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
    server_domain?: ServerDomain;
    enabled: boolean;
    proxy_mode: "system" | "none" | "user_provided";
    /** Never returned in clear text; only whether one is stored. */
    user_proxy_configured: boolean;
    custom_dns_mode: false;
    connection_diagnostics: boolean;
    proxy_url?: string;
    compatibility_mode?: boolean;
    /** App-scoped request blocking; independent of enabled, off by default. */
    block_access_domains?: boolean;
  };
  telemetry: false;
  /** §P14/§P15 download behaviour (additive). */
  download?: {
    /** Upper bound used by ordinary downloads of videos. */
    default_quality: QualityCap;
    /** Parallel connections per task, 1–16. Never raised on throttling. */
    connections_per_task: number;
  };
}

export type SettingsPatch = Partial<Omit<Settings, "schema_version" | "cache" | "experimental_network" | "download">> & {
  cache?: Partial<Settings["cache"]>;
  experimental_network?: Partial<Omit<Settings["experimental_network"], "user_proxy_configured">>;
  download?: Partial<NonNullable<Settings["download"]>>;
};

/** §P4 auth event (no event for auth completion exists in §4). */
export interface AuthChangedEvent {
  state: AuthState["state"];
  flow_id?: string;
  error?: ClientError;
}

export const PROPOSED_EVENTS = {
  authChanged: "nekopak://auth/changed",
  duplicatesProgress: "nekopak://duplicates/progress",
  jobChanged: "nekopak://job/changed",
} as const;

export type ServerDomain = "auto" | "mypikpak.com" | "mypikpak.net" | "pikpak.me" | "pikpakdrive.com";
export interface AuthPreferences { remember_account: boolean; restore_session: boolean; username?: string }
export interface NetworkDomainReport {
  selected_domain: ServerDomain; active_domain: Exclude<ServerDomain, "auto">; checked_at: string;
  results: { domain: Exclude<ServerDomain, "auto">; usable: boolean; latency_ms?: number; warm_latency_ms?: number; error?: string }[];
}

/** Full command map: name -> [args, result]. Frozen names from §3 + §P commands. */
export interface CommandMap {
  transfer_forget: [{ task_id: string }, void];
  transfer_clear_finished: [undefined, { removed: number }];
  auth_preferences_get: [undefined, AuthPreferences];
  auth_preferences_update: [{ preferences: AuthPreferences }, AuthPreferences];
  network_reconnect: [undefined, void];
  network_domains_probe: [undefined, NetworkDomainReport];
  autostart_get: [undefined, { enabled: boolean; available: boolean }];
  autostart_set: [{ enabled: boolean }, { enabled: boolean; available: boolean }];
  reference_create: [{ ids: string[]; idempotency_key: string }, ReferenceManifest];
  reference_list: [undefined, ReferenceManifest[]];
  reference_status: [{ manifest_id: string }, ReferenceManifest];
  reference_store_manifest: [{ manifest_id: string; idempotency_key: string }, TaskRef];
  reference_archive: [{ manifest_id: string; confirm_trash: true; idempotency_key: string }, ReferenceManifest];
  reference_restore: [{ manifest_id: string; allow_recreate: boolean; idempotency_key: string }, ReferenceManifest];
  ui_prefs_get: [undefined, { account_id: string; prefs: UiPreferences }];
  ui_prefs_update: [{ expected_account_id: string; prefs: UiPreferences }, UiPreferences];
  accounts_list: [undefined, SavedAccount[]];
  account_switch: [{ id: string }, AuthState];
  account_remove: [{ id: string }, void];
  player_config_pick: [undefined,string|null];
  share_open: [{share_id:string;pass_code?:string},SharedFolder];
  share_folder: [{access_ref:string;parent_id:string;cursor?:string;limit?:number},SharedFolder];
  share_restore: [{access_ref:string;ids:string[];target_parent_id:string;idempotency_key:string},BulkResult];
  /** Mint a read-only shared source handle; entries in share_folder keep original IDs. */
  share_preview_entry: [{ access_ref: string; file_id: string }, RemoteEntry];
  local_sources_list: [undefined, LocalSource[]];
  local_sources_add: [{ path?: string }, LocalSource | null];
  local_sources_remove: [{ source_id: string }, void];
  media_session_open: [{ remote_id: string; quality_id?: string }, MediaSession];
  media_session_close: [{ session_id: string }, void];
  upload_pick_and_start: [{ parent_id: string }, BatchRef | null];
  upload_directory_pick_and_start: [{ parent_id: string }, BatchRef];
  desktop_status: [undefined,{tray_available:boolean;window_visible:boolean}];
  download_directory_pick: [undefined, string | null];
  transfer_open_directory: [{task_id:string}, void];
  transfer_export: [{task_id:string}, boolean];
  verification_status: [{ verification_id?: string }, VerificationState | null];
  verification_open: [{ verification_id: string }, void];
  verification_complete: [{ verification_id: string }, void];
  verification_cancel: [{ verification_id: string }, void];
  // §3 frozen names
  app_status: [undefined, AppStatus];
  backend_discover: [undefined, CapabilityReport[]];
  auth_begin: [{ backend: BackendId }, AuthFlow];
  auth_status: [undefined, AuthState];
  auth_logout: [{ backend: BackendId }, void];
  files_list: [{ parent_id: string; cursor?: string; limit?: number }, Page<RemoteEntry>];
  files_stat: [{ id: string }, RemoteEntry];
  files_mutate: [{ op: MutateOp; ids: string[]; target?: MutateTarget; idempotency_key: string }, OperationResult];
  /** §P20 adds optional `preview_id` + `file_indices` to save only some files. */
  cloud_task_create: [{ source_url: string; parent_id: string; idempotency_key: string; preview_id?: string; file_indices?: number[] }, TaskRef];
  transfers_list: [{ cursor?: string; limit?: number }, Page<TransferTaskExt>];
  /** §P14 adds optional `options` (tier / clip). */
  transfer_start: [{ remote_id: string; destination: string; options?: TransferOptions; idempotency_key?: string }, TaskRef];
  transfer_control: [{ task_id: string; action: TransferAction }, void];
  quota_get: [undefined, QuotaReport];
  sync_scan: [{ config_id: string }, ScanRef];
  sync_plan: [{ config_id: string }, SyncPlan];
  sync_apply: [{ plan_id: string; confirm_deletes: boolean }, TaskRef];
  sync_history: [{ config_id: string; cursor?: string }, Page<SyncEvent>];
  sync_run_status: [{ id: string }, {job_id:string;state:"queued"|"running"|"completed"|"failed"|"cancelled";error?:ClientError}];
  mount_capabilities: [undefined, MountCapabilities];
  mount_configs_list: [undefined, MountConfig[]];
  mount_config_save: [{ config: MountConfig }, MountConfig];
  mount_status: [{ id: string }, MountState];
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

  // §P23 native sign-in (wrong password → PERMISSION_DENIED, details.reason = "invalid_credentials")
  auth_sign_in: [{ username: string; password: string }, SignInResult];
  auth_verify: [{ challenge_id: string; code: string; password?: string }, SignInResult];
  auth_resend: [{ challenge_id: string }, { resend_after_s: number }];

  // §P14 media
  media_probe: [{ remote_id: string }, MediaProbe];
  media_align: [{ remote_id: string; quality_id: string; start_ms: number; end_ms: number }, ClipAlignment];
  media_preview_frame: [{ remote_id: string; position_ms: number; max_width: number }, PreviewFrame];
  // §P15
  transfer_start_batch: [{ remote_ids: string[]; destination: string; options?: TransferOptions; idempotency_key: string }, BatchRef];
  // §P16
  files_starred: [{ cursor?: string; limit?: number }, Page<RemoteEntry>];
  // §P17
  duplicates_scan: [{ folder_id: string }, ScanRef];
  duplicates_report: [{ scan_id: string }, DuplicateReport];
  duplicates_cancel: [{ scan_id: string }, void];
  // §P18
  files_space_summary: [{ folder_id: string }, SpaceSummary];
  // §P19
  archive_list: [{ file_id: string; path?: string; password_ref?: string }, ArchiveListing];
  archive_unlock: [{ file_id: string; password: string }, ArchiveUnlock];
  archive_extract: [{ file_id: string; paths: string[] | "all"; target_parent_id: string; password_ref?: string; idempotency_key: string }, { job_id: string }];
  jobs_list: [{ cursor?: string; limit?: number }, Page<ServerJob>];
  // §P20
  cloud_task_preview: [{ source_url: string }, LinkPreview];
  // §P21
  media_open_external: [{ remote_id: string; quality_id?: string }, ExternalPlayResult];
  // §P22
  share_create: [{ ids: string[]; expires_in_days: 1 | 7 | 30 | null; with_code: boolean; idempotency_key: string }, ShareInfo];
  shares_list: [{ cursor?: string; limit?: number }, Page<ShareInfo>];
  shares_cancel: [{ share_ids: string[]; idempotency_key: string }, BulkResult];
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
  "auth_sign_in",
  "auth_verify",
  "auth_resend",
  "media_probe",
  "media_align",
  "media_preview_frame",
  "transfer_start_batch",
  "files_starred",
  "duplicates_scan",
  "duplicates_report",
  "duplicates_cancel",
  "files_space_summary",
  "archive_list",
  "archive_unlock",
  "archive_extract",
  "jobs_list",
  "cloud_task_preview",
  "media_open_external",
  "share_create",
  "shares_list",
  "shares_cancel",
]);

export const ACTIVE_STATES: ReadonlySet<TaskState> = new Set<TaskState>([
  "queued", "running", "paused", "retrying", "verifying",
]);
