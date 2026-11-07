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

## 7. M0 后端冻结补充（2026-11-07）

本节冻结首轮实际实现，原有字段和命令名保持不变。前端以 `src/lib/api-types.ts`
为完整声明，`tests/fixtures/*.json` 由 Rust 导出。`contract_version = "0.1"`。

- 当前仅 `mock` 被注册；其他后端不返回虚假的安装/授权状态。
- 已实现：app_status、backend_discover、files_list、files_stat、transfers_list、
  transfer_start、transfer_control、cloud_task_create、quota_get、request_cancel。
  未列出的设计命令尚未注册，UI 必须禁用。
- 参数一律 snake_case。根目录 ID 是 `root`；分页 limit 默认 100、范围 1–500；
  cursor 是不透明字符串。失效/无效 cursor 返回 CONFLICT，必须重新读第一页。
- TaskRef 是 `{ task_id: string }`。transfer_control.action 是
  `pause | resume | cancel | retry`。终态 completed 不允许重开；cancelled 不允许重试。
- TransferTask 和 progress/changed 事件新增 `revision: string`（十进制 u64，
  SQLite 全局递增）及 `simulated: boolean`。事件同时携带 updated_at 和完整进度。
  客户端通过 BigInt 比较 revision，丢弃重复/乱序旧事件。
- AppStatus 为 `{ version, contract_version, mode: "mock", storage: { status:
  "ready" | "recovery_required", schema_version, error? }, features: Record<string, boolean> }`。
- QuotaReport 为 `{ backend, account_tier, source: "mock", checked_at, limitations,
  dimensions: [{ dimension, period, limit_bytes?, remaining_bytes?, resets_at? }] }`。
  M0 额度明确为 synthetic_month 样本，不代表实际服务限额；缺失额度字段不得补成零。
- 异步 provider 命令可追加 `request_id?: string`，调用 `request_cancel({request_id})`
  取消该请求；同一时刻 ID 不得重复。每个 provider 调用 15 秒超时。取消不撤销已提交事务。
- transfer_start 可追加 `idempotency_key?: string`；缺省表示创建新模拟任务。
  cloud_task_create 必须传 key。同一个操作内同 key+同参数返回原任务，即使重启；
  同 key 不同参数返回 CONFLICT。key 限 1–128 ASCII 字母数字及 `-_.:`。
- Mock 只模拟下载，不创建 destination 文件；路径输入仍拒绝穿越、设备名、UNC、
  特殊前缀和 ADS。真实落盘前必须增加句柄级 symlink/reparse 校验。
- 已实现事件：transfer/progress、transfer/changed（均为 TransferEvent）；
  新增 `nekopak://transfer/resync-required`（null）及 `nekopak://storage/error`（ClientError）。
  其他设计事件暂未发布，因为 M0 不含相应动态服务。
- 可靠启动序列：读取全分页任务快照 → 订阅 → 再次读取快照补齐窗口期 → 按 revision 合并。
  总线溢出或未知 task_id 要重新读快照，不能把事件当作数据库。

本补充为后端交接基线，尚无另一位前端开发者的实际集成确认。

## 8. 官方 CLI 只读预览补充（2026-11-07）

兼容增加以下字段/命令；contract_version 仍为 0.1。第 7 节的 M0 状态是历史基线。

- app_status.mode 增加 `official_cli_preview`；features 增加 `real_transfers: false`。
  real_providers 表示选择了真实适配器，不表示账号已授权或成功请求已实测。
- 宿主 backend_discover 注册 mock 和 official_cli；未显式配置的 CLI 返回
  `not_installed`，其含义是未配置可用安装，不声称已全盘搜索系统。
- 新增 `official_cli_probe({ request_id? }) -> CliProbeReport`：包含 backend、status、
  version?、binary_sha256?、verified_commands、live_account_verified、limitations、checked_at。
  probe 只检查本地版本/帮助，不触发登录；成功探测也不意味着 ready。
- `auth_status({ request_id? }) -> AuthState`：backend、authenticated、credential_owner
  (`none | external_cli`)、account_tier、message。无 token/password/email/user_id 字段。
- CLI 只读预览需显式配置 NEKOPAK_BACKEND=official_cli、NEKOPAK_CLI_PATH 和
  NEKOPAK_CLI_PROFILE。仅允许已核验 Windows x64 v0.6.0 的 SHA-256。
- files_list/stat 的 JSON 类型不变；CLI 分页 limit 限 10–500，低于 10 返回 CONFLICT，
  不静默超出请求上限。raw parent_id 空串映射 root；字节数字串严格校验；时间转 UTC。
- 仅当明确启用预览且外部 CLI 的 auth status 返回 logged_in=true 才启用 list/stat。
  capabilities.limitations 始终注明尚未完成真实授权目录验证。
- download/cloud_task/upload/quota/read_range 保持 false；授权配额未知，不从账户容量推导。
- auth_begin/auth_logout 尚未注册。用户在外部 CLI 完成官方授权；NekoPak 不收集凭据，
  不承诺外部 CLI 的凭据文件使用系统安全存储。

## 9. 视频片段预留契约与状态（2026-11-07）

Operation、TaskKind 加入 `segment_download`，现有 provider 显式报告 false。
app_status.features.segment_download=false。新枚举仅预留，当前不创建该任务，不改既有
字段/命令；前端新增分支应显示未实现。旧客户端应容忍未知未来 kind。

领域/TS 类型新增 SegmentDownloadRequest：remote_id、destination、start_ms、end_ms、
expected_duration_ms?、representation_id?、idempotency_key。时间单位毫秒，按十进制
u64 字符串传输；representation_id 缺省表示原画，不准无提示换转码画质。

SegmentDownloadPlan：plan_id、remote_id、representation_id?、start_ms、end_ms、
output_format、stream_copy、starts_at_previous_keyframe、aligned_start_ms?、
subtitle_warning、requires_random_read。aligned_start_ms 未实测时省略，不填请求起点冒充。
计划不能包含签名 URL/令牌。所有本地/远端来源身份与版本由后端持有。

规划中的 `media_probe`、`segment_plan`、`segment_start` **尚未注册 Tauri 命令**，
不添加可点击假入口。已实现 `nekopak-media` 本地实验库和 `clip_local` 示例，独立于 IPC。
远端命令须在随机读取、缓存、持久任务、取消恢复、画质/轨道和真实账号验证完成后接入。
此阶段不能宣称已实现云端片段下载；TS 明确待补齐，详见 MEDIA_SEGMENTS.md。
