# NekoPak Rust ↔ UI 接口契约 v0.1

此文件为 Astra 与 Opus 5.5 的**共享基线**。任何字段变更需提交双向兼容说明、更新 mock fixtures、更新 UI 类型定义与测试。示例命令只表示设计，不暗示已经实现。

## 1. 约定

- 使用 Tauri v2 `invoke` 请求/响应、`emit` 后台事件。所有跨边界 JSON 字段为 `snake_case`，稳定 `id` 不当作展示名称。
- 所有时间为 UTC RFC3339，文件大小/偏移为 u64。JS 端若超过 `Number.MAX_SAFE_INTEGER` 必须以十进制字符串传输，不得精度损失。
- 公共错误包含 `code`, `message`, `retryable`, `backend`, `request_id`, `details?`。不能只返回任意 Rust Debug 字符串。
- 全部异步命令都允许取消/超时，且需要幂等或明确的 `idempotency_key` 语义，避免重试导致创建重复云下载任务。
- 密码/token 不允许作为 Tauri 事件与 debug dump 输出；错误可展示但禁止内含任何 URL 查询 token。

## 2. 核心类型（概念性 TS 声明）

```ts
export type BackendId = "official_cli" | "rust_compat" | "webdav" | "rclone" | "mock";
export type Operation =
  | "list" | "stat" | "search" | "mkdir" | "rename" | "move"
  | "copy" | "trash" | "upload" | "download" | "cloud_task"
  | "stream_url" | "read_range" | "quota";

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

export interface Page<T> { items: T[]; next_cursor?: string; }

export type TaskState =
  | "queued" | "running" | "paused" | "retrying" | "verifying"
  | "completed" | "failed" | "cancelled";

export interface TransferTask {
  id: string;
  remote_id: string;
  kind: "download" | "upload" | "cloud_download";
  state: TaskState;
  transferred_bytes: string;
  total_bytes?: string;
  speed_bytes_per_sec?: number;
  backend: BackendId;
  error?: ClientError;
  created_at: string;
  updated_at: string;
}

export interface ClientError {
  code: "AUTH_EXPIRED" | "ACCOUNT_RESTRICTED" | "PERMISSION_DENIED"
      | "QUOTA_EXCEEDED" | "NOT_FOUND" | "UNSUPPORTED"
      | "TRANSIENT_NETWORK" | "DISK_FULL" | "INTEGRITY_FAILED"
      | "CONFLICT" | "CANCELLED" | "INTERNAL";
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
```

## 3. 建议 Tauri 命令（命令参数需实际冻结）

| 命令 | 参数 | 返回 | 主要行为 |
|---|---|---|---|
| `app_status` | — | AppStatus | 版本、存储状态、feature flags |
| `backend_discover` | — | CapabilityReport[] | 检测官方 CLI、本地兼容层等 |
| `auth_begin` | `{ backend }` | AuthFlow | 启动外部授权/显示官方链接，绝不让 UI 收集密码 |
| `auth_status` | — | AuthState | 授权状态与账号信息 |
| `auth_logout` | `{ backend }` | void | 清除本地授权引用并调用对应注销 |
| `files_list` | `{ parent_id, cursor?, limit? }` | Page<RemoteEntry> | 可分页、可取消 |
| `files_stat` | `{ id }` | RemoteEntry | 读取元数据 |
| `files_mutate` | `{ op, ids, target?, idempotency_key }` | OperationResult | 明确危险确认 |
| `cloud_task_create` | `{ source_url, parent_id, idempotency_key }` | TaskRef | 创建磁力/URL 云下载 |
| `transfers_list` | `{ cursor?, limit? }` | Page<TransferTask> | 历史任务持久化 |
| `transfer_start` | `{ remote_id, destination }` | TaskRef | 返回后台任务 ID |
| `transfer_control` | `{ task_id, action }` | void | 暂停、恢复、取消、重试 |
| `quota_get` | — | QuotaReport | 分维度展示额度与生效时间 |
| `sync_scan` | `{ config_id }` | ScanRef | 只读生成快照 |
| `sync_plan` | `{ config_id }` | SyncPlan | 不触发写入 |
| `sync_apply` | `{ plan_id, confirm_deletes }` | TaskRef | 需重新验证 revision 与危险操作确认 |
| `sync_history` | `{ config_id, cursor? }` | Page<SyncEvent> | 可审计日志 |
| `mount_capabilities` | — | MountCapabilities | 检查 OS/依赖/后端随机读取能力 |
| `mount_start` | `{ config_id, read_only }` | MountState | 仅在全部检查通过后开启 |
| `mount_stop` | `{ id }` | void | 安全卸载 |
| `settings_get` / `settings_update` | 相应配置 | Settings | 字段 schema version，敏感字段不返回明文 |

## 4. 异步事件

```text
nekopak://backend/status          { backend, status, checked_at }
nekopak://transfer/progress       { task_id, transferred_bytes, total_bytes?, speed_bytes_per_sec?, state }
nekopak://transfer/changed        { task_id, state, error? }
nekopak://sync/scan-progress      { scan_id, files_scanned, warnings }
nekopak://sync/plan-ready         { plan_id, summary }
nekopak://sync/changed            { job_id, state, error? }
nekopak://mount/status            { mount_id, status, message? }
nekopak://quota/changed           { quota, checked_at }
```

前端必须在启动/重连时先用 `transfers_list` 读取可靠快照，再订阅增量事件；不能把事件总线当持久数据库。重复/乱序事件以单调 `revision` 或 `updated_at` 丢弃旧版本。

## 5. BackendPort（概念 Rust 伪代码）

```rust,ignore
#[async_trait::async_trait]
pub trait BackendPort: Send + Sync {
    async fn discover(&self) -> Result<CapabilityReport, ClientError>;
    async fn list(&self, parent: &str, cursor: Option<&str>) -> Result<Page<RemoteEntry>, ClientError>;
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError>;
    async fn get_download_source(&self, id: &str) -> Result<DownloadSource, ClientError>;
    async fn read_range(&self, id: &str, start: u64, end_inclusive: u64) -> Result<Vec<u8>, ClientError>;
    async fn create_cloud_task(&self, request: CloudTaskRequest) -> Result<TaskRef, ClientError>;
    // 其他能力单独 trait / feature，不要求所有后端都实现。
}
```

上述接口为讨论基线，不是可以直接编译的代码。正式实现优先通过小型能力 traits 避免所有 adapter 被迫实现无关方法。

## 6. Mock 测试样本与契约冻结

- 固定 7 组 demo：免费账户、Premium、未授权、限额耗尽、网络中断、目录超大、文件名称冲突。
- 文件命名含：汉字、emoji、空格、超长路径、Windows 保留名、UTF-8 组合字符；任何系统路径操作必须安全转义。
- `mock` 必须支持确定性故障注入：下载到 37% 时断网、进度事件乱序、退出时事务未提交、完整性检查失败、配额耗尽。
- P0 开始后仅增加字段不随意改动既有字段含义；破坏性修改需要 `contract_version` 和同步 PR。
