# 引用归档与安全取回：协议核验

日期：2026-11-08。本文是只读研究与实现建议，**不表示归档功能已实现或真实账号已验收**。
没有使用用户凭据、执行客户端探针、创建/删除远端文件或调用真实账号 API。
本轮仅读取固定提交的一手源码、上游设计文档及本仓库现有代码；未复制上游实现。
社区客户端协议不称为官方公开 API。

## 1. 可据此实施的结论

引用归档应先生成完整、可恢复的清单并确认其持久化，再根据用户明确选择处置原文件。
取回优先恢复回收站原对象，只有原对象不存在且用户授权创建新对象时才尝试 GCID 秒传。
下列情况都必须保留清单并停止后续处置：枚举未完成、账号变化、内容身份变化、清单确认失败、
容量不明确而无法判断安全余量、恢复请求结果未知。

**新建自有分享不是独立备份。** MIT SDK 的 `ShareSummary` 和 `listMyShares` 文档明确记录：
文件被删除后，原分享仍可出现在列表中，但状态为 `DELETED`。因此“先创建分享并打开验证，再删掉
分享指向的原文件”不足以证明之后可取回。已核对资料没有证明分享在原文件进入回收站后的所有阶段
都仍能用；不能把这个未验证假设用于删除条件。[S1]

**GCID 命中不是永远可恢复保证。** 内容可能已不可用。`GET /drive/v1/resource/cid` 需要
`cid` 和 `file_size`，不存在本次证实的 `?gcid=...` 查询形式。返回 GCID 只说明索引命中；
真正创建请求的完成结果才证明当时能建立文件对象。CID 本身是抽样身份，不能当成全文件完整性哈希。[S2]

## 2. 已核实的远端操作

### 回收站与恢复

- 枚举回收站使用既有 `GET /drive/v1/files`，`parent_id="*"`，
  `filters={"trashed":{"eq":true}}`；按 `next_page_token` 完整翻页。SDK 明确说明
  `trashed` 是 JSON 布尔，不是字符串，名称匹配由客户端分页后执行；`parent_id="*"`
  的具体回收站请求可在固定 pikpaktui `ls_trash` 核对。[S3][S10]
- 移入回收站：`POST /drive/v1/files:batchTrash {"ids":[...]}`。[S4]
- 恢复原对象：`POST /drive/v1/files:batchUntrash {"ids":[...]}`。
  此操作恢复到原父目录；没有“同时设置任意新父目录”的已证实参数。需要另选目录时，先确认恢复，
  再以独立用户意图执行已有移动操作。[S4]
- `delete_time` 是服务提供的回收站到期时间。SDK 某个账号曾观察到 15 天，但不能把 15 天硬编码成
  所有账号的保证。缺该字段时显示未知，不自行计算。[S5]
- 永久删除与清空回收站属于不同的破坏操作，不是归档默认步骤。SDK `deleteFile` 使用
  `DELETE /drive/v1/files/{id}` 且明确为永久删除；不应将其命名为“安全清理”后自动调用。[S4]

上游批量大小不是稳定服务承诺：固定 SDK 记录 200 项曾接受、1000 项曾失败，自己选择每批 100；
本仓库此前另一参考实现按 500 分块。归档可以选择较保守的有界批次，但应将逐批进度持久化，
不能把整个多批操作误称为原子事务。[S4]

### GCID 秒传创建

`instantCreate` 不是单独的恢复 endpoint。固定 MIT SDK 的 `MagnetResolveEndpoint.kt`
直接复用 `UploadEndpoint.kt` 的 `createUploadNode`，即：[S6][S7]

```json
{
  "kind": "drive#file",
  "name": "original-name.ext",
  "size": "12345",
  "hash": "40_HEX_GCID",
  "upload_type": "UPLOAD_TYPE_RESUMABLE",
  "parent_id": "verified-target-folder-id",
  "body": {"duration": "", "width": "", "height": ""},
  "objProvider": {"provider": "UPLOAD_TYPE_UNKNOWN"}
}
```

请求为 `POST /drive/v1/files`。SDK 根目录省略 `parent_id`；NekoPak 现有上传协议已支持把
`root` 映射为空字符串。`size` 是十进制字符串，GCID 必须来自已核对内容元数据，不能用 CID、
普通 SHA-1 或文件名代替。返回至少包含 `file.id`、`file.phase`。[S6][S7][S8]

- 只有 `file.phase == "PHASE_TYPE_COMPLETE"` 且有合法 ID，才可能确认秒传完成。
  NekoPak 仍应读取新对象，核对 ID、父目录、名称、大小和 GCID 后将其标为已取回。
- `PENDING` 等其他阶段说明服务需要文件字节。此时已经创建真实待上传节点，不等于什么都没发生。
  上游观察显示节点不会自行消失，再次创建会产生带 `(1)` 的重复对象。[S6][S7]
- SDK 会自动永久删除这个待上传节点。该行为只是上游实现，不应直接照搬到 NekoPak。
  NekoPak 应记录 `pending_file_id`，显示无法秒传，允许对**本次确定创建且内容身份核对通过**的
  节点执行单独清理意图；网络超时而拿不到 ID 时只能标记结果未知，不能猜测对象并删除。
- 即使 COMPLETE，也可能暂时拿不到可用下载详情。SDK 明确记录这种可见性延迟。
  只能有界轮询核对已返回的 ID，不可因此重发创建。[S6]

秒传不传输上传字节，但仍创建占用完整大小的真实文件，并可能消耗上传额度。固定 SDK 记录了其
特定测量中的费用比例；NekoPak 不应把这些比例作为所有账号的准确计费公式或宣传其绕开其他限制。
应读当前账号容量/额度，保留服务端拒绝，绝不自动轮转账号。[S6]

## 3. 清单的最小持久数据

以下为 NekoPak 独立设计建议，不是声称上游 API 存在这些字段：

- `manifest_id`、`schema_version`、`revision`、账号 scope、创建时间、清单内容摘要。
- 文件树：每个根的原 ID，所有目录/文件的相对路径、原父 ID、名称、类型。目录必须保存层级而非
  仅保存拼接路径；循环、重复 ID、不完整分页必须失败。
- 每个文件：精确字节长度，`hash.algorithm="gcid"` 与值，可选服务 revision/mtime、原文件 ID，
  原父 ID，必要时的原父路径。没有 GCID 的条目必须显示缺少秒传恢复依据。
- 可选 CID：算法版本、取样方式、值和取样时的内容版本；没有 CID 不应伪造。
- 来源分类与来源记录引用。磁力/分享来源应经过明确允许的解析，下载签名 URL 不可写入清单。
  包含提取码/token 的资料仅存 OS 凭据库，清单只存不透明引用；此点不能照抄 Piko 的明文来源账本。
- 独立状态：`prepared`、`manifest_verified`、`trash_intent`、`trashed`、`restore_intent`、
  `restored`、`pending_upload`、`outcome_unknown`、`failed`、`cancelled`。
  每个条目保留请求幂等键、已知服务任务 ID、已知新文件 ID、最近确认时间和脱敏错误。
- 已恢复对象 ID 与其核验后的内容身份仍保留，不能删除清单行；这使改名/移动之后仍可识别内容，
  也能避免同一恢复请求创建重复对象。

本仓库当前 `RemoteEntry` 没有 `trashed`、`delete_time` 或来源字段，普通 `stat` parser 会拒绝
`trashed=true`。因此不能只拿当前 `files_stat` 就声称安全验证了回收站恢复。最小可行路径是完整
`FileQuery::Trash` 列表加内容匹配，但确认临界点仍宜有只在 Rust 内部使用的 `recovery_metadata`
接口，保留真实 trash 状态/到期时间及必要来源白名单。不要直接把整个 `params` 对象跨 IPC。

## 4. 最小正确的 Core/Provider 分工

下列名称为提案，需要根任务冻结到共享契约后实施：

1. `reference_archive_plan(ids, options, expected_account_id)`：只读、完整分页，生成有界清单候选和
   处置预览。返回 `plan_id`、版本摘要、条目/容量计数与不可恢复项目；不改远端文件。
2. `reference_archive_apply(plan_id, revision, trash_originals, idempotency_key)`：先将清单事务提交并
   重新读取核验。若提供跨设备归档，须上传真实清单文件并读回核对；仅本地数据库不能宣称等同
   Piko 云端目录清单。随后逐项再核对账号、原 ID、父 ID、名称、大小、GCID，才执行明确授权的 trash。
3. `reference_archive_restore(manifest_id, entry_ids, expected_revision, target, mode, idempotency_key)`：
   mode 明确 `trash_only` 或用户授权的 `allow_content_create`。先检查目标中是否已有同一内容，再完整
   核对回收站候选；匹配原目录+名称+GCID+大小，优先记录的原 ID。歧义不任选，交用户处理。
4. Provider 增量只需两个实际能力：
   `recovery_metadata(id, scope)`（已有 GET file endpoint、保留 trash 字段）；
   `create_from_content(name, size, gcid, parent, scope)`（上述实际 POST upload endpoint）。
   后者应返回 `Completed(receipt)` / `Pending(receipt)`，不能把 PENDING 伪装成错误且丢掉新 ID。
5. `reference_archive_status/list` 和 `reference_archive_cancel` 从持久记录读状态。
   取消停止后续动作，已提交远端动作仍需核对；取消不是反向删除。取回撤销须是新的显式意图，
   只处理此次确认为新建/恢复且未被用户修改的对象。

所有远端写入前保存意图；结果未知不重放。成功 HTTP 或“task complete”都不能代替最终对象内容核对。
正常批次部分失败应逐项保留，不能把完成项和失败项一起重发。

## 5. 上游行为与当前可交付边界

Piko 固定提交的 `docs/development/archive.md` 明确采用先写清单后处置原文件、恢复时优先同目录
同名同 GCID/大小的回收站对象、否则秒传并检查空间；其清单存于云端原目录，取回后保留恢复对象 ID。
本轮仅将其作为功能/行为证据，没有读取或复制 AGPL 归档实现。[S9]

完整目录归档不能直接 trash 整个被扫描目录：扫描完成后可能出现新文件，而服务无已证实的
“按目录 revision 条件删除”参数。保守实现只 trash 已核对且写入清单的文件 ID，并保留目录骨架；
若要处置目录必须重新枚举确认，仍需承认服务端并发写入不是本地检查可完全消除的竞态。

“释放了多少空间”应在处置后重新读取账号容量，不能把清单总字节直接称为实际释放量。
回收站有独立 `usage_in_trash` 元数据，但本次没有做真实账号容量核验。[S5]

建议必须覆盖的离线验收：清单提交失败绝不 trash；分页中断绝不完成计划；账号变化拒绝；同名不同
GCID 不恢复；原对象已恢复时幂等返回；恢复请求超时不重放；PENDING 新节点有可追溯 ID；源父目录
不存在或候选冲突不擅自移动；新文件加入目录不被连带处置；重启恢复 intent；清单损坏不重置。

## 6. 一手来源与校验

MIT SDK 的共同 commit：`8eaf5587a883ed4ea9bcca7b43a9194118719e63`。
以下链接均固定到该提交；本轮下载的额外内容存于 ignored `target/file-protocol-audit/`，
URL、路径、Git blob、SHA-256 清单为 `manifest-reference-archive.json`，既有清单为
`manifest-shared-media.json`。

- [S1 — ShareEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/ShareEndpoint.kt)：
  blob `ca8702de585b785b47e851a07519ee7996f03f0f`，SHA-256
  `63440a28d87ba93c408d3afa47ce00c60439222b0f8199e9e96649f1b1d1ddb9`。
- [S2 — CidEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/CidEndpoint.kt)：
  blob `daddd6285b5dea27c1f6361d709f6daae1704d9a`，SHA-256
  `f5b88047644a7d325079525ecc701da4c65329ccd813924acfd4905da1e72129`。
- [S3 — Endpoints.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/Endpoints.kt)：
  blob `97eb5eb4a66adf21c09f72160b2dc45041195d6f`，SHA-256
  `1e44e1020ef238b748bd6584cebfdf1821c7beabe34008d24b1a1dc46a6d5eb7`。
- [S4 — FolderEndpoints.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/FolderEndpoints.kt)：
  blob `ab454f2ea78b6e999b1b949414812c6c891cc236`，SHA-256
  `6fdb7cc5d6f22e283dfa0a55e7e8f66404f5d580ae194124af51951723b3d533`。
- [S5 — Models.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/Models.kt)：
  blob `b13e947060f2521e88c61f488891ef64e3f656a2`，SHA-256
  `74ee16741f2ac9c38293d3dacda8708cd65814cf49a46901b838df4c9036cd7d`。
- [S6 — MagnetResolveEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/MagnetResolveEndpoint.kt)：
  blob `1ded6da39003c45f0f7eb850397b3a5323e37ee7`，SHA-256
  `29899c72951cb94ef2078aa8501a8275b49b1c6aa60d79fe3eabe4639dd1c5a1`。
- [S7 — UploadEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/UploadEndpoint.kt)：
  blob `62d6512427e266b5c4357e29bc5061c3f1633610`，SHA-256
  `4fa2d7fd97473385739039b8c1357df44f55744ff9c55bfd47db7f2c1beca572`。
- [S8 — MagnetResolveMockTest.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonTest/kotlin/io/github/nihildigit/pikpak/MagnetResolveMockTest.kt)：
  blob `0c9a335cd7421f8eda52645866f35dbf279dd8e7`，SHA-256
  `63f0089c6e0e9a525f0e7f3e0981b24ecb4fa2b53cf54198ed458ed8cd262797`。
- [S9 — Piko archive.md](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/archive.md)：
  Piko commit `d61c1580783911be557d4c39026c300517187bcb`，AGPL 仓库中的设计文档，仅行为参考；
  blob `e8afe79dcec43b6b5b82c6200a9b431b0458548a`，SHA-256
  `b0e50f4926c001865b9ca443bbeeba97b58685cd00be2d8faa48a291204e4d61`。
- [S10 — pikpaktui files.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/files.rs)：
  Apache-2.0，commit `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`；
  blob `b64a91f084d96c314ae3f1afb11bb71bdda0ae80`，SHA-256
  `25e85d90267c30c0527ab2a8ccbe12773ce646dae6a848c76ae096dca170c2b2`。

执行过的校验：对 10 个源文件重新计算 Git blob SHA-1 与 SHA-256，全部与缓存清单/API blob 一致。
结果记录于 `target/file-protocol-audit/reference-archive-verification.json`。
本任务没有修改产品代码，也未运行上游测试；这些源码注释与测试均是协议研究证据，不是 NekoPak
的真实服务验收结果。
