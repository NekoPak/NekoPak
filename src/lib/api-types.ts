// NekoPak IPC contract 0.2. Manually maintained alongside nekopak-domain and docs/03_CONTRACTS.md.
export type DecimalU64 = string;
export type BackendId = 'official_cli' | 'rust_compat' | 'webdav' | 'rclone' | 'mock';
export type Operation = 'list' | 'stat' | 'search' | 'mkdir' | 'rename' | 'move' | 'copy' | 'trash' | 'upload' | 'download' | 'cloud_task' | 'stream_url' | 'read_range' | 'quota' | 'segment_download' | 'star' | 'restore' | 'delete_forever' | 'folder_download' | 'duplicates' | 'space_summary' | 'media_probe' | 'clip_download' | 'quality_download' | 'external_player' | 'share' | 'archive' | 'link_preview';
export type ErrorCode = 'AUTH_EXPIRED' | 'ACCOUNT_RESTRICTED' | 'PERMISSION_DENIED' | 'VERIFICATION_REQUIRED' | 'QUOTA_EXCEEDED' | 'NOT_FOUND' | 'UNSUPPORTED' | 'TRANSIENT_NETWORK' | 'DISK_FULL' | 'INTEGRITY_FAILED' | 'CONFLICT' | 'CANCELLED' | 'INTERNAL';
export interface ClientError { code: ErrorCode; message: string; retryable: boolean; backend?: BackendId; request_id?: string; details?: Record<string, unknown> }
export interface CapabilityReport {
  backend: BackendId;
  status: 'ready' | 'needs_login' | 'not_installed' | 'unavailable';
  operations: Partial<Record<Operation, boolean>>;
  account_tier?: 'free' | 'premium' | 'unknown';
  version?: string;
  limitations: string[];
  checked_at: string;
}
export interface RemoteEntry {
  id: string; parent_id?: string; name: string; kind: 'file' | 'folder'; size_bytes: DecimalU64;
  modified_at?: string; etag?: string; hash?: { algorithm: 'md5' | 'sha1' | 'sha256' | 'gcid'; value: string }; mime_type?: string;
  starred?: boolean;
}
export interface Page<T> { items: T[]; next_cursor?: string }
export type TaskState = 'queued' | 'running' | 'paused' | 'retrying' | 'verifying' | 'completed' | 'failed' | 'cancelled';
export type TransferAction = 'pause' | 'resume' | 'cancel' | 'retry';
export interface TransferTask {
  id: string; remote_id: string; kind: 'download' | 'upload' | 'cloud_download' | 'segment_download'; state: TaskState;
  transferred_bytes: DecimalU64; total_bytes?: DecimalU64; speed_bytes_per_sec?: number;
  backend: BackendId; error?: ClientError; created_at: string; updated_at: string;
  revision: DecimalU64; simulated: boolean;
  name?: string; destination?: string;
  quality_label?: string; clip?: {start_ms:number;end_ms:number}; progress_percent?: number;
}
export interface TransferEvent {
  task_id: string; state: TaskState; transferred_bytes: DecimalU64; total_bytes?: DecimalU64;
  speed_bytes_per_sec?: number; error?: ClientError; revision: DecimalU64; updated_at: string; simulated: boolean;
}
export interface TaskRef { task_id: string }
/** Reserved contract; segment commands are not registered in Tauri yet. */
export interface SegmentDownloadRequest {
  remote_id: string; destination: string; start_ms: DecimalU64; end_ms: DecimalU64;
  expected_duration_ms?: DecimalU64; representation_id?: string; idempotency_key: string;
}
export interface SegmentDownloadPlan {
  plan_id: string; remote_id: string; representation_id?: string; start_ms: DecimalU64; end_ms: DecimalU64;
  output_format: string; stream_copy: boolean; starts_at_previous_keyframe: boolean;
  aligned_start_ms?: DecimalU64; subtitle_warning: string; requires_random_read: boolean;
}
export interface StorageStatus { status: 'ready' | 'recovery_required'; schema_version: number; error?: ClientError }
export interface AppStatus { version: string; contract_version: string; mode: 'mock' | 'rust_compat_preview'; storage: StorageStatus; features: Record<string, boolean> }
export interface AuthState { backend: BackendId; authenticated: boolean; credential_owner: 'none' | 'os_vault'; account_tier: 'free' | 'premium' | 'unknown'; message: string; state: 'signed_out' | 'pending' | 'signed_in' | 'expired'; display_name?: string; account_id?: string }
export interface SignInChallenge { challenge_id: string; kind: 'captcha'; url: string; expires_at: string; requires_password: boolean }
export type SignInResult = { state: 'signed_in' } | { state: 'challenge'; challenge: SignInChallenge };
export interface QuotaReport {
  backend: BackendId; account_tier: 'free' | 'premium' | 'unknown'; source: 'mock' | 'account'; checked_at: string; limitations: string[];
  dimensions: { dimension: string; period: string; used_bytes?: DecimalU64; limit_bytes?: DecimalU64; remaining_bytes?: DecimalU64; resets_at?: string }[];
}
export interface SyncAction { id: string; op: 'upload' | 'download' | 'mkdir' | 'keep_both' | 'delete' | 'skip'; path: string; reason: string; destructive: boolean }
export interface SyncPlan {
  plan_id: string; base_revision: string; source: string; destination: string;
  mode: 'upload_only' | 'download_only' | 'bidirectional'; actions: SyncAction[];
  warnings: string[]; has_unresolved_conflicts: boolean; has_deletes: boolean;
}
export interface CommandMap {
  download_directory_pick: {args: Record<string,never>;result:string|null};
  transfer_open_directory: {args:{task_id:string};result:void};
  verification_status: { args: { verification_id?: string; request_id?: string }; result: {verification_id: string; state: 'pending' | 'verified' | 'expired'; expires_at: string} | null };
  verification_open: { args: {verification_id: string; request_id?: string}; result: void };
  verification_complete: { args: {verification_id: string; request_id?: string}; result: void };
  verification_cancel: { args: {verification_id: string; request_id?: string}; result: void };
  files_starred: { args: {cursor?: string; limit?: number; request_id?: string}; result: Page<RemoteEntry> };
  files_recent: { args: {cursor?: string; limit?: number; request_id?: string}; result: Page<RemoteEntry> };
  trash_list: { args: {cursor?: string; limit?: number; request_id?: string}; result: Page<RemoteEntry> };
  files_search: { args: {query: string; cursor?: string; limit?: number; request_id?: string}; result: Page<RemoteEntry> };
  files_mutate: {args: {op: import('./contract-proposals').MutateOp; ids: string[]; target?: import('./contract-proposals').MutateTarget; idempotency_key: string; request_id?: string}; result: import('./contract-proposals').OperationResult};
  shares_list: {args: {cursor?: string; limit?: number; request_id?: string}; result: Page<import('./contract-proposals').ShareInfo>};
  settings_get: {args: Record<string, never>; result: import('./contract-proposals').Settings};
  settings_update: {args: {patch: import('./contract-proposals').SettingsPatch}; result: import('./contract-proposals').Settings};
  app_status: { args: Record<string, never>; result: AppStatus };
  backend_discover: { args: { request_id?: string }; result: CapabilityReport[] };
  files_list: { args: { parent_id: string; cursor?: string; limit?: number; request_id?: string }; result: Page<RemoteEntry> };
  files_stat: { args: { id: string; request_id?: string }; result: RemoteEntry };
  files_path: { args: { id: string; request_id?: string }; result: RemoteEntry[] };
  transfers_list: { args: { cursor?: string; limit?: number }; result: Page<TransferTask> };
  transfer_start: { args: { remote_id: string; destination: string; idempotency_key?: string; request_id?: string }; result: TaskRef };
  transfer_control: { args: { task_id: string; action: TransferAction }; result: void };
  cloud_task_create: { args: { source_url: string; parent_id: string; idempotency_key: string; request_id?: string }; result: TaskRef };
  quota_get: { args: { request_id?: string }; result: QuotaReport };
  request_cancel: { args: { request_id: string }; result: void };
  auth_status: { args: { request_id?: string }; result: AuthState };
  auth_sign_in: { args: { username: string; password: string; request_id?: string }; result: SignInResult };
  auth_verify: { args: { challenge_id: string; code: string; password?: string; request_id?: string }; result: SignInResult };
  auth_logout: { args: { backend: BackendId; request_id?: string }; result: void };
}

