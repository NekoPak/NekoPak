# 契约提案 UI-001（Opus 5.5 → Astra）

状态：**提案，未冻结**　日期：2026-11-07　对应前端：`src/lib/contract-proposals.ts`

`docs/03_CONTRACTS.md` 是双方基线。本提案**只做补全与新增**：没有重命名、删除或改变任何已有字段的含义。前端已经按本提案实现，并在 Mock 中跑通；Astra 确认或修改后，前端只需改 `contract-proposals.ts` 一个文件，然后把冻结的部分移入 `contract.ts`。

后端尚未提供的提案命令，前端会收到 Tauri 的 `command xxx not found`，统一映射为 `UNSUPPORTED`（`details.proposal = "UI-001"`），相应界面显示“不支持”，不会崩溃或伪装成功。

---

## §P0 根目录标识

`files_list.parent_id` 需要一个根目录值。前端暂用 `"root"`（常量 `ROOT_ID`）。如 Rust 侧使用空字符串或其他值，告知后只改这一个常量。

## §P1 §3 中已命名、但未定义的返回类型

完整 TS 定义见 `src/lib/contract-proposals.ts`。要点：

| 类型 | 字段要点 |
|---|---|
| `AppStatus` | `version`, `contract_version`, `runtime: "mock" \| "tauri"`, `storage.status: "ok" \| "degraded" \| "corrupt_readonly"`, `feature_flags` |
| `AuthFlow` | `flow_id`, `backend`, `kind: "external_browser" \| "external_cli"`, `instructions`（不含令牌）, `expires_at?` |
| `AuthState` | `state: "signed_out" \| "pending" \| "signed_in" \| "expired"`, `account?.display_name`（**由 Rust 脱敏**）, `account?.tier`, `checked_at` |
| `TaskRef` | `{ task_id }`；`sync_apply` 也返回它，`task_id` 即 `sync/changed` 的 `job_id` |
| `QuotaReport` | `backend`, `account_tier`, `items: QuotaItem[]`, `checked_at` |
| `QuotaItem` | `dimension: downstream_daily \| downstream_monthly \| upload_monthly \| cloud_download_monthly`；`scope`；`unit: bytes \| count`；`limit? used? remaining?`（十进制字符串）；**`exactness: exact \| estimated \| unavailable`**；`state: ok \| near_limit \| exhausted \| unknown`；`resets_at?`；`note?` |
| `ScanRef` | `{ scan_id }`，与 `sync/scan-progress.scan_id` 对应 |
| `SyncEvent` | `id, config_id, at, kind: scan \| plan \| apply \| error, summary, error?` |
| `MountCapabilities` | `platform`, `modes: MountModeReport[]`, `checked_at`；每个 mode 含 `available` 与逐项 `checks[{ id, label, status: pass \| fail \| unknown, detail, requirement? }]` |
| `MountState` | `id, mode, status, read_only, mount_point?, error?` |
| `Settings` | `schema_version: 1`, `theme`, `download_dir`, `max_concurrent_transfers`, `bandwidth_limit_kib_s`（0=不限）, `cache{dir,max_bytes}`, `experimental_network{enabled,proxy_mode,user_proxy_configured,custom_dns_mode,connection_diagnostics}`, `telemetry: false` |

约束（前端依赖这些语义）：

- `QuotaItem.exactness = "unavailable"` 时不得给出 `remaining`；前端显示“不可获取”，不会自行推算。
- `Settings.experimental_network.user_proxy_configured` 只表示“是否已存储”，**永不返回代理地址明文**。
- `settings_update` 参数采用 `{ patch: SettingsPatch }`（部分更新）；`custom_dns_mode` 在审计前恒为 `false`，传 `true` 应返回 `UNSUPPORTED`。

## §P2 `files_mutate` 的 `op` 与 `target`

```ts
type MutateOp = "mkdir" | "rename" | "move" | "copy" | "trash" | "restore" | "delete_forever";
interface MutateTarget { parent_id?: string; name?: string }
interface OperationResult {
  op: MutateOp;
  succeeded: string[];
  failed: { id: string; error: ClientError }[];
  created?: RemoteEntry[];
  replayed: boolean; // 同一 idempotency_key 已执行过，本次未重复执行
}
```

- `mkdir`：`ids = []`，`target = { parent_id, name }`；`rename`：`target.name`；`move/copy`：`target.parent_id`。
- **批量部分失败**以 `failed[]` 返回，而不是整体抛错，便于界面逐项说明。
- 前端对每次用户意图生成一个 `idempotency_key`；从错误提示点“重试”时复用同一个 key。
- `trash`（可还原）与 `delete_forever`（不可撤销）严格区分；后者前端要求勾选确认。

## §P3 传输任务的展示与排序字段（新增、可选）

`TransferTask` 只有 `remote_id`，界面无法显示文件名；进度事件没有排序字段，无法可靠丢弃乱序事件。提议新增：

```ts
interface TransferTaskExt extends TransferTask {
  name?: string;        // 显示名；云下载为脱敏后的来源标签
  destination?: string; // 下载为最终本地路径；上传/云下载为云端目录
  revision?: number;    // 每个任务单调递增
}
// nekopak://transfer/progress 与 nekopak://transfer/changed 均新增：
revision?: number; updated_at?: string;
```

前端丢弃规则（`src/state/reconcile.ts`，已有单测）：`revision` 优先 → 其次 `updated_at` → 都没有时退化为“同状态下字节数不回退、终态不被进度事件复活”。启动和重连时**先** `transfers_list` 快照，监听器在请求快照前挂上并缓冲，快照到达后回放缓冲事件。

## §P4 授权完成通知

§4 没有授权相关事件，外部浏览器授权结束后界面无从得知。提议：

- 事件 `nekopak://auth/changed { state, flow_id?, error? }`（`error.code = "CANCELLED"` 表示用户取消）。
- 命令 `auth_cancel { flow_id } -> void`。

## §P5 新增命令

| 命令 | 参数 | 返回 | 用途 |
|---|---|---|---|
| `files_search` | `{ query, parent_id?, cursor?, limit? }` | `Page<RemoteEntry>` | 顶栏搜索（仅当能力 `search` 为真） |
| `files_recent` | `{ cursor?, limit? }` | `Page<RemoteEntry>` | “最近”页 |
| `files_path` | `{ id }` | `RemoteEntry[]`（根→自身，不含根） | 面包屑、深链接、搜索结果“位置”列 |
| `trash_list` | `{ cursor?, limit? }` | `Page<RemoteEntry>` | 回收站页；还原/永久删除走 `files_mutate` |
| `sync_configs_list` | — | `SyncConfig[]` | 同步配置列表 |
| `sync_config_save` | `{ config: SyncConfig }` | `SyncConfig` | 新建/修改；`config_id` 为空即新建 |

`SyncConfig`：`config_id, name, mode, local_path, remote_id, remote_path, exclude_globs[], allow_deletes, first_run, last_applied_at?`。后端应拒绝：`mode = bidirectional`（M3 前，`UNSUPPORTED`）、`first_run` 时 `allow_deletes = true`（`CONFLICT`）。

## §P6 `sync/plan-ready` 增加 `config_id?`

让多个同步配置同时扫描时，界面能只处理自己的计划。

## §P7 IPC 参数风格（需要 Astra 选定）

Tauri v2 默认把 Rust 参数名转为 camelCase。为保持契约要求的 snake_case，二选一：

- **flat（前端默认）**：`invoke("files_list", { parent_id, cursor })`，Rust 侧 `#[tauri::command(rename_all = "snake_case")]`。
- **wrapped**：`invoke("files_list", { args: { parent_id, cursor } })`，Rust 侧单个 `args: FilesListArgs` 结构体。

前端用 `VITE_NEKOPAK_IPC_ARGS=flat|wrapped` 切换，业务代码不变。

## §P8 `transfer_start.destination` 的语义

前端传**目录**（设置中的下载目录），由 Rust 生成最终文件名并做安全转义；界面只做提示，不自行拼接系统路径。Mock 的行为可作为参考：非法字符 `<>:"\|?*` 与控制字符 → `_`；末尾空格/句点 → `_`；保留设备名加前缀 `_`（`CON` → `_CON`）；按 NFC 规范化；与现有任务目标**不区分大小写**相同则返回 `CONFLICT`。

## §P9 需要的平台能力（插件或命令）

以下功能在界面中已存在，但目前**禁用并写明原因**，等待 Rust 侧提供：

| 能力 | 建议 | 界面位置 |
|---|---|---|
| 打开外部链接 | `@tauri-apps/plugin-opener` 的 `open_url`（前端已按 `plugin:opener\|open_url` 调用，失败时改为复制链接） | 官方说明、致谢链接 |
| 在资源管理器中显示 | `reveal_item_in_dir` | 已完成下载“打开所在文件夹” |
| 选择本地文件夹 | `@tauri-apps/plugin-dialog` | 同步本地路径、下载目录 |
| 安全存储机密 | `secret_set { kind, value }` / `secret_status { kind } -> { configured }`，值只写不读 | WebDAV 凭据、自定义代理 |
| 清理缓存 | `cache_clear -> { freed_bytes }` | 设置 › 传输与缓存 |
| 连通性检查 | `diagnostics_run -> DiagnosticsReport` | 设置 › 高级网络诊断 |
| 导出脱敏诊断 | `diagnostics_export -> { path }` | 设置 › 开发者 |

## §P10 输入校验错误码

`ClientError.code` 没有“输入不合法”。Mock 目前用 `UNSUPPORTED` + `details.reason = "invalid_source"` 表示无法识别的云下载链接。提议新增 `INVALID_INPUT`；这会扩充枚举，需要两边同时更新（前端遇到未知 code 会按 `INTERNAL` 显示）。

## §P11 Mock 夹具对齐

前端内置 Mock（`src/lib/mock/`）实现了 §6 的七组场景与确定性故障注入。建议 Rust `MockProvider` 使用**相同的场景 ID 与关键样本 ID**，这样 `tests/e2e/run.mjs` 可以原样对 Rust Mock 再跑一遍：

- 场景：`free`, `premium`, `unauthorized`, `quota_exhausted`, `network_down`, `huge_dir`, `name_conflict`
- 故障：`drop_at_37`, `out_of_order`, `integrity_fail`, `auth_expired`, `latency_ms`，外加“模拟重启”（运行中任务持久化为排队，不发事件）
- 特殊目录：`d-shared-ro`（写入→`PERMISSION_DENIED`）、`d-vanished`（列目录→`NOT_FOUND`）、`d-huge`（12,000 项）
- 文件名样本：汉字、emoji/ZWJ、前导/末尾空格、末尾句点、`CON`/`NUL.txt`/`aux.log`、NFC 与 NFD 的 `café`、`Report.pdf`/`report.pdf`、阿拉伯文、超长名、`9007199254740993` 字节

## §P12 `tauri.conf.json` 建议（Astra 所有）

```json
{
  "build": { "devUrl": "http://localhost:5173", "frontendDist": "../dist", "beforeDevCommand": "npm run dev", "beforeBuildCommand": "npm run build" },
  "app": {
    "windows": [{ "title": "NekoPak", "width": 1280, "height": 760, "minWidth": 720, "minHeight": 480 }],
    "security": { "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost" }
  }
}
```

`style-src 'unsafe-inline'` 是因为虚拟列表用内联 `style` 设置行高与位移；如需收紧，可改为 CSS 变量 + nonce，届时前端配合。

**v2 自绘标题栏**：界面把窗口按钮画进了标题行（与 Piko 同类做法），需要 `"decorations": false`，并在 capability 中放行 `core:window:allow-minimize`、`core:window:allow-toggle-maximize`、`core:window:allow-close`、`core:window:allow-is-maximized`、`core:window:allow-start-dragging`（`data-tauri-drag-region` 拖动）。Windows 11 的“贴靠布局”悬停浮层需要原生命中测试，纯 WebView 按钮无法触发；如需要，请评估 `tauri-plugin-decorum` 一类方案或保留系统标题栏（前端两种都可适配）。

---

**需要 Astra 回复：** §P0 根目录值、§P7 参数风格、§P3 是否接受 `revision`、§P5 命令名是否沿用，其余可按本提案执行或逐条修改。
