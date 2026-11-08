# 后端适配说明：去掉官方 CLI，主攻 PikPak 接口

> 写给 Astra（Codex）。日期 2026-11-08。来源：项目所有者 Kiana 的决定 + Opus 5.5（前端）整理的接口对照。
> 附件来源的接口研究与目标说明。已采纳“完全移除 CLI、主连接 rust_compat、原生登录”。
> 当前实际实现与验证以 `API_ADAPTER.md`、`03_CONTRACTS.md` §10 为准；本文件中的“已核对”仅表示原作者记录的源码行为，不是本仓库真实服务测试。

## 1. 决定（不要再讨论，直接按这个做）

1. **去掉官方 CLI。** 不写 `nekopak-provider-cli`，不做 CLI 探测、安装引导或子进程适配。CLI 主要是给 AI 工具用的，做厚没有意义。契约里 `BackendId` 的 `"official_cli"` 保留（契约冻结，不改名），但不实现、不在 `backend_discover` 里返回。
2. **主连接 = PikPak 客户端接口**（社区抓包整理，GitHub 上已有多套实现，见 §2）。在 Rust 核心里实现为 `rust_compat` provider（沿用现有 `BackendId`，不新增）。界面不显示连接名，也不让用户选后端。
3. **原生登录。** 账号和密码由 NekoPak 自己的登录表单提交（`auth_sign_in`，见 `docs/CONTRACT_PROPOSAL_UI_001.md` §P23）。核心用它换 access/refresh token，token 存 Windows 凭据管理器（keyring），**密码不落盘、不进日志、不回传前端**。不做内嵌网页登录窗口。
4. WebDAV、Rclone 只服务挂载与同步，不参与普通文件浏览和下载。
5. 界面里不放“非官方接口”“Connected Apps 规则”之类的说明文字；这些只留在开发文档与 `THIRD_PARTY_NOTICES.md`。

## 2. 参考实现（2026-11-08 克隆核对）

| 仓库 | 提交 | 许可证 | 语言 | 用法 |
|---|---|---|---|---|
| [Bengerthelorf/pikpaktui](https://github.com/Bengerthelorf/pikpaktui) | `68b7a6b`（2026-10-22） | Apache-2.0 | Rust | **首选底子。** `src/pikpak/` 覆盖登录（captcha init → signin → refresh）、列表/批量操作/星标/回收站、并行分段下载与 `.part` 续传、离线任务、链接解析、分享、最近事件、配额。可抽取改写（blocking reqwest → async），保留 Apache NOTICE 并记录改动。 |
| [rclone/rclone](https://github.com/rclone/rclone) `backend/pikpak` | `82cb84b`（2026-11-06） | MIT | Go | 字段定义最全（`api/types.go`：`medias`、`web_content_link`、`thumbnail_link`、`starred`、`hash`=gcid）；云端解压、`resource/cid`、分片上传、重试与限速（`shouldRetry`、pacer）。 |
| [52funny/pikpakcli](https://github.com/52funny/pikpakcli) | `560f263`（2026-08-03） | MIT | Go | 行为对照：分享转存、配额。 |
| [Quan666/PikPakAPI](https://github.com/Quan666/PikPakAPI) | `c233bd3`（2025-02-11） | **GPL-3.0-only** | Python | 只读参考，**不要复制代码**。 |
| Piko / OpenList PikPak 驱动 | — | AGPL | Kotlin / Go | 只读参考，**不要复制代码**。Piko 的 HLS 片段裁剪（`shared/media/PreparedClip.kt`）可以看思路。 |

**当前采用的客户端标识**：默认内置官网公开 Web client ID 及协议版本，普通用户无需配置环境变量。pikpaktui 的 Android client_secret、captcha salts 和 UA 没有复用；NekoPak 使用自身 UA 和本地随机设备标识。登录流程已按官网 Web SDK 重新核对，详见 `NATIVE_LOGIN_RESEARCH.md` 与契约 §10。

## 3. 主机

- 认证：`https://user.mypikpak.com`
- 网盘、会员、解压：`https://api-drive.mypikpak.com`

## 4. IPC 命令 → PikPak 接口

“已核对”表示至少一个参考实现里有可运行的调用；“待验证”表示参考实现里没找到，需要实测或抓包确认后再实现，实现前 `backend_discover` 不要报告对应能力为 `true`（前端会自动把按钮置灰）。

### 4.1 登录与账号

| IPC | 接口 | 状态 | 说明 |
|---|---|---|---|
| `auth_sign_in` | `POST /v1/shield/captcha/init`（action=method:path、`meta.email`、本地 `device_id`）→ `POST /v1/auth/signin`（正文仅 client_id/username/password；token 放 `x-captcha-token` 请求头） | 官网 Web SDK 已核对 | 无默认 client_secret，无 grant_type=password 或正文 captcha_token。只要响应有 url，就需人工验证，即使同时有初始 token。 |
| `auth_verify` | 人工验证回调返回新的 CAPTCHA token；重新输入密码，用新 token 请求 signin | 官网回调语义已核对；真实人工流程待用户验证 | 回环监听只接收随机 state/path 匹配的回调。不能凭点击“已完成”就继续；短信/邮箱验证码重发仍未实现。 |
| 刷新 | `POST /v1/auth/token`（`grant_type=refresh_token`） | 已核对 | `invalid_grant` → 清 token，`auth/changed` 发 `expired`。并发请求只刷新一次（pikpaktui `refresh_session_inner`）。 |
| `auth_status` | `GET /v1/user/me` + `GET /drive/v1/privilege/vip` | 已核对 | `display_name` 由核心脱敏（`k***@gmail.com`）。 |
| `auth_logout` | 本地删除 token | 已核对 | 远端注销接口待验证，不影响实现。 |
| `quota_get` | `GET /drive/v1/about`（容量）+ `GET /vip/v1/quantity/list?type=transfer`（传输额度）+ `privilege/vip` | 已核对 | 映射到 `QuotaReport`；拿不到的项用 `exactness: "unavailable"`，不要推算。 |

### 4.2 文件

| IPC | 接口 | 状态 |
|---|---|---|
| `files_list` | `GET /drive/v1/files?parent_id=&limit=500&filters={"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}}&page_token=`；`next_page_token` → `next_cursor`。前端根目录 id 为 `"root"`，核心换成 PikPak 的根（`parent_id` 空串，rclone 同样处理）。同一 token 重复出现要报错（pikpaktui 有此保护）。 | 已核对 |
| `files_stat` | `GET /drive/v1/files/{id}` | 已核对 |
| `files_path` | 无专门接口：沿 `parent_id` 逐级 `stat`，结果缓存 | 已核对（组合） |
| `files_search` | 名称过滤 | **待验证** |
| `files_recent` | `GET /drive/v1/events?limit=` | 已核对 |
| `trash_list` | `GET /drive/v1/files?parent_id=*&filters={"trashed":{"eq":true},...}` | 已核对 |
| `files_starred`（§P16） | `GET /drive/v1/files?parent_id=*&filters={...,"system_tag":{"in":"STAR"}}` | 已核对 |
| `files_mutate` mkdir | `POST /drive/v1/files` `{kind:"drive#folder", parent_id, name}` | 已核对 |
| rename | `PATCH /drive/v1/files/{id}` `{name}`；`file_rename_uncompleted` 可重试 | 已核对 |
| move / copy | `POST /drive/v1/files:batchMove` / `:batchCopy` `{ids, to:{parent_id}}`；可能返回异步任务，轮询 `GET /drive/v1/tasks/{id}`（pikpaktui `mutation.rs`：每批 ≤500，轮询 10×500 ms） | 已核对 |
| trash / restore / delete_forever | `:batchTrash` / `:batchUntrash` / `:batchDelete`；清空回收站 `POST /drive/v1/files/trash:empty` | 已核对 |
| star / unstar | `POST /drive/v1/files:star` / `:unstar` `{ids}` | 已核对 |

`files_mutate` 的幂等键由核心记录在 SQLite：同一个键第二次到达时返回首次结果并置 `replayed: true`，**不要**再次请求 PikPak。

### 4.3 下载、画质、片段

| IPC | 接口 / 做法 | 状态 |
|---|---|---|
| `transfer_start`（原文件） | `GET /drive/v1/files/{id}` → 优先 `medias[]` 中的原画链接，其次 `web_content_link`（pikpaktui `file_info.rs` 有完整取舍与测试）→ HTTP Range 多连接写 `.part`，完成后校验再改名（pikpaktui `download.rs`：默认 4 连接、8 MiB 分块、`.part` 身份文件防误删）。签名链接只在核心内使用，不进 IPC、不进日志。 | 已核对 |
| `options.quality_id` / `quality_cap`（§P14） | 同一接口的 `medias[]` 转码档；`media_probe.qualities` 由它生成，`readable=false` 对应拿不到链接的档位 | 字段已核对，取舍规则需实测 |
| `options.clip`（片段下载） | 原画 MP4：Range 读 `moov`（`stss` 同步样本 + `stco/co64`）→ 找起点前一个关键帧 → 只下载这段样本并重写 `moov` 输出新 MP4；MKV：读 `Cues`；转码档若为 HLS：按 m3u8 分片选段拼接 TS。全部纯 Rust，不带 FFmpeg。 | **待实现**；接口本身只用到 Range 下载 |
| `media_align` | 用上面的关键帧索引计算实际起点与字节估算 | 待实现 |
| `media_preview_frame` | PikPak 没有任意时刻截图接口；无解码器时返回 `preview: "unavailable"`，可以用 `thumbnail_link` 做封面 | 已核对（无此接口） |
| `media_open_external` | 核心取链接后直接启动系统默认播放器，链接不经过前端 | 已核对（组合） |
| `transfer_start_batch`（文件夹） | 递归 `files_list` 展开；超过上限返回 `skipped` | 已核对（组合；pikpaktui `download_dir`） |

### 4.4 云下载、链接预览、分享、解压

| IPC | 接口 | 状态 |
|---|---|---|
| `cloud_task_create` | `POST /drive/v1/files` `{kind:"drive#file", upload_type:"UPLOAD_TYPE_URL", url:{url}, parent_id, folder_type:""}`（不传 parent 时 `folder_type:"DOWNLOAD"`） | 已核对 |
| `cloud_task_preview`（§P20） | `POST /drive/v1/resource/list` `{urls, page_size:500, thumbnail_type:"FROM_HASH"}` | 已核对 |
| `cloud_task_create.file_indices` | 只保存部分文件的参数 | **待验证** |
| 云下载进度 | `GET /drive/v1/tasks`（按 phase 过滤）/ `GET /drive/v1/tasks/{id}`；删除 `DELETE /drive/v1/tasks` | 已核对 |
| `share_create` / `shares_list` / `shares_cancel` | `POST /drive/v1/share` / `GET /drive/v1/share/list` / `POST /drive/v1/share:batchDelete` | 已核对 |
| `archive_extract` | `POST /decompress/v1/decompress` `{gcid, file_id, password, files, default_parent}`（rclone `helper.go`） | 已核对 |
| `archive_list` | 列出压缩包内容的接口 | **待验证**（rclone 只记录了网页 `https://mypikpak.com/drive/decompression/{id}?gcid=`） |
| 上传 / 秒传 | rclone `multipart.go`、`GET /drive/v1/resource/cid`；pikpaktui `upload.rs`（OSS 分片） | 已核对，M1 再接 |

### 4.5 不需要 PikPak 接口、由核心计算的

- `duplicates_scan`（§P17）：递归列目录，按 `hash`（gcid）+ 大小分组；没有 hash 的文件计入 `unhashed_files`。用 `nekopak://duplicates/progress` 报进度。
- `files_space_summary`（§P18）：递归累加大小。
- 两者都走 SQLite 目录缓存和全局限速，避免一次扫描打出几千个请求。

## 5. 登录状态机（前端已按此实现，见 `src/pages/Auth.tsx`）

```
auth_sign_in(username, password)
  ├─ 成功                    → { state: "signed_in" } + auth/changed(signed_in)
  ├─ 需要人机验证（captcha init 含 url，即使同时有 token）
  │                          → { state: "challenge", challenge: { kind: "captcha", url } }
  │                            用户完成浏览器验证 → 回环回调带回新 token → 重新输入密码 → auth_verify(challenge_id, "", password)
  ├─ 需要验证码（待验证）     → { state: "challenge", challenge: { kind: "verify_code", channel, target, code_length } }
  ├─ 账号或密码错误           → ClientError PERMISSION_DENIED, details.reason = "invalid_credentials"
  └─ 网络失败                 → ClientError TRANSIENT_NETWORK, retryable
```

密码只存在 `auth_sign_in` 这一次调用的参数里。核心收到后立即用于 signin，然后丢弃。

## 6. 错误映射（PikPak 返回 `{ error, error_code, error_description }`）

| PikPak | ClientError | 自动重试 |
|---|---|---|
| HTTP 401、`unauthenticated` | 先刷新 token；刷新失败 → `AUTH_EXPIRED` | 刷新后重试一次 |
| `invalid_grant` | `AUTH_EXPIRED` | 否 |
| `captcha_invalid` | 重新 init captcha；需要人工 → 登录 challenge | 是（一次） |
| HTTP 403 | `PERMISSION_DENIED` | 否 |
| HTTP 404、`file_not_found` | `NOT_FOUND` | 否 |
| `file_space_not_enough` | `QUOTA_EXCEEDED`（`details.dimension = "storage"`） | 否 |
| `task_daily_create_limit*` | `QUOTA_EXCEEDED`（`cloud_download_daily`） | 否 |
| HTTP 429 | `TRANSIENT_NETWORK`，按 `Retry-After` / 指数退避加抖动 | 只自动重试只读请求 |
| `file_rename_uncompleted`、`file_duplicated_name`（刚改动后） | — | 是（rclone 同样处理） |
| 5xx、超时 | `TRANSIENT_NETWORK` | 只读请求自动重试；写操作靠幂等键，由用户触发重试 |

原生实现仅自动重放刷新后的只读 GET 一次；没有实现表中建议的其他自动重试。所有错误带 `request_id`，`message` 不含 token、签名链接或完整请求体。

## 7. 限速

全局令牌桶（建议起步 5 请求/秒，突发 10），列表分页串行，批量操作每批 ≤500。不因为 429 加大并发；“每个任务的连接数”（1–16，默认 4）只作用于单个文件的 Range 下载。

## 8. 前端已经就绪的部分

- 全部命令与类型：`src/lib/contract.ts`（冻结）+ `src/lib/contract-proposals.ts`（§P1–§P23）。
- 可执行的行为参考：`src/lib/mock/backend.ts`，每条命令的成功、失败、幂等、事件顺序都在里面，`tests/unit/mock-contract.test.ts` 有断言。
- 接入方式：`VITE_NEKOPAK_TRANSPORT=tauri npm run build`，命令参数默认扁平（`VITE_NEKOPAK_IPC_ARGS=wrapped` 可切换）。
- `backend_discover` 只需要返回一条 `rust_compat` 报告；`operations` 里为 `true` 的能力前端才会启用（§P13 的扩展键同样适用）。

## 9. 验收

1. `cargo test` 覆盖：登录（成功 / 密码错 / 需要人机验证）、刷新竞态、分页重复 token、批量操作异步轮询、幂等回放、错误映射表每一行。
2. 用真实账号跑通：登录 → 列目录 → 新建、重命名、移动、移至回收站、还原 → 下载一个文件（断点续传）→ 添加一条磁力链接 → 配额显示。
3. 前端以 Tauri 运行时 E2E 全绿；每项“待验证”的接口在 `docs/IMPLEMENTATION_NOTES.md` 写明实测结果。
