# 上游功能对齐研究与不回退门禁

核查日期：2026-11-07。本文只记录行为、接口和验收事实，不复制上游实现。它把“最终功能不能少于上游”转成可检查的范围；Windows x64 是第一发布目标，Android/macOS/Linux 的平台差异必须单独记录，不能借“首版不承诺”悄悄删掉共享能力。

## 证据边界

主基线是 [Piko v1.4.0](https://github.com/NihilDigit/piko/tree/d61c1580783911be557d4c39026c300517187bcb)，commit `d61c1580783911be557d4c39026c300517187bcb`，发布标签 v1.4.0。核验了发布说明、README、选定 Kotlin/FFmpeg/下载/归档/播放/本地树源码和测试名称。Piko 代码是 AGPL-3.0-or-later；NekoPak 只做行为和接口研究，不复制其实现。

本次没有运行 Piko、SDK 或 Rclone 的真实账号测试，也没有执行其测试套件。“源码测试证据”仅表示发布提交包含相应断言，不能写成这些测试已由 NekoPak 团队跑通。下方建议的 NekoPak 测试和输出字段是独立设计的验收要求，均不代表现有实现已经具备。

第三方能力参考不是“必须等官方 CLI”：

- [pikpak-kotlin](https://github.com/NihilDigit/pikpak-kotlin/tree/8eaf5587a883ed4ea9bcca7b43a9194118719e63)，HEAD `8eaf5587a883ed4ea9bcca7b43a9194118719e63`，MIT。README 和 `RangeDownloadEndpoint.kt`、`TransferQuotaEndpoint.kt` 被核对。它提供随机偏移读取、共享块缓存、磁力哈希/秒传、范围下载、转移配额模型等行为参考。
- [pikpaktui](https://github.com/Bengerthelorf/pikpaktui/tree/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c)，HEAD `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`，Apache-2.0。Cargo manifest/README 被核对；它是 29 个 CLI 子命令的 Rust 社区客户端，覆盖浏览、下载、上传、分享、更新、JSON 和 dry-run。当前没有复制源码。
- [Rclone PikPak](https://github.com/rclone/rclone/tree/82cb84b2014bec6a167dc28eb53233b2d4abe1b3)，HEAD `82cb84b2014bec6a167dc28eb53233b2d4abe1b3`，MIT。官方文档和 `backend/pikpak` 源码被核对；它覆盖 Range 下载、MD5、上传、复制、移动、回收站、离线 URL、解压等行为。
- [PikPak 官方 CLI](https://mypikpak.com/en-US/cli)，已单独完成 v0.6.0 Windows x64 无账号探测，见 `docs/CLI_ADAPTER.md`；它是可选外部适配器，不是功能上限。

“文档/源码已核验”不等于真实账号成功，也不等于 PikPak 承诺稳定公开 API。Piko、pikpak-kotlin、pikpaktui、Rclone 可能使用服务私有或社区兼容协议；接入前要核验服务条款、账号授权、版本兼容和许可证。禁止内置别人的客户端密钥、验证码绕过、设备伪造或隐藏风控逻辑。任何实际复用代码仍需记录原项目、commit、路径、许可证和改动，必要时按上游许可证重新评估主仓许可证。

## 最高优先级：视频指定片段下载

Piko 已发布的片段功能不是“按文件比例下载一段字节”。它先拿到媒体时长和可读流，再根据用户毫秒区间做媒体层截取；NekoPak 必须保留这些语义。

### 用户行为和输入

来源：[片段选择 UI](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/SegmentDownloadSheet.kt)、[v1.1.0](https://github.com/NihilDigit/piko/releases/tag/v1.1.0)、[v1.2.0](https://github.com/NihilDigit/piko/releases/tag/v1.2.0) 的发布说明。

- 入口名义应是“下载指定段落/视频片段”，用户选择起点与终点，最短区间由 UI/核心统一校验（Piko 的实现使用最小片段时长约束）。结束必须晚于开始；无时长、无可读视频或不可获取表示时要显示明确错误。
- 打开面板时可带入信息流的初始区间；默认区间不是假定整片，而是读取成功后从片头起一个短区间。收起、返回、遮罩退出时保留编辑状态；已改区间需要放弃确认。
- 可选择原画或已经存在的转码画质。批量任务应一次选择画质；未知/读不出的画质必须禁用，不能悄悄改成“按设置挑”。
- 输出名称、估算大小、起止时间、总时长、预计占用空间必须来自后端实际媒体信息；没有可靠值就显示未知，不编造。

### 源读取、容器和关键帧

来源：[桌面截取](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloader.kt)、[转封装器](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/FfmpegRemuxer.kt)、[下载缓存文档](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/downloads.md)。

- Piko 的桌面实现通过自定义随机读取源接给 FFmpeg；读取缺的块才向云端请求，已读取块留在稀疏缓存。原画播放、范围读取和整文件下载共用按账号、内容标识（GCID/大小/表示）隔离的缓存；片段缓存另设可淘汰空间。
- 支持 FFmpeg 能识别的容器，至少验证 MP4、MKV、MPEG-TS。TS 的索引和时间戳问题要有专门回归：不能把 TS 当作 MP4/MKV 的精确 seek。
- 片段起点对齐到用户请求时间之前（含）的最近视频关键帧，然后从该关键帧开始流复制。最终画面可能早于用户选点，这是流复制的可解释结果；界面应提示“起点按前一关键帧对齐”。
- 终点按解码/时间戳窗口截取，保留完成解码所需的前导帧，不能因越过终点几帧就造成花屏；音频在同一时间窗内裁切。PTS/DTS 必须重置到输出起点并保持每条流递增。
- 输出容器是 fast-start MP4；HEVC 在 MP4 中写成播放器更兼容的 `hvc1` 标记。源视频编码无法封装进 MP4、没有可读画面或 FFmpeg 版本/布局不匹配时失败，不交付“只有声音”或损坏文件。
- 原实现保留 MP4 可接受的视频和音频轨道、语言/标题等流元数据；外挂/内封字幕不随无损 MP4 流复制。UI 必须说明“片段下载可能不含字幕/部分音轨”；若未来提供字幕转码，单独声明转码与许可，不把它冒充无损。
- “转码画质自动转 MP4”与“原画片段流复制”是两条路径，不能混为一个下载任务类型；任务记录要保存 source representation、时间区间、对齐后的起点、输出容器和是否转码。

### 文件安全、取消和恢复

来源：[桌面片段回归测试](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopTest/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloaderTest.kt) 和下载缓存文档。上游在不支持原子移动时有普通替换回退；NekoPak 若未验证该回退的安全性，应保持旧目标并报错，而不能先删除旧目标。恢复下载块缓存与恢复半成品转封装是不同能力，不能因前者存在就宣称转封装可直接断点续写。

- 上游片段先写目标旁 `.part`，输出成功且非空后尝试原子替换，运行时失败和协程取消会清理 `.part`。NekoPak 还需自行验证进程被杀后的清理/恢复：只能清理本次任务所属的 `.part`，并保持旧目标。不能把协程取消测试说成已证明断电清理。
- 取消必须传入底层读取/FFmpeg 工作，阻塞的随机读取也要终止；不能只停止 UI 进度。进度至少从读取字节/源时长提供单调值，完成必须是 1.0。
- 远程源需要 `read_range`/随机访问能力和可校验的缓存块；若后端只有整文件 URL，不能伪称已支持“云端指定片段”，可先允许本地文件片段或明确禁用远端入口。
- 不得按文件总字节线性估算片段起止偏移来替代媒体定位；这会在可变码率、关键帧稀疏、TS 时间戳和多轨文件上产生错误。

### 片段验收矩阵

Windows x64 首先必须有可重复的小型媒体样本和以下测试：

1. MP4/H.264+AAC：请求 5.0–8.5 秒，验证输出有 `ftyp`、`moov` 在 `mdat` 前，轨道可播放，时长和关键帧窗口在允许误差内。
2. MKV/H.264+AAC：使用随机读取源，确认读过的块数量大于零、没有整片预下载、起点落在前一关键帧。
3. MPEG-TS/HEVC+AAC：验证首帧无花屏、时长不漂移、seek 窗口经过 TS 特殊回归；输出失败要有可解释错误。
4. 多音轨/语言标签：保留 MP4 可接受音轨和语言标签；字幕缺失必须显示警告。
5. 不支持编码/无视频轨/损坏源：任务失败，不生成可见目标或残留 `.part`。
6. 取消时卡在随机读取：任务收到取消，底层读取退出，`.part` 清理，旧目标不变。
7. 目标不可替换（目录占用、权限、跨卷）：失败且保留原目标，不留下半截文件。
8. 重启恢复：`.part`/块位图/缓存版本不匹配时重取受影响块；不把未知块当已完成。
9. 原画与转码画质：两种表示分别缓存、记录、计费和验收；不互相覆盖。
10. 远端无 `read_range`：能力报告为 false，远端片段按钮禁用；本地片段仍可用（若本地后端实现）。

上游源码和测试证据：Piko `shared/.../PikoSegmentDownloader.kt`、`desktopApp/.../DesktopPikoSegmentDownloader.kt`、`desktopApp/.../media/FfmpegRemuxer.kt`、`DesktopPikoSegmentDownloaderTest.kt`，均来自 pinned commit；这些路径仅是行为证据，不是复制许可。

## 其他已发布能力清单

下面是 Piko v1.4.0/较早版本发布说明与源码明确出现的能力。每项都必须进入 NekoPak 的功能矩阵：实现、适配器支持、平台限制和禁用原因要可见；不能因为先做普通文件列表就算“功能已对齐”。

### 文件浏览、索引和整理

来源：[v1.4.0 发布说明](https://github.com/NihilDigit/piko/releases/tag/v1.4.0)、[v1.3.0](https://github.com/NihilDigit/piko/releases/tag/v1.3.0)、[v1.2.0](https://github.com/NihilDigit/piko/releases/tag/v1.2.0)、[v1.1.0](https://github.com/NihilDigit/piko/releases/tag/v1.1.0)，及 pinned [压缩包浏览文档](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/archive-browsing.md)。

- 本地持久化完整目录树，增量同步/双缓冲重建，目录导航和“在网盘中显示”路径定位。
- 全盘搜索（含归档条目）、目录内筛选、模糊匹配（简繁中文、日文汉字/假名和分隔符归一化）、按文件夹记忆排序。
- 目录图/路径补全、标签页/工作区、历史、最近位置、快速访问、列表/网格/图库视图和虚拟化大目录。
- 新建目录、重命名（批量、正则、序号、前后缀、冲突预览）、移动、复制、星标/取消星标、回收站删除与恢复、撤销/结果核对。
- 查找重复：跨目录按内容标识/大小分组，先预览结果再操作，避免把同名误判为重复。
- 压缩包作为只读虚拟文件夹浏览；密码尝试/记忆、包内文件预览/下载、选择解压/全部解压；分卷包显示不可浏览并提供下载路径。
- 分享链接：解析、预览目录、选择文件后保存/转存，保存前可播放视频、查看图片、浏览压缩包；大型分享子目录完整分页。
- 直接下载分享文件/已解析条目：关闭添加链接面板后任务继续，暂停、重启续传、未完成媒体预览，任务来源持久化。

### 下载、上传和云任务

来源：v1.2.0–v1.4.0 发布说明和 pinned [downloads.md](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/downloads.md)；上传/哈希与协议能力另由 [pikpak-kotlin README](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/README.md) 支持。具体上传暂停和跨进程恢复仍需单独验证 Piko 的用户入口，不由 SDK 能力直接推断 UI 覆盖。

- 多连接并发、连接/块级重试、Range 验证、首尾预取、稀疏块位图、CRC/内容身份、`.part` 和断电恢复。
- 完整文件和文件夹批量下载，保持目录结构、清理名称冲突并编号；批量任务共享调度空位，避免整棵目录扫描阻塞入队。
- 下载画质选择、原画/转码表示分开、蜗牛/带宽模式运行时切换，进度和共享缓存只统计一次网络字节。
- 磁力、种子、URL、分享链接解析；解析预览、按文件选择、云下载、直接秒传/引用保存；任务进度、取消、重试、失败原因和配额展示。
- 可恢复分片上传、内容 hash/秒传、上传目录和并发块限制；上传完成前后处理状态必须可见。
- 传输任务按需分类筛选（需要处理/进行中/完成等），同一内容/请求去重，跨重启持久化来源和缓存保留标志。

### 归档、引用和空间管理

来源：v1.2.0/v1.4.0 发布说明及 pinned [archive.md](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/archive.md)。这是网盘引用清单/内容取回功能，区别于 ZIP/RAR 压缩包浏览。

- 文件或文件夹归档为不占空间的记录；先写清单并确认成功，再将原文件移回收站/按明确选项永久删除；默认可取消处置。
- 归档清单保存文件名、大小、内容身份、来源、可恢复 ID 和封面信息；同目录合并清单，避免旁路来源文件。
- 按需取回/取消取回：优先回收站同目录同名同内容恢复，否则秒传；检查所需空间；失败条目保留并可撤销。
- 归档条目支持星标、搜索、缩略图、路径定位；旧清单和改名/移动后的内容身份可恢复。
- 归档并发、CID 取样、50 MiB 门槛等属于调度策略，必须有预算、超时和不处置安全规则。

### 播放、媒体和本地文件

来源：v1.1.0–v1.4.0 发布说明，以及 pinned [播放列表](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/player/PlayerPlaylist.kt) 与 [轨道显示](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/player/MediaTracks.kt) 源码。

- 原画/转码流播放、按偏移 seek、预取、并发连接调度、冷文件唤醒提示；播放器保持画质/倍速/音轨/字幕选择。
- 外挂/网盘字幕匹配，字幕选择器和无法匹配时的视频可用性提示；外部播放器（Windows/Android；Mac 具备时）走用户授权的直链。
- 信息流/Clip Feed：从当前文件夹或作品中选择片段、去重、恢复位置、自动顺延/自动连播开关、垂直翻页；其片段播放与“下载指定段落”共享媒体时间语义。
- 播放列表按作品/集数/版本整理，原名与规范名都可搜索；媒体轨道和语言显示不暴露私有 URL。
- 本地文件夹只读挂载到当前云端目录；挂载仅保存在本机，取消不改本地文件；本地视频/图片可在同一浏览、图库、信息流播放并自动匹配同目录外挂字幕。该能力与 WinFsp 独立盘符不是一回事。
- 图片图库/瀑布流：视频和图片切换、缩略图、时长角标、完整图片查看；预取和缓存有上限。

### 账号、配额、同步和发布

来源：v1.1.0–v1.4.0 发布说明；转移配额细分来自社区 SDK 的 [TransferQuotaEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/TransferQuotaEndpoint.kt)，不能据此断言 Piko 的每个 UI 页面已展示所有额度维度。

- 多账号切换；NekoPak 的额外安全要求是凭据由操作系统安全存储、切换时取消/隔离后台加载和缓存，不能串号。本次没有独立核验 Piko 凭据存储采用何种系统保护，不把本项目安全要求写成上游实测事实。
- 账号状态、会员、存储空间、传输额度（账户额度和 Connected Apps 单独），配额耗尽后的任务解释。
- NekoPak 自有补充要求：单向/双向文件同步、冲突副本、删除确认、空响应/失败保护、同步日志。本次没有证据证明 Piko 已有完整的文件同步引擎；不要将“偏好/星标跨设备同步”误写为完整双向文件同步。
- 设置：语言（简体/繁体/韩语/日语/英语）、减少动画、代理/兼容性范围、下载目录、蜗牛模式、默认播放器、更新检查、托盘/开机启动等；敏感设置不回传明文。
- 应用更新、便携模式数据目录、Windows x64/ARM64、Linux/macOS/Android 的平台支持是发布能力，不应由 Windows x64 第一阶段伪装成全平台已支持。

## 社区/第三方接口能力与法律边界

NekoPak 可以参考或在审计后实现社区兼容适配器；官方 CLI 不是功能天花板。可行的能力来源分层如下：

1. **官方 CLI adapter**：浏览器/设备授权，机器可读 JSON，当前只读预览；不随包分发，不自动安装/更新。
2. **pikpak-kotlin 行为/协议参考**：Range 读取、块缓存、磁力哈希、秒传、传输配额、重认证和错误/重试语义；MIT 允许在满足条件后复用代码，但必须先做 Rust 化、秘密/验证码审计和端点版本探测。README 明确其协议参考了 `52funny/pikpakcli` 和 `PikPak_Enhancement_Master`，这不是官方 API 保证。
3. **pikpaktui Rust adapter 研究/可选实现**：Apache-2.0；可研究其 JSON、命令、分页、上传/下载/分享/解压行为。不要把整个 TUI 或其依赖树嵌入 Lite；实际抽取要记录 commit/path/改动。
4. **Rclone/WebDAV adapter**：Rclone PikPak 文档和源码明确了 MD5、Range、上传分块、copy/move/trash、offline `addurl` 和 `decompress`。可作为用户自行安装的外部程序/可选 adapter，不捆绑；WebDAV 需 Premium/Connected Apps 权限。

社区实现可能依赖私有端点、客户端 ID、captcha 签名、设备 ID或账号/密码模式。NekoPak 不复制秘密、不伪造设备、不绕过 CAPTCHA/风控；凭据只走用户明确授权的安全存储/外部工具。每个 adapter 必须做：版本探测、协议/JSON schema fixture、Range/分页/错误/配额测试、明文日志脱敏、ToS/许可证复核、能力声明和可撤销禁用。未知能力要显示“未验证”，不是自动降级成另一个会重复计费的后端。

## 未发布目标与旧文档的处理

[Piko issue #10](https://github.com/NihilDigit/piko/issues/10) 是持续更新的 roadmap，不是 v1.4.0 的完整功能证据。该 issue 仍提到“改进片段截取，支持 MKV”，但 pinned v1.4.0 已包含 MKV 截取测试；不能仅凭旧 roadmap 将 MKV 标成没有实现，也不能因测试存在就宣称已在本机验证。

发布 tag 内的 `release-notes-draft.md`、`unreleased-work.md` 也是历史开发记录：例如 draft 中的多账号、归档、画质选择已经出现在 v1.2.0 正式发布说明，因此本清单以正式 release + pinned source 为准。响度归一化、Windows HTTPS 分享链接系统拦截等只有 WIP/未实现说明的事项暂记候选，不伪装成 v1.4.0 已发布承诺。未来基线升级时逐项复核，不能让不断变化的 main/roadmap 无界扩张当前验收。

## 有限且可执行的“不少于上游”门禁

每个发布候选建立 `parity-manifest.json`（按 `capability -> behavior -> provider -> platform -> test -> evidence` 记录），初始范围固定为：

- **P0/core**：授权状态、目录/搜索/筛选/元数据、文件整理与撤销、Range/完整下载、云任务、任务恢复、配额/错误。
- **P0/media**：播放/seek、指定片段（本地 + 远端随机读取）、MP4/MKV/TS、关键帧/音轨/字幕警告、取消/原子交付。
- **P1/organize**：分享预览/选择保存、压缩包浏览/解压、归档/取回、批量重命名/重复报告、图库/Clip Feed、本地文件夹只读挂载。
- **P1/sync**：单向/双向同步、冲突、删除保护、日志和离线恢复。
- **P2/platform**：WinFsp/Cloud Files 分离、Linux FUSE、Android/iOS/macOS 能力矩阵、语言/辅助功能/更新。

门禁规则：

- 任何 Piko v1.4.0 已发布能力若在 NekoPak 尚未实现，必须在能力报告和 UI 中写明 `planned/unsupported/unverified`，不能出现可点击假按钮。
- 可以继续发布明确标为开发预览的阶段产物，但在声明“Windows 功能对齐版/上游功能不回退”之前，manifest 中属于 Windows x64 的所有已发布上游必需行为都必须通过。P0/P1/P2 只决定实现顺序，不能把 P1 自动排除出最终对齐门禁；所有非适用项需要具体平台/权限理由或用户明确批准，不能仅写“为了体积暂不做”。
- `official_cli`、`pikpak-kotlin`、`pikpaktui`、Rclone/WebDAV 每个 adapter 单独列能力；一个 adapter 不支持不代表整个产品永久不支持，但也不能无证据宣称支持。
- 任何真实服务行为需记录账号类型、日期、后端版本、请求能力、脱敏日志和配额影响；没有真实账号时只能用协议 fixture/Mock，不得把合成成功写成服务成功。
- 片段验收必须同时有本地媒体测试和远端随机读取测试；缺任一项，远端“指定片段下载”保持 disabled。
- 许可证/来源扫描、依赖体积、外部运行时和平台测试与功能清单一起作为发布证据。复用 AGPL 源码会触发主仓许可证和源码提供义务，不能用 MIT 包装掩盖。

## 当前 NekoPak 差距（研究后确认）

研究启动时仓库已有 Mock 任务恢复、Tauri IPC、CLI v0.6.0 无账号只读探测和结构化契约；同期新增本地 MP4/MKV 实验核心，当前状态见 MEDIA_SEGMENTS.md。真实远端字节下载、Range 缓存、完整片段任务、播放、字幕、分享预览、归档、压缩包、搜索/目录树、上传、同步或本地文件夹挂载仍未完成。因此当前版本不能宣称与 Piko 功能对齐。Android 不列首版承诺，但共享 Rust/domain 与能力矩阵不能把移动端能力从长期目标清单中删掉。

## 直接来源索引

- Piko README/release：<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/README.md>；<https://github.com/NihilDigit/piko/releases/tag/v1.4.0>
- Piko 片段证据：<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/download/PikoSegmentDownloader.kt>；<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloader.kt>；<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/FfmpegRemuxer.kt>
- Piko 片段测试证据：<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopTest/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloaderTest.kt>
- Piko v1.3/v1.2/v1.1 发布说明（下载指定段落、画质、分享预览、归档、压缩包、目录图、字幕、外部播放器）：<https://github.com/NihilDigit/piko/releases/tag/v1.3.0>；<https://github.com/NihilDigit/piko/releases/tag/v1.2.0>；<https://github.com/NihilDigit/piko/releases/tag/v1.1.0>
- Piko 下载缓存/归档/目录树设计：<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/downloads.md>；<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/archive.md>；<https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/docs/development/local-drive-tree.md>
- pikpak-kotlin README/Range/Quota：<https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/README.md>；<https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/RangeDownloadEndpoint.kt>；<https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/TransferQuotaEndpoint.kt>
- pikpaktui manifest/README：<https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/Cargo.toml>；<https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/README.md>
- Rclone PikPak docs/source/license：<https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/docs/content/pikpak.md>；<https://github.com/rclone/rclone/tree/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak>；<https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/COPYING>
- Official CLI probe and limitations: `docs/CLI_ADAPTER.md`, `docs/evidence/official-cli-v0.6.0-probe.json`, `docs/evidence/official-cli-adapter-probe.json`。

本文结论：NekoPak 可以使用第三方 API/兼容层补齐上游能力，但必须以独立 adapter、明确授权、协议 fixture、能力检测、许可证记录和真实验收为前提。官方 CLI 当前只读预览只是一个接入点；它不降低“最终功能不少于上游”的产品要求。
