# 基础文件、分享、配额与原文件下载协议核对

核对日期：2026-11-08。目的：为 `rust_compat` 补齐可实际接入的最近、星标、回收站、
基础文件操作、分享列表、配额和原文件下载提供一手协议依据。

本次只读取官网公开浏览器脚本，以及固定提交的 Apache-2.0/MIT 源码。
没有读取用户凭据，没有登录、假密码尝试、CAPTCHA 提交、文件请求、下载或账户写操作；
没有运行官方/社区 CLI，没有复制客户端实现到产品。下文“核对”均为源码观察，
不等于服务端成功实测。可以据此独立实现并交付用户测试，不能标为真实账号验收通过。

## 1. 可以立即着手的最小切片

1. 统一 DRIVE 请求认证与 CAPTCHA 后，接入三个分页读取：普通/星标/回收站文件列表、
   最近事件、我的分享。它们的响应容器不同，分别为 `files[]`、`events[]`、`data[]`。
2. 接新建文件夹、重命名、星标/取消星标、移入回收站/恢复、移动/复制。
   先落持久请求意图和结果核对，明确区别“已受理异步任务”和“已完成”。
3. 配额分开读取存储容量和传输使用量，禁止把 `assets` 当剩余。
4. 普通下载按官网 `GET file?usage=FETCH → web_content_link` 起步；CDN 请求与 API
   认证客户端隔离，先实现安全 `.part`、长度/身份核对、暂停/续传，再接多连接。
5. 搜索先在 Rust 全量目录索引上实现名称匹配；当前未找到可据此启用的服务端名称搜索过滤。

这些步骤都不需要恢复 CLI 方案。片段裁剪、转码画质、压缩包内部读取、挂载并不由普通
文件链接自动具备，仍需各自实现和验证。

## 2. 来源、固定版本与许可

官网构建为 `v2.30.3-9-gcc540f8e`。入口 HTML 的 build-id 为 `cc540f8e`，
build-time 为 `Fri, 06 Nov 2026 10:40:12 GMT`。官网脚本没有建立可复用源码许可，
本次只研究协议事实，不能将 webpack 模块或组件复制到 NekoPak。主要来源：

- [官网 DRIVE 请求包装器 5492.76e680dd.js](https://mypikpak.com/drive/5492.76e680dd.js)：
  文件、事件、批量操作、分享、任务、about 的 endpoint/method/query/body。
  SHA-256 `3343d4a37d421070c3f3b9b80c1d3c60e0f9e84ee2a6ffc75ca0198f1af5dba7`。
- [官网文件状态/索引 2431.8bd55f43.js](https://mypikpak.com/drive/2431.8bd55f43.js)：
  `getEvents`、`getTrashedFiles`、`getSharedList`、全量同步和本地搜索过滤。
  SHA-256 `b0a7b330f7d75bf9c74934a7d79cc1e36929488e7a4e5b69644c050d3d2463d9`。
- [官网文件浏览 5640.74b82e69.js](https://mypikpak.com/drive/5640.74b82e69.js)：
  500 条分页、`system_tag.in=STAR`、完整阶段过滤、列表更新。
  SHA-256 `f7b58558d6000d7144a65f272da040a7bbc04bb88df64cf9d326e91ed9a56479`。
- [官网传输额度 2767.df225ce8.js](https://mypikpak.com/drive/2767.df225ce8.js)：
  `GET /vip/v1/quantity/list?type=transfer`、账号/已连接应用的独立层级。
  SHA-256 `b3dcd424b67f631c3daeddf6af678b460d6feaf7db267ebab368fc2437b8ebf5`。
- [官网下载入口 9173.9f7d05c6.js](https://mypikpak.com/drive/9173.9f7d05c6.js)：
  `usage=FETCH`、`web_content_link` 和媒体详情读取。
  SHA-256 `af96dd527ea664d470c7656952b9dd5446a50041f571d04c0abeca3d811bf02b`。
- [官网枚举/错误 3765.9a7a52c0.js](https://mypikpak.com/drive/3765.9a7a52c0.js)：
  文件/任务 phase、配额错误名、最近/回收站/星标路由。
  SHA-256 `0914dc27fc415a5915a46dfda6b7a5d0cb8716bf066940b7bbaae41f16bdfedc`。

社区一手源码均通过 GitHub Contents API 按以下完整提交读取，没有运行：

- [Bengerthelorf/pikpaktui](https://github.com/Bengerthelorf/pikpaktui/tree/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c)，
  commit `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`，
  [LICENSE](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/LICENSE)
  为 Apache-2.0，license blob `999160cf62c7a10bfd8e5eafe539757026ad8036`。
- [rclone/rclone](https://github.com/rclone/rclone/tree/82cb84b2014bec6a167dc28eb53233b2d4abe1b3)，
  commit `82cb84b2014bec6a167dc28eb53233b2d4abe1b3`，
  [COPYING](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/COPYING)
  为 MIT，license blob `8c27c67fd0a12a1e18db983f19450abed435e689`。

原始公共源码和脚本仅缓存于被忽略的 `target/file-protocol-audit/`、`target/login-audit/`。
缓存清单记录 URL/commit/path/blob/SHA-256：`manifest-pikpaktui.json`、
`manifest-additional.json`、`manifest-official.json`。不得把缓存源码打入产品包。
后续若选择实际改编 Apache/MIT 实现，仍须记录原路径与修改并保留许可证/NOTICE；
当前独立协议实现不等于已经完成上游代码复用。

## 3. 文件读取与列表

DRIVE origin：`https://api-drive.mypikpak.com`。本节的 endpoint 都相对此 origin。
授权/CAPTCHA 由 Rust 持有；登录协议与 DRIVE 风控参数不能混用，参见
`NATIVE_LOGIN_RESEARCH.md`。查询字符串里的 `filters` 是 JSON 文本，经 URL 编码传输。

### 普通列表、星标、回收站

共同请求：`GET /drive/v1/files`，查询 `parent_id`、`limit`、`page_token?`、`filters`，
官网和 pikpaktui 还使用 `with_audit=true`、可选 `thumbnail_size`。
响应为 `{files:[File...], next_page_token?:string}`。空/缺省结束游标结束分页；
存在游标时继续，不因当前页过滤后没有可显示条目而自行终止远端分页。

- 普通目录根的 wire `parent_id` 为 `""`，NekoPak 根 sentinel 仍为 `root`。
  filters 为 `{"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}}`。
- 星标全盘列表用 `parent_id="*"`，在普通 filters 增加
  `"system_tag":{"in":"STAR"}`。不要猜成 `starred=true` query。
- 回收站用 `parent_id="*"`，filters 为
  `{"trashed":{"eq":true},"phase":{"eq":"PHASE_TYPE_COMPLETE"}}`。
  元数据里的真实父目录仍可能是原目录；不能要求每个条目的 parent_id 等于 `*`。
- 全量索引扫描也可用 `parent_id="*"` 加普通 filters，每页 500。

来源：[pikpaktui files.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/files.rs)
的 `ls`、`ls_trash`、`starred_list`；官网 2431 的 full-sync/getTrashedFiles 和 5640 的
`system_tag` 请求。注意 pinned `starred_list` 自身只取一页，这不是 NekoPak 可以截断
星标列表的理由；返回模型仍有 next_page_token，官网列表会持续翻页。

文件核心字段：`id,name,kind,parent_id,size,created_time,modified_time,phase,trashed,tags`；
`kind` 为 `drive#file` / `drive#folder`。星标可从 `tags[].name == "STAR"` 判断。
Rclone 还定义可选 `starred` 布尔，兼容解析可以接受两种已观察形状，但不能凭缺字段
证明取消了星标。`size` 通常是字节数字串；pikpaktui 还接受 JSON u64。若接受数值变体，
应在 Rust 做精确整数解析，跨 IPC 仍规范化十进制字符串，禁止 JS 浮点中转。
文件缺 size 不能伪造 0；文件夹可按已有契约显示 0。

来源：[pikpaktui drive.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/drive.rs)、
[Rclone api/types.go](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/api/types.go)。
`hash` 是 GCID/服务内容身份，不得改标为普通 SHA-1；另有 `md5_checksum` 才可按 MD5 验证。

### 最近

`GET /drive/v1/events?limit=<n>&thumbnail_size=SIZE_MEDIUM&page_token=<cursor>`。
官网最近首页直接调用事件包装器，无 `TYPE_PLAY` 过滤；播放历史才使用
`filters={"type":{"in":"TYPE_PLAY"}}`，二者不是一个页面。

响应为 `{events:[Event...], next_page_token?:string}`。
事件自身有 `id,type,file_id,file_name,created_time,updated_time,mime_type,params`；
`reference_resource` 是可显示/操作的文件记录。官网把该对象展开为文件，再用事件时间
和 MIME 增补显示字段。**不要用 event.id 作为 RemoteEntry.id**；取
`reference_resource.id`，与存在的 `file_id` 做一致性核对。缺少必要文件字段时可用
file_id 做 stat 补齐，或明确跳过已删除/无权条目，不构造可操作的假文件。
同一文件可能有多个事件：UI 要求 RemoteEntry 列表时应按文件 ID 去重并保留最新事件顺序。

来源：官网 5492 的 `U` 事件包装器与 `hK` 播放历史包装器；2431 的 `getEvents` 和
事件到文件映射函数 `S`；[pikpaktui mod.rs:729](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/mod.rs#L729)、
[responses.rs 的 EventsResponse](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/responses.rs)。
pikpaktui 的简化 EventsResponse 未保留游标和完整引用对象，官网源码补齐了此证据。

### 搜索

当前读到的官网搜索不是已证实的 `filters.name.match` endpoint。
2431 模块 `88553` 对本地 node map 做 `lowerName.includes(lowercaseQuery)`，同时按
星标、kind、MIME 前缀、来源 platform 过滤；2431 的 full-sync 通过 `parent_id="*"`
取全量完整阶段、未回收文件。Rclone 的 FindLeaf 也先列目录再按名称比较。

可独立实现：Rust 受限分页扫描 → 按账号保存目录索引 → 本地名称匹配/分页，之后增量更新。
必须区分索引未完整、扫描失败和完整零结果；不把只扫前 500 条标成全盘搜索。
当前未核对服务端名称/模糊搜索接口，不能根据旧附件“名称过滤”四字臆造 query。
更高级的简繁/日文模糊匹配仍属于独立实现和验收内容。

## 4. 基础修改与异步完成

以下 body 均是结构化 JSON；根目录 parent_id 从 `root` 转 `""`：

- 新建目录：`POST /drive/v1/files`，`{kind:"drive#folder",parent_id,name}`。
  返回 `{file:{...}}`，不是裸 RemoteEntry；从 file 读取真实 ID/name/parent。
- 重命名：`PATCH /drive/v1/files/{id}`，`{name}`。
  Rclone 按更新后的裸 File 解码；pikpaktui 只判断成功。NekoPak 可在成功后 stat 验证名称，
  不能把服务器返回原名或未完成阶段显示为重命名成功。
- 移动/复制：`POST /drive/v1/files:batchMove` / `:batchCopy`，
  `{ids:[...],to:{parent_id}}`。普通自己文件不需要分享/压缩包的额外权限字段。
- 移入回收站/恢复：`POST /drive/v1/files:batchTrash` / `:batchUntrash`，`{ids:[...]}`。
- 星标/取消：`POST /drive/v1/files:star` / `:unstar`，`{ids:[...]}`。
- 永久删除：`POST /drive/v1/files:batchDelete`，`{ids:[...]}`，须单独危险确认。
- 清空回收站：**`PATCH /drive/v1/files/trash:empty`**。官网与 pikpaktui 均使用 PATCH；
  旧附件里写的 POST 不能作为当前实现依据。

来源：官网 5492 请求包装器；[pikpaktui files.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/files.rs)；
[Rclone helper.go](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/helper.go)
的 requestBatchAction、requestNewFile、patchFile。

批量每次最多 500 是 pinned pikpaktui 的分块边界。已核对成功响应形状包括同步
204/空 body、`{task_id:"..."}`、`{task:{id,phase,...}}`；不能要求所有成功响应都有 JSON。
若 task.phase 已 COMPLETE，可确认任务成功；ERROR 是失败；其他 phase 需后续查询。

任务查询有两条一手依据：

- 官网和 Rclone：`GET /drive/v1/tasks/{id}`，返回裸 Task。
- pikpaktui：`GET /drive/v1/tasks?type=&limit=500&filters={"id":{"in":"<id>"}}`，
  从 `tasks[]` 中匹配 ID。其策略是 10 次、间隔 500ms；这是参考实现策略，不是服务 SLA。

phase 包含 `PHASE_TYPE_PENDING`、`RUNNING`、`COMPLETE`、`ERROR`、`PAUSED`，
未知值的实际拼写在官网为 `PHASE_TYPE_UNKNOW`。任务未找到、轮询超时、解析失败，均不能
把“已受理”升级为完成，也不能自动重新提交同一批修改。

来源：[pikpaktui mutation.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/mutation.rs)、
[parity_tests.rs:152](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/parity_tests.rs#L152)、
[Rclone helper.go:186](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/helper.go#L186)。
这些上游测试只是被阅读，没有在本次运行。

Rclone 注释说明：移动到当前目录/自身子目录可能返回 `file_move_or_copy_to_cur`；目标同名时
服务可能自动追加 `(1)`。NekoPak 必须读取结果/重新 stat，不能承诺同名必定报错或目标名字不变。
来源：[pikpak.go:1086](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/pikpak.go#L1086)。

本地 idempotency_key 不能自动变成服务端幂等保证。需先持久化 intent，再提交，记录
task ID/已确认结果；网络断在副作用发生后时保留 unknown/pending 并核对，禁止盲重发。
后端返回 succeeded 只包含已确认项目，部分批次成功后不得从第一批重来。

## 5. 我的分享

`GET /drive/v1/share/list?limit=100&thumbnail_size=SIZE_SMALL&page_token=<cursor>`。
响应 `{data:[MyShare...], next_page_token?:string}`。所有页面须读取；重复/循环游标应明确失败，
不能效仿部分社区代码在重复游标时静默结束并显示“全部”。

记录字段包括 `share_id,share_url,title,pass_code,share_to,create_time,expiration_days,
view_count,restore_count,file_num,share_status`；多个数量/天数字段实际模型为字符串。
官网另读取 `file_kind,file_size,expiration_at,expiration_left,expiration_left_seconds,
share_status_text,follow_count`。

`share_status="OK"` 表示官网认为可用；官网以 `expiration_at="-1"` 判断永久有效。
其他状态不能一律映射 active，也不能仅由 expiration_days 猜测 expired/cancelled。
如果现有 UI 状态枚举无法表达服务限制，应扩充 unknown/restricted 或给出明确不可用原因。
分享 URL 是可对外分发的分享页，与签名 CDN 下载地址不是一类，仍需 HTTPS/host 校验。

创建分享：`POST /drive/v1/share`，参考 body 为
`{file_ids:[...],share_to:"encryptedlink"|"publiclink",expiration_days:<integer>,
pass_code_option:"REQUIRED"|"NOT_REQUIRED"}`；返回 `share_id,share_url,pass_code?,share_text?`。
取消分享：`POST /drive/v1/share:batchDelete`，`{ids:[share_id...]}`，同样核对异步完成。
永久分享 expiration_days 的 wire sentinel 本次没有足够直接证据，不应把 UI null 随意转 0/-1。

来源：官网 5492 的 Q/X/ce 与 2431 的 getSharedList/格式化函数 P/T；
[pikpaktui share.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/share.rs)、
[responses.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/responses.rs)。

## 6. 存储容量、传输额度、会员

容量：`GET /drive/v1/about` → `quota:{limit,usage,usage_in_trash}`，字节数通常为数字串。
容量剩余可由确实存在的 limit/usage 做饱和减，但不能把缺失字段补成 0。回收站用量
是否计入 usage、usage_in_trash 的服务可靠性应实测，不重复扣减；Rclone 对 trash 字段有疑问注释。
官网 about 包装器还能传重复 query `with_quotas`，不是初版容量读取必须项。

传输：`GET /vip/v1/quantity/list?type=transfer`。官网还支持 `begin`、`limit`，用于
其更多配额明细；初版仅取已核对的 base 汇总，不能误把所有嵌套明细加总。

`base.offline`、`base.download`、`base.upload`、`base.download_daily` 的 band 含
`total_assets` 和 `assets`；**assets 是已使用量 used，不是 remaining**。
`base.expire_time` 表示来源模型给出的有效时间，不能未经验证标为每日或每月重置点。
`offline` 对应云下载流量维度，不是本地离线缓存。
官网还区分 `transfer`、`apps_summary`、`clients`、`sessions`：不得把客户端会话的
限额覆盖账号 base，不能直接把 base 标为 Connected Apps 额度。

`download_daily.total_assets=0` 在 pinned pikpaktui 显示“无日限额”；这和额度耗尽不是
同义。缺失 daily/band/字段依旧未知。若不能在目标账号核验该特殊值，可显示“未提供日上限”
而不是“剩余 0 / 已耗尽”。只有明确 total>0、used 已知时才算 remaining=max(total-used,0)。
参考客户端默认零值的宽松解析不是 NekoPak 可以伪造额度的依据。

来源：[pikpaktui account.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/account.rs)、
[responses.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/responses.rs)、
[src/cmd/quota.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/cmd/quota.rs)
明确把 assets 映射 `used`，该文件 blob 为 `aaeae46b1bd50cde0edd739c91781672f319a4e2`，
SHA-256 `b217b8a098c68a4acb1ec4c92ab779af4717073de7d14b9454f522221f5e4028`；
官网 2767 的 quantity 请求/消费逻辑、Rclone api/types.go 的 About/Quota。

会员：`GET /drive/v1/privilege/vip` → `data:{type,status,expire}`。字段含义以实际结果为准；
参考 fixture 示例是 `type="platinum",status="ok"`，但 fixture 不证明所有套餐值。
身份：认证 host 上 `GET /v1/user/me`，含 `sub,name,email,...`。`sub` 留后端用于账号隔离；
昵称/邮箱必须脱敏后再过 IPC。接口缺失或读取失败不能降级猜成免费账号。

## 7. 原文件下载直链与安全字节读取

最接近当前官网的普通下载路径：

1. `GET /drive/v1/files/{id}?usage=FETCH`；`usage` 是 query。
2. 核对 phase、文件 ID、类型、大小、可用权限；取 `web_content_link`。
3. URL 只在 Rust 内使用，创建本地持久任务；UI 获得 task ID/显示字段，不获得 CDN URL。

依据：官网 9173 的下载处理器默认 `usage="FETCH"`，从响应取 web_content_link；
5492 的详情包装器 F 编码 `usage`、可选压缩包密码。官网媒体查看另外出现 `usage="ALL"`，
不应将媒体播放请求形状默认为整文件下载。

兼容来源：pikpaktui `FileInfoResponse::download_url` 优先取
`medias[].is_origin=true && is_visible!=false && need_more_quota!=true` 的 `link.url`，
其次 web_content_link，再 `links["application/octet-stream"].url`。
Rclone 在用 media 链替换 octet-stream 链时额外比较 URL `fid`，避免误选转码表示。
NekoPak 首版可坚持 FETCH 的原文件链接；若增加 fallback，不能静默选转码或绕过媒体权限标记。

来源：[pikpaktui file_info.rs:221](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/file_info.rs#L221)、
[Rclone pikpak.go:1785](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/pikpak.go#L1785)。
链接有效期可在 `Link.expire` 或 query `expire`（Unix 秒）中出现；Rclone 留 10 秒余量。
缺可靠有效期不能长期缓存为永久链接。

后续字节读取的实现约束（NekoPak 独立安全要求，不能把参考代码当已满足）：

- 使用独立 CDN HTTP client，不携带 API Authorization、x-captcha-token、客户端认证 cookie。
  保留 TLS 校验；只接受 HTTPS，拒绝 userinfo 和本地/私有地址，重定向每一跳重新验证。
  域名清单未由本次真实账号枚举，不能随意硬编码一组 CDN 主机后声称完整覆盖。
- 不把 reqwest 含 URL 的原始错误、query、media 元数据、完整响应写日志/IPC/SQLite。
- 首次安全创建任务专属 `.part` 与源身份记录；不要覆盖既有用户文件。零字节文件可能没有
  下载链接，须依据已核实元数据安全创建空文件，而不是从链接缺失推断空文件。
- 续传 `Range: bytes=<offset>-` 或有界范围。206 必须核对 Content-Range 起止、总大小和
  实际 body 长度；200 表示未按 Range 返回，不能追加旧 .part。416 不能直接标完成，需核对长度/版本。
- 原内容身份、expected size、ETag/Last-Modified 等可用版本信息必须关联任务；直链过期后
  重新读取详情且重新核对身份。API 401 和 CDN 签名过期不是同一种登录错误。
- 完成需实际字节长度/可用 hash 校验及安全提交；下载进度来自写入字节，不能沿用 Mock tick。

协议对照：[pikpaktui download.rs:253](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/download.rs#L253)
的 200/206 处理和有界 Content-Range 验证；
[Rclone pikpak.go:1936](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/pikpak.go#L1936)
用独立请求读取 URL，并把非 2xx 转为错误，防止把错误 HTML 写成下载文件。

## 8. 错误与实现时容易遗漏的接缝

已观察的 API 错误对象为 `error,error_code,error_description,error_url?,details?/error_details?`。
只读取已核验 code/name 来做结构化错误，不能照搬社区客户端将原 body 输出终端的做法。

- `file_space_not_enough`、`task_daily_create_limit*` → QUOTA_EXCEEDED。
- 官网明确出现 `monthly_download_size_exceeded`、`daily_download_size_exceeded`、
  `monthly_upload_size_exceeded`、`monthly_cloud_download_size_exceeded`，应映射相应配额维度。
- `file_duplicated_name`、`file_rename_uncompleted`、`file_move_or_copy_to_cur` → 冲突/未完成操作，
  提示刷新核对；Rclone 的场景重试策略不能直接移植成所有写操作盲重试。
- `file_not_found` → NOT_FOUND；`unauthenticated` → 会话处理；`captcha_invalid` → 正常验证流程。
  配额、账号受限与删除后的 NOT_FOUND 都不通过换后端或增加连接数解决。

来源：官网 3765 错误枚举；[Rclone shouldRetry](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/pikpak.go#L422)
和 api/types.go 的 Error。

当前代码接入这些功能时需要显式协调：

- 普通文件 parser 拒绝 trashed=true、普通 list 强制 parent 一致的保护，不能原样用于
  回收站/星标/全盘索引；应加入明确读取模式，同时保留每种模式自身的合法性校验。
- HTTP helper 的 JSON-only 成功解析必须为确有依据的 204/空 mutation 响应保留路径，
  不能为方便而把所有空响应都当成功。
- 服务操作缺少已确认结果/异步任务状态时，现有 OperationResult 成功列表不可提前填满；
  必要时先扩充契约/持久任务模型，再开放按钮。
- 当前 QuotaReport 的旧 Mock dimensions 与前端 account/connected_apps scope 必须一起更新；
  不能直接把真实返回塞入旧的“模拟额度”文案，也不能丢掉存储维度。
- 最近、星标、分享、回收站都需完整分页与事件/文件 ID 区分；前端一次读取的默认数量
  不是服务数据总量。失败/未完成扫描不产生任何同步删除结论。

本次新增文件仅本文；公共源码缓存均在 ignored target。功能代码、接口冻结、离线回归和
用户真实账号测试由主任务继续完成。
