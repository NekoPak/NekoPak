# NekoPak Rust ↔ UI 接口契约 v0.2

2026-11-08：当前冻结变更见 §10–§12，后续小节覆盖此前实现状态。UI-001 的其他功能仍是
提案，不因导入 v3 界面而自动成为已实现能力。§7–§9 保留为历史记录。

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
| `backend_discover` | — | CapabilityReport[] | 当前主连接 rust_compat；显式离线开发模式只返回 mock |
| `auth_begin` | `{ backend }` | AuthFlow | 未来扩展保留；PikPak 原生登录使用 §10 |
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

## 8. 已撤销的 CLI 预览契约（历史）

2026-11-08 完全移除本节旧实现，包括 official_cli_probe、official_cli_preview 模式、
external_cli 凭据所有者与外部程序配置。旧观察见 [历史记录](history/CLI_ADAPTER.md)，
不是当前可调用接口。BackendId.official_cli 仅保留历史序列化兼容。
当前主连接、命令与会话语义见 §10。

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

## 10. 移除 CLI，原生 API 登录切片（2026-11-08）

- `contract_version = "0.2"`。移除 `official_cli_probe` 命令、CLI crate、配置读取和运行路径。
  `BackendId.official_cli` 只保留旧记录的反序列化兼容性，永远不注册或路由到它。
- 默认桌面主连接为 `rust_compat`，`app_status.mode = "rust_compat_preview"`。
  显式 `NEKOPAK_BACKEND=mock` 使用原有独立 Mock 数据库，保持离线演示语义。
  不将历史 CLI 队列转换成模拟任务，不自动删除任何数据库。
- 原有 AuthState 字段保留，新增 `state`（signed_out/pending/signed_in/expired）、
  `display_name?`（已脱敏）；`credential_owner` 增加 `os_vault`。密码与 token 不进入返回值。
- `auth_sign_in({username,password,request_id?}) -> SignInResult`；
  `auth_verify({challenge_id,code,password?,request_id?}) -> SignInResult`；
  `auth_logout({backend,request_id?}) -> void`。参数风格维持 flat / snake_case。
  邮箱+密码流程先行；短信/邮箱验证码重发未验证，不注册 `auth_resend`。
- SignInResult 使用 UI-001 §P23 的 signed_in/challenge 联合。
  CAPTCHA challenge 增加 `requires_password?: boolean`：原生流程为 true。
  密码在首次请求结束后即清除，人工验证后须重新输入，不能凭一个空验证码冒充已完成登录。
  challenge 有超时且绑定当前登录尝试，只保留账号/设备/挑战元数据，不保存密码。
- `nekopak://auth/changed {state,error?}` 只包含公开状态。登录/退出/失效后 UI 重读状态与
  能力；刷新请求串行化。过期或取消的请求不能覆盖另一登录会话。
- 默认安装内置已核对的公开 Web 登录协议参数，普通用户不需要环境变量或开发者资格。
  公开 client_id / SDK 版本属于协议元数据，不是用户凭据；不内置 client_secret、
  Android 身份或验证码自动解题逻辑。系统凭据库故障时仍明确报错，不回退明文。
- 登录按官网 Web SDK 请求：正文仅 client_id/username/password，CAPTCHA token 放在
  x-captcha-token 请求头；captcha init 使用 meta.email。人工验证使用带随机 state 的
  本机回环回调接收已验证 CAPTCHA token，token 留在 Rust，继续登录仍须重新输入密码。
- 当前切片范围：登录、人工验证码衔接、刷新、系统凭据库、退出、目录分页和元数据。
  HTTP 协议 fixture 验证不代表真实账号验收。写操作、下载、配额、Range、媒体、同步、
  挂载在相应引擎/真实验收完成前保持 false。`real_transfers` 始终为 false。
  本条记录最初登录测试包；2026-11-08 后续 §11 覆盖文件与下载的当前实现状态。
- v3 `transfer_start.options` / `cloud_task_create.preview_id,file_indices` 暂未实现，
  宿主必须返回 UNSUPPORTED，不能静默忽略选段、画质或文件选择后执行整文件任务。
- 字节与任务 revision 继续使用十进制 u64 字符串，根目录继续为 root。
  本节接受了 §P0、§P3、§P7 和 §P23 的上述子集；其余提案不变。

## 11. 网盘安全验证与基础文件功能修复（2026-11-08）

本次接入在 §10 之上增加以下契约，同步更新 Rust、前端和离线 fixture。
既有会话继续使用系统凭据库，不要求为了网盘安全验证退出账号或重新输入密码。

- 新增 `VERIFICATION_REQUIRED` 错误，`details.verification_id` 是本地随机引用。
  错误不含 CAPTCHA token、验证链接、账户 ID 或服务原始诊断。真正权限不足继续为
  PERMISSION_DENIED，不提示购买会员来解决安全验证。
- `verification_status({verification_id?}) -> {verification_id,state,expires_at}?`，
  state 为 pending / verified / expired。`verification_open({verification_id})` 由 Rust
  校验并在系统浏览器打开其持有的验证链接。`verification_complete({verification_id})`
  仅在收到有效人工回调时完成，`verification_cancel` 释放本地挑战。状态查询不发账户请求。
- 网盘请求独立获取相应 action 的 CAPTCHA，保存在 Rust 内存，账号/设备/会话切换即失效。
  服务拒绝 token 时只读请求允许限次重新初始化；副作用请求不盲目重放。人工验证完成后
  界面提供“继续读取”，写操作需要再次确认原意图。
- §P2 的 mkdir / rename / move / copy / trash / restore / star / unstar 与 §P16 的
  files_starred、trash_list、files_recent、files_search、shares_list 接入实际 Rust 路径。
  每项仅在相应实现与协议测试完成后声明能力；不存在以 HTTP200 代表异步写入已完成。
  删除与恢复语义分离；永久删除未接入时保持禁用。
- 文件变更幂等记录须在请求发送前持久化。相同账号、键和参数返回已记录结果，重启后
  未知结果保持未知并要求核对，绝不再次自动发送。其他账号不能复用原账户记录。
- 旧凭据可增加可选 user_id；若缺少，由 Rust 使用现有会话向固定 user/me 查询 sub，
  不从邮箱猜测、不读取其他客户端会话、不让用户提供客户端参数。

准确公共协议与研究证据见 DRIVE_PROTOCOL_RESEARCH.md、FILE_PROTOCOL_RESEARCH.md。
上述新增路径以逐项真实测试记录标注完成状态；未通过的功能不因此声明上线。

### 11.1 下载、设置及字段补充

- Native `transfer_start.destination` 为已经存在的本地下载目录，核心根据远端文件名生成
  安全落盘名；Mock 原有参数仍只作模拟，不写文件。`idempotency_key` 按账号/参数隔离。
  当前只支持原文件、单任务单连接有界块读取；不把这一能力宣称为挂载用随机读或媒体选段。
- `TransferTask` 增加可选 name / destination。原生传输仍用既有状态、decimal bytes/revision；
  成功必须完成长度与分块校验。启动恢复暂停，用户显式继续。失败、取消不自动删除 .part。
  CDN 请求不携带账号 Authorization/CAPTCHA，HTTPS/公共地址校验、重定向和 Range 有界。
- `real_transfers=true` 表示当前 provider 有实际落盘引擎；native 下载 capability 在已有会话
  时可用。已完成离线真实文件写入与 WebView2 本地HTTP端到端验证，真实服务仍待用户验收。
- 原生进度事件不带旧账号任务数据，仅发送 resync-required；UI 用按当前账号过滤的
  transfers_list 快照更新。后续真实速度采样见 §12；未测量值仍不得用模拟数值补齐。
- `settings_get/update` 接受实际实现的 theme、download_dir、bandwidth_limit_kib_s（0为不限），
  experimental_network.enabled/proxy_mode（后续扩展见 §12）。其他提案字段不能假保存。
  enabled=false恢复系统代理；不修改OS代理/DNS/Hosts。设置与下载元数据、远端intent存在
  `api-preview.sqlite3.actions.sqlite3`，主任务库标记其已初始化；遗失时拒绝创建空记录。
- QuotaReport.source 增加 account；QuotaDimension 增加 used_bytes，保留服务报告的真实
  使用量（可大于limit），remaining单独饱和减，未知不补零。新增storage/download_daily维度。
- RemoteEntry增加可选starred；ShareInfo增加expires_known，缺省日期不得被native标为永久。
  分享列表与分享创建/取消是不同能力；当前只接入列表。永久删除保持禁用。
- provider请求在宿主有45秒总期限（登录30秒）；后台下载块最多90秒并可取消。UI只读等待
  50秒，不对写请求自行超时后重放。操作发生后结果未知显示outcome_unknown且不重发。

## 12. 桌面下载体验、语言、托盘与网络设置（2026-11-08）

保持 `contract_version=0.2`；新增字段可选，旧设置读取时补默认值，不迁移或清空任务。
Rust/TS 接口、浏览器 Mock、契约与原生 E2E 同步维护。此节不启用媒体、分享写入或云下载。

- 原生 `speed_bytes_per_sec` 来自 Rust 单调时钟与约 3 秒滑动窗口的实际网络字节采样，
  不把断点已有字节计成新下载速度；停流后衰减为 0，暂停、失败、校验、完成时清除样本。
  速率为易失信息，同一个持久 `revision` 可以有新的速度快照；UI 接受同 revision 的快照，
  不接受更旧 revision。进度仍只计已验证、已提交的字节。UI 活动下载期间每 500ms 重读快照。
- `download_directory_pick() -> string | null`：打开所属窗口的系统目录选择器，返回已存在的
  本地绝对路径；取消返回 null，不更改设置、不创建任务。不能选择的路径返回结构化错误。
  选择之后由 `settings_update` 显式保存；下载前单次选目录不覆盖默认目录。
- `transfer_open_directory({task_id}) -> void`：Rust 根据当前账号拥有的持久下载任务解析父目录，
  校验后调用系统文件管理器；不接收 UI 任意路径。不随新的默认下载目录改变历史任务位置。
- 设置新增 `ask_download_directory=false`、`close_to_tray=false`、
  `language="system" | "zh-CN" | "en"`。语言保存于 Rust 设置库，WebView localStorage 仅为
  首屏缓存；切换不刷新页面，缓存不可用也不阻止启动。当前词典覆盖主要可用界面，五语种与
  全量提示覆盖仍未完成，服务返回文字/文件名/路径不自动翻译。
- 托盘提供显示、隐藏、打开下载任务和退出。关闭到托盘默认关闭；启用且托盘存在时关闭
  窗口只隐藏，后台下载继续。退出结束进程，重启按既有恢复规则暂停待续。
  `desktop_status() -> {tray_available:boolean,window_visible:boolean}` 返回本机宿主状态；
  `nekopak://desktop/navigate` 负载 `"transfers"` 要求 UI 导航至下载任务。
- `experimental_network` 默认关闭；`proxy_mode` 支持 system / none / user_provided，
  `proxy_url` 支持 HTTP、HTTPS、SOCKS5、SOCKS5H，拒绝账号密码、路径、查询和片段。
  地址属于非凭据配置，可回读；`user_proxy_configured` 由 Rust 推导，不接受 UI 伪造。
  `compatibility_mode=false`；启用时仅认证/网盘 API 客户端限定 TLS 1.2，CDN 保持其下载
  TLS 配置。始终验证证书/主机名；没有修改系统 Hosts、DNS、代理或地区检查的功能。
- Rust 在提供网络 IPC 和后台下载前加载网络配置，UI 在认证/能力读取前恢复设置。
  网络配置损坏时不带着默认网络继续认证，不覆盖原设置。切换仅影响后续请求，已在途请求
  继续使用旧客户端。关闭增强恢复系统代理和正常 TLS 协商；未实现的 DNS 选项仍拒绝。

验证与限制见 [DESKTOP_UX_FIX_REPORT.md](DESKTOP_UX_FIX_REPORT.md)。本地 HTTP/WebView2 测试
不证明真实账号、区域连通性或系统播放器等后续能力已经验收。

## 13. 完整客户端集成增量（2026-11-08，实施中）

按用户要求连续推进完整能力；本节冻结跨模块字段，功能完成以对应测试和能力报告为准。
新增命令只有处理器及测试完成才放入可调用集合；不为占位实现报告 true。

- `RemoteEntry.hash.algorithm` 加入 `gcid`，保留服务的内容身份用于同内容重复检查，
  不把 GCID 标为标准 SHA-1，也不作为普通下载的 SHA-1 校验承诺。
- `transfer_start_batch({remote_ids,destination,options?,idempotency_key}) -> BatchRef` 采用
  UI-001 §P15；递归保留目录结构，所有页读取与账号核对成功后才入队，深度/条数有界，
  循环、路径冲突、不可读项不伪装为完整下载；相同用户意图重试不能重复创建子任务。
- `duplicates_scan/report/cancel` 与 `files_space_summary` 采用 §P17/§P18，Rust 扫描真实
  树和完整分页；重复仅按同算法内容身份和精确长度，不自动删除。失败与不完整分别显示。
  扫描报告与索引按账号隔离；取消只停止扫描，不改远端文件。
- 语言枚举扩展为 system/zh-CN/zh-TW/en/ja-JP/ko-KR；历史三值继续有效。
- 远端写意图仍在发请求前保存，结果未知不重发；扩展操作使用带命令名的账号范围键。
  辅助记录库采用显式验证后事务迁移，已有损坏/缺表不自动修复为空记录。
- 媒体 RangeSource、缓存、队列由 Rust 持有来源身份和签名链接；时间参数明确为毫秒。
  `media_probe`/`media_align` 与 `transfer_start.options` 采用 §P14，指定表示失效不得换档；
  ClipRange 的数字必须是 JS 安全整数，在 Rust 解析时拒绝非整数、越界和倒置时间。

root 与媒体/协议/前端代理按照 docs/09_AGENT_HANDOFF.md 协调本节，具体新增本地源与
播放器返回值在实现前继续补充，不改变已有字节/任务 revision 十进制字符串规则。

- `experimental_network.block_access_domains=false` 是用户明确要求的独立开关，不受代理的
  enabled 值代管；Rust 在DNS/连接前拒绝四个精确 access 主机及其带尾点变体。匹配不扩展到
  子域或相似后缀，规则错误不产生伪造成功响应；关闭立即用于后续请求。配置重启持久化。
  不修改系统 Hosts，代理使用仍按用户网络需要。用户已澄清：其环境需要代理才能登录/使用，
  未加 Hosts 屏蔽仍可用；这不是无代理直连成功的证据。

### 13.1 已接入命令与恢复语义

- 云任务 `cloud_task_preview/create`、分享 `share_create/shares_cancel`、压缩包
  `archive_list/unlock/extract/jobs_list` 接入UI-001对应形状。云任务的`progress_percent`独立于
  未知字节长度；只有服务的完成phase可完成。云任务仅取消/重试，重试原task ID不重建source URL。
- `accounts_list()->SavedAccount[]`、`account_switch({id})->AuthState`、`account_remove({id})`。
  SavedAccount为id/display_name/active/state，AuthState增account_id；账号最多8个，所有
  会话及索引在系统凭据库。添加成功才激活，失败/验证中保留原账号；切换事件即使状态同为
  signed_in也发送，前端按account_id重置账号数据。公开id不含邮箱或远端sub。
- `ui_prefs_get()->{account_id,prefs}`、`ui_prefs_update({prefs,expected_account_id})->prefs`。
  pins/view/sort/density/gridSize/foldersFirst按账号存Rust，迟到旧账号写被拒绝，原生pins
  不写WebView共享localStorage。主题、语言等设备偏好继续走settings。
- `local_sources_list/add/remove`返回`{source_id,root_id,name,path}`；add.path缺省系统picker，
  取消null。files_list/stat/path接受Rust返回的`local:`句柄，仅访问已映射根内常规文件；
  不允许云修改。取消映射不删除本地内容并停止相关播放读取。
- `media_session_open({remote_id,quality_id?})->{session_id,url,mime_type,name}`仅返回随机
  127.0.0.1流地址，支持GET/HEAD/单Range，校验Host/Origin/账号/源身份，限时限并发。
  `media_session_close`撤销。CDN签名URL不出Rust；图片可转WebView blob后关闭流。
- `media_probe/align`探测真实MP4/MKV/TS索引；`transfer_start.options`指定质量/时间。
  `prepare_format(...,"mp4")`执行纯Rust转封装，所有产物校验后无覆盖发布。任务暂停重启时
  从保存的计划重新封装并复用已验证缓存，不能宣称复用半写mux状态。缓存按账号/表示/版本
  隔离，本地块SHA256、512MiB预算及旧块回收；转码需要强ETag并使用If-Match。
- `player_config_pick()`选择已安装播放器；settings.media.player_path保存路径。外播用
  独立参数启动所选程序，给它本机流URL；不调用shell拼字符串，不用浏览器冒充播放器。
- `share_open/folder/restore`使用本地access_ref，实际share token留Rust；
  `share_preview_entry({access_ref,file_id})`返回`shared:`只读句柄供探测/播放/原画下载。
  下载与媒体任务保留独立系统凭据库ref供重启恢复。分享撤销/过期明确失败，不创建自有副本
  冒充只读预览。跨子目录转存ancestor边界仍按研究/测试逐项核验。
- `upload_pick_and_start`、`upload_directory_pick_and_start`返回BatchRef，选择前捕获账号，
  hash/完整目录计划、分片etag、源身份和提交结果持久化，OSS临时授权在系统凭据库。
  上传完成确认GCID/长度/phase后才清理授权；不向OSS带API Bearer/CAPTCHA。
- sync_configs_list/config_save/scan/plan/apply/history采用原提案；apply返回聚合run ID，
  `sync_run_status({id})`查询。子任务全完成后才更新三方基线，目标更新前备份，默认无删除。
  SyncConfig新增auto_enabled=false、interval_seconds=300（60..86400）；自动调度遇到删除
  或冲突只生成待确认计划，失败持久退避。读失败/链接异常不当空目录。
- mount_config_save/list保存账号范围配置，mount_start/stop/status区分CloudFiles同步根与
  WinFsp只读盘符；start真实tail/head/middle读验证后调用OS，status仅来自native连接状态。
  CloudFiles停止不注销或删除占位文件，安全刷新只新增；WinFsp须用户安装。账号切换与挂载
  启停串行，并撤销pending读；已下载本地内容不会因退出自动抹除。
- reference_create/list/status/store_manifest/archive/restore以ReferenceManifest记录；先保存
  完整清单，云清单实际上传并按SHA256/GCID/长度读回核对后才允许明确confirm_trash的逐文件
  回收。restore优先原对象，allow_recreate必须单独授权；PENDING新节点ID留档，不自动重复
  或永久删除。目录不随归档删除，分享链接不当成可恢复备份。
- autostart_get/set独立宿主命令，Windows仅当前用户Run值，默认不写，用户显式开关时修改。

以上为产品集成契约，不代表真实用户账号、所有格式、所有平台或全部上游功能已验收。

## 14. Android 宿主增量（2026-11-09）

保留全部现有命令形状和 `contract_version`，只追加能力与导出命令。Android 使用同一个
Rust Core/native provider，凭据由 Android Keystore 的不可导出 AES-GCM 密钥封装，
密文保存在不参与系统备份的私有目录；凭据接口不暴露给 WebView。

- `app_status.features` 追加 `mobile`、`desktop_window`、`tray`、`directory_picker`、
  `transfer_open_directory`、`transfer_export`、`media_external`、`player_config`、
  `background_transfers`。Android 的 `mobile/transfer_export=true`，其余这些宿主能力为
  false。未适配的目录上传、本地目录映射、同步、挂载与外部播放器明确禁用。
- Android 下载写入应用私有 `Downloads`，不接受任意共享存储路径或 `content://` 假路径。
  `transfer_export({task_id})->boolean` 只导出当前账号已完成且长度一致的普通/片段下载；
  系统保存对话框返回的 SAF URI 只留宿主。取消返回 false，写入失败明确报错，原文件保留。
- Android 暂停活动时阻止启动新的本地任务，并将活动本地任务持久暂停；回到前台后由用户
  继续。云端任务不因应用切到后台而取消。尚未接入前台服务，不宣称锁屏持续下载。
- 前端按 Rust 能力判断 Android，不凭窗口宽度推断平台。小屏布局也用于浏览器测试，
  浏览器样例不能替代 Android APK/Keystore/设备验收。

## 15. 手机重做与连接恢复增量（2026-11-10）

- 手机使用独立文件／传输／我的导航、触屏文件行、上下文选择和底部操作单；桌面多标签与
  侧栏不参与手机组件树。`app_status.system_locale?:string` 由 Android 原生配置提供，
  跟随系统语言时优先采用它；用户显式选择语言继续优先。保持既有契约版本与字段含义。
- `transfer_forget({task_id})->void`、`transfer_clear_finished()->{removed:number}` 只移除当前
  账号的 completed/failed/cancelled 可见历史，进行中的任务拒绝。持久隐藏标记保存于 Rust
  journal；任务回执、幂等证据、下载字节和远端文件保留，重启后不再出现在列表。
- 可重试的下载网络故障进入 retrying，按有界退避恢复授权 Range／媒体读取；账号、来源身份、
  ETag和块校验继续有效。用户暂停／取消优先；上传与远端写的未知结果不盲目重放。
  速度来自 Rust 实际接收的字节计时，重试等待、暂停和已结束任务不报告假速度。
  连续无进度最多自动重试6次，基础退避为1/2/4/8/16/30秒；服务端Retry-After可延长等待，
  手动重新连接不会缩短该限制。CDN临时错误在成功响应ETag核对前分类；403/404/412不当断线重试。
- `auth_preferences_get()->{remember_account,restore_session,username?}`、
  `auth_preferences_update({preferences})->同形`。账号名与选项存 OS 加密凭据索引；密码
  保持 request-only。关闭保持登录后，下一次启动不自动激活会话；已保存账号可手动选择。
- `experimental_network.server_domain` 只接受 auto/mypikpak.com/mypikpak.net/pikpak.me/
  pikpakdrive.com。仅重写允许的认证与网盘API主机，不改写CDN签名地址。明确保存设置后
  用于后续请求；失败不触发远端写的跨域重放。
- `network_domains_probe()->{selected_domain,active_domain,checked_at,results}`；每项包含
  domain/usable/latency_ms?/warm_latency_ms?/error?。无凭据、有限时的固定端点探测；自动
  选择只在明确测速后采用可用结果，不能用测速宣称账号或配额可用。
- `network_reconnect()->void` 重新取得代理／HTTP连接设置并唤醒已排程的安全下载读取；
  不自动继续用户手动暂停的任务。网络和服务错误不一律标成密码错误。
- 原生视频选段预览使用可撤销本机媒体会话与系统WebView视频解码，跟随选段起终点seek；
  关闭、切画质和切账号会撤销会话。Mock示意帧与实际视频不混称，不保证系统支持所有编码。

## 16. 下载吞吐、种子导入与输入法（0.1.2）

- 普通下载由独立持续worker读取就绪块，云任务轮询与UI刷新不再限制下载块频率。Native块
  最大4MiB；每块仍同步落盘并记录SHA256，重启／源变化／账号切换规则保持。仅对实际返回
  强ETag的来源短期复用元数据，后续每次Range携带If-Match并核对ETag；弱／缺ETag继续刷新。
  CDN连接最多8个origin、30秒复用，DNS仍只允许公共地址并固定解析结果，网络重连代次改变失效。
- `cloud_torrent_pick()->TorrentPreview|null`仅由系统选择器读取用户指定的.torrent，最多10MiB。
  Rust校验bencode并对原始info字典计算SHA1，生成磁力资源；这与已核查Web客户端的种子导入行为
  一致，不把普通文件上传当作提交成功。v1／含v1信息的hybrid可解析，纯v2明确不支持。
- `TorrentPreview={import_id,title,info_hash,total_bytes,file_count,files:[{path,size_bytes}],private,expires_at}`。
  字节数为十进制u64，files最多200个预览条目；完整任务导入不按该预览长度截断。本地绝对路径和tracker
  授权不进IPC，Android content URI留在宿主，导入引用在Rust内存按账号隔离，5分钟过期。
- `cloud_torrent_create({import_id,parent_id,idempotency_key})->TaskRef`在用户确认后提交整个种子
  资源，通过既有云下载意图／回执防止重复创建。取消选择返回null；无效文件／过期／切账号
  均在发远端写请求前拒绝。`cloud_torrent_release({import_id})->void`清除未使用的临时引用。
- Android宿主发`nekopak:keyboard {visible:boolean}`本地显示事件；软键盘打开时隐藏底部主导航，
  收起恢复，返回键先关闭输入法。WebView保持对键盘避让，输入与提交控件可滚动到可见区域。

以上已接入并通过本轮离线、原生Android与发行构建验证，见SPEED_TORRENT_IME_REPORT.md。
这不代表真实账号、实际网络吞吐或功能对齐验收。

## 17. 发布更新与实时进度（0.1.3）

- `TransferTask.received_bytes?:decimal u64`为当前有效下载块已经接收的实时字节位置，仅在
  running普通下载的快照中追加；`transferred_bytes`仍为已校验并落盘的持久进度。UI使用前者
  展示进度与ETA，暂停／错误后回到实际落盘值，同revision快照允许更新；不提前显示任务完成。
- `update_status()->UpdateStatus`、`update_check()->UpdateStatus`、`update_download({update_id})->void`、
  `update_cancel()->void`、`update_install({update_id})->{state:"installer_opened"|"permission_required"}`。
  更新与PikPak账号无关。状态阶段idle/checking/up_to_date/available/downloading/ready/error，包含
  当前版本、目标版本、preview通道、签名内更新说明、decimal下载字节、可选结构化错误。
- Rust更新服务固定读取NekoPak/NikoPak公开release列表，选择较新且平台匹配的已签名清单；
  不接受UI任意URL／路径／公钥，不使用PikPak凭据。预览构建接受preview更新，stable构建只
  接受stable。没有可用release与请求失败分别显示，不把失败称为最新。
- `settings.updates={auto_check:false}`可选启动检查；下载、安装始终用户触发。应用内代理设置
  同样作用于更新网络；证书校验、HTTPS固定主机白名单、响应限长／超时保留。
- `update-manifest.json`使用独立协议的Ed25519签名封套（payload_base64/signature），payload
  包含schema/product/version/channel/published_at/notes及平台name/bytes/sha256。内置公钥
  与仓库发布密钥对应；这不是Tauri updater协议或Windows Authenticode。签名、平台、版本、
  文件长度与SHA256通过才成为ready，安装前再次核验。取消及失败不留下可安装半包。
- Windows启动已验证NSIS安装器前暂停本地传输，禁用其间新任务，成功启动后退出本程序；
  Android通过只读FileProvider将已验证APK交给系统，核验包名／升级版本／原签名证书。
  未开放本应用安装权限时只打开系统授权页，返回后再次明确点安装；不静默安装或自动授权。
- 检查、下载与安装核验串行互斥；检查时撤销旧候选与可安装状态，清理私有updates目录内
  本更新器的UUID包文件。不会递归清理目录／跟随链接／删除其他文件。安装动作错误不被
  后续状态轮询抹除。重启不会信任上次残留包，下次检查清理后重新下载和验证。

公开release仍标预览，不能因此通过上游功能对齐门禁。测试与发布记录见UPDATES_PROGRESS_REPORT.md。
