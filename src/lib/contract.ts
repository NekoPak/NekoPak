/**
 * NekoPak Rust <-> UI contract v0.1 — FROZEN BASELINE.
 *
 * Mirrors docs/03_CONTRACTS.md §2 and §4 verbatim. Do NOT rename fields or
 * change meanings here; additive proposals live in `contract-proposals.ts`
 * and docs/CONTRACT_PROPOSAL_UI_001.md until Astra and Opus both sign off.
 *
 * Cross-boundary JSON is snake_case. u64 values travel as decimal strings.
 */

export const CONTRACT_VERSION = "0.1";

export type BackendId = "official_cli" | "rust_compat" | "webdav" | "rclone" | "mock";

export type Operation =
  | "list" | "stat" | "search" | "mkdir" | "rename" | "move"
  | "copy" | "trash" | "upload" | "download" | "cloud_task"
  | "stream_url" | "read_range" | "quota" | "segment_download";

export interface CapabilityReport {
  backend: BackendId;
  status: "ready" | "needs_login" | "not_installed" | "unavailable";
  operations: Partial<Record<Operation, boolean>>;
  account_tier?: "free" | "premium" | "unknown";
  version?: string;
  limitations: string[];
  checked_at: string;
}

export interface RemoteEntry {
  id: string;
  parent_id?: string;
  name: string;
  kind: "file" | "folder";
  size_bytes: string; // decimal u64
  modified_at?: string;
  etag?: string;
  hash?: { algorithm: "md5" | "sha1" | "sha256"; value: string };
  mime_type?: string;
}

export interface Page<T> {
  items: T[];
  next_cursor?: string;
}

export type TaskState =
  | "queued" | "running" | "paused" | "retrying" | "verifying"
  | "completed" | "failed" | "cancelled";

export interface TransferTask {
  id: string;
  remote_id: string;
  kind: "download" | "upload" | "cloud_download" | "segment_download";
  state: TaskState;
  transferred_bytes: string;
  total_bytes?: string;
  speed_bytes_per_sec?: number;
  backend: BackendId;
  error?: ClientError;
  created_at: string;
  updated_at: string;
  simulated?: boolean;
}

export type ClientErrorCode =
  | "AUTH_EXPIRED" | "ACCOUNT_RESTRICTED" | "PERMISSION_DENIED"
  | "QUOTA_EXCEEDED" | "NOT_FOUND" | "UNSUPPORTED"
  | "TRANSIENT_NETWORK" | "DISK_FULL" | "INTEGRITY_FAILED"
  | "CONFLICT" | "CANCELLED" | "INTERNAL";

export interface ClientError {
  code: ClientErrorCode;
  message: string;
  retryable: boolean;
  backend?: BackendId;
  request_id?: string;
  details?: Record<string, unknown>; // redacted
}

export interface SyncPlan {
  plan_id: string;
  base_revision: string;
  source: string;
  destination: string;
  mode: "upload_only" | "download_only" | "bidirectional";
  actions: SyncAction[];
  warnings: string[];
  has_unresolved_conflicts: boolean;
  has_deletes: boolean;
}

export interface SyncAction {
  id: string;
  op: "upload" | "download" | "mkdir" | "keep_both" | "delete" | "skip";
  path: string;
  reason: string;
  destructive: boolean;
}

/* ------------------------------------------------------------------ */
/* §4 Async events                                                     */
/* ------------------------------------------------------------------ */

export const EVENTS = {
  backendStatus: "nekopak://backend/status",
  transferProgress: "nekopak://transfer/progress",
  transferChanged: "nekopak://transfer/changed",
  syncScanProgress: "nekopak://sync/scan-progress",
  syncPlanReady: "nekopak://sync/plan-ready",
  syncChanged: "nekopak://sync/changed",
  mountStatus: "nekopak://mount/status",
  quotaChanged: "nekopak://quota/changed",
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];

export interface BackendStatusEvent {
  backend: BackendId;
  status: CapabilityReport["status"];
  checked_at: string;
}

export interface TransferProgressEvent {
  task_id: string;
  transferred_bytes: string;
  total_bytes?: string;
  speed_bytes_per_sec?: number;
  state: TaskState;
  /** PROPOSAL UI-001 §P3 (additive, optional): monotonic per-task revision. */
  revision?: string | number;
  /** PROPOSAL UI-001 §P3 (additive, optional). */
  updated_at?: string;
  error?: ClientError;
  simulated?: boolean;
}

export interface TransferChangedEvent {
  task_id: string;
  state: TaskState;
  error?: ClientError;
  /** PROPOSAL UI-001 §P3 (additive, optional). */
  revision?: string | number;
  /** PROPOSAL UI-001 §P3 (additive, optional). */
  updated_at?: string;
  transferred_bytes?: string;
  total_bytes?: string;
  speed_bytes_per_sec?: number;
  simulated?: boolean;
}

export interface SyncScanProgressEvent {
  scan_id: string;
  files_scanned: number;
  warnings: string[];
}

export interface SyncPlanReadyEvent {
  plan_id: string;
  summary: string;
  /** PROPOSAL UI-001 §P6 (additive, optional): lets UI ignore plans of other configs. */
  config_id?: string;
}

export interface SyncChangedEvent {
  job_id: string;
  state: string;
  error?: ClientError;
}

export interface MountStatusEvent {
  mount_id: string;
  status: string;
  message?: string;
}

export interface QuotaChangedEvent {
  quota: unknown; // QuotaReport — shape is PROPOSAL UI-001 §P1
  checked_at: string;
}

export interface EventPayloads {
  "nekopak://backend/status": BackendStatusEvent;
  "nekopak://transfer/progress": TransferProgressEvent;
  "nekopak://transfer/changed": TransferChangedEvent;
  "nekopak://sync/scan-progress": SyncScanProgressEvent;
  "nekopak://sync/plan-ready": SyncPlanReadyEvent;
  "nekopak://sync/changed": SyncChangedEvent;
  "nekopak://mount/status": MountStatusEvent;
  "nekopak://quota/changed": QuotaChangedEvent;
}
