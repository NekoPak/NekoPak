# Piko 下载与桌面交互参考

核验日期：2026-11-08。针对下载目录选择、速度、任务文件定位、侧栏与进度配色，以及网络兼容、语言、媒体、托盘、自启、云下载和分享的实际发布范围。本记录是行为参考，不是 NekoPak 功能完成报告，也不替代完整的 [上游对齐基线](UPSTREAM_PARITY.md)。

## 版本、来源与验证范围

- [GitHub latest release API](https://api.github.com/repos/NihilDigit/piko/releases/latest) 本次返回 [v1.4.0](https://github.com/NihilDigit/piko/releases/tag/v1.4.0)，发布时间 `2026-10-31T15:48:52Z`，`draft=false`、`prerelease=false`。
- [标签引用](https://api.github.com/repos/NihilDigit/piko/git/ref/tags/v1.4.0) 指向 annotated tag `68238283e29b41ed618475ec79d98b35debc16bc`；[标签对象](https://api.github.com/repos/NihilDigit/piko/git/tags/68238283e29b41ed618475ec79d98b35debc16bc) 解引用为 commit `d61c1580783911be557d4c39026c300517187bcb`，与仓库既有研究基线一致。
- 下文 Piko 发布源码链接全部固定到该 commit。[递归源码树 API](https://api.github.com/repos/NihilDigit/piko/git/trees/d61c1580783911be557d4c39026c300517187bcb?recursive=1) 返回 `truncated=false`。初次重点读取 63 个文件；用户追问 Hosts 后，把关键词扫描扩至 851 个代码、资源、测试、配置、构建脚本或说明文本，均按 Git blob 算法重新计算 SHA-1，与该树中的 blob SHA 一致。main 差异和增强大师另注明提交，不混入发布基线。
- 没有运行 Piko 安装包、Piko 测试套件或真实账号操作；源码存在、上游注释中的实测记录，均不等于本次已经实测。NekoPak 的实现、协议 fixture、本机 UI 和用户账号验收应分别记录。
- Source-Project：NihilDigit/piko；License：AGPL-3.0-or-later；Reuse-Type：`behavior-only` / `design-reference`。没有复制 Kotlin 实现、组件、图标、截图或其他素材，没有修改上游文件。本次新增文件仅为本文；依据 [来源与许可证约定](04_SOURCES_LICENSES.md) 保留行为来源。

## 下载目录与下载前确认

Piko 设置中的“下载位置”展示当前路径，打开后可选默认位置或自定义位置。选自定义会弹目录选择器；已经使用自定义目录时再次点击仍可换目录。选择完成后设置对话框保留，让用户看到新路径。来源：[SettingsScreen.kt，目录设置与回调](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L203-L219)、[下载位置入口](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L519-L527)、[默认/自定义选择](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L709-L754)。

Windows 的实现是系统 `IFileOpenDialog` 目录模式（`FOS_PICKFOLDERS`、文件系统路径），模态关联到发起操作的窗口。在独立线程执行，选择成功才调用保存回调；取消与选择器不可用是不同结果，取消不会触发另一个选择器。来源：[DesktopPikoPlatform.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPlatform.kt#L208-L230)、[平台选择器分流](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPlatform.kt#L343-L353)、[winrt/FolderPicker.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/winrt/FolderPicker.kt#L24-L105)。

Piko 桌面的缺省路径为用户主目录下 `Downloads/Piko`，空配置代表恢复默认；这是 Piko 的具体实现，并非 Windows 下载已知文件夹一定在此路径的保证。NekoPak 应独立使用系统下载目录解析与持久化。来源：[DesktopSettingsStore.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopSettingsStore.kt#L41-L49)。

**普通下载没有每次询问本地目的目录的证据。** 固定发布版的普通入口按已保存的默认画质直接入队；用户显式选择“按画质下载”才显示画质对话框。该对话框中的“以后按此画质直接下载”保存的是画质。逐下载确认保存位置可作为满足本次用户反馈的 NekoPak 改进，但不能记成 Piko 已有的同名对话框。来源：[DownloadLauncher.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/DownloadLauncher.kt#L55-L64)、[发起、确认及入队](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/DownloadLauncher.kt#L96-L166)。

## 下载速度、进度与剩余时间

Piko 的下载引擎每 500 ms 采样一次，以单调时钟和约 3 秒窗口计算实际交付的数据字节增量。下载进度使用缓存中有效的已持有字节，速度使用网络交付字节；已有缓存和恢复时已有文件大小不会自动成为本次网络速率。多任务引用相同缓存时只为其中一个活跃任务记录速度，避免总速率重复累计。来源：[PikoDownloadCoordinator.kt，采样过程](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/download/PikoDownloadCoordinator.kt#L112-L140)、[采样常量](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/download/PikoDownloadCoordinator.kt#L1032-L1038)、[PikoFileCachePool.kt，计数来源](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/cache/PikoFileCachePool.kt#L155-L160)。

传输行的实际呈现如下，来源为 [LocalTransferRow.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/LocalTransferRow.kt#L84-L106) 和 [TaskPresenter.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/TaskPresenter.kt#L115-L119)：

- 普通下载显示已下载字节/总字节；下载中速率大于零才附加每秒速率。
- 已知剩余字节且速度有效时计算剩余时间；片段导出或转封装不套用普通文件 ETA。
- 片段定位阶段显示“正在定位起点”，有媒体进度后显示百分比，避免 `0 B / 0 B`。
- 未知总量采用不定进度；完成与失败不继续绘制进行中的进度条。

页面总下行速度只汇总本机正在下载的任务，总上行速度只汇总本机上传。源码明确云端离线任务没有可报告的本机速度，云下载的未知速率不能让本机下载统一显示“未提供”。底栏没有活动传输时可显示 `0 B/s`，采用固定最小宽度与等宽数字，减轻数字更新造成的抖动。来源：[TransfersState.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/TransfersState.kt#L382-L401)、[TransfersFooter.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/TransfersFooter.kt#L58-L92)。

NekoPak 应保持“数值为零”和“后端没有测量值”两种语义；采样归 Rust Core 所有，前端接收快照和事件。切换页面不能通过重新计算起点制造虚假速度，暂停/完成后不能继续显示上次的活动速率。这些是独立验收要求，不是对现有实现通过的声明。

## 打开下载所在文件夹

Piko 根据任务状态提供真实的本机动作，路径来自任务记录的 `destinationPath`。完成的任务可打开所在文件夹；正在下载且路径非空时也可打开；暂停/等待时还要求已经下载字节大于零。来源：[LocalTransferRow.kt，动作和启用条件](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/LocalTransferRow.kt#L111-L117)、[状态菜单](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/LocalTransferRow.kt#L178-L197)。

Windows 实现调用系统 Shell 定位文件，文件已不存在时改为打开父目录。上游特意避免把 `/select,路径` 拼成易损坏空格路径的命令行。来源：[DesktopPikoPlatform.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPlatform.kt#L193-L199)、[WinRTSupport.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/winrt/WinRTSupport.kt#L238-L263)。

NekoPak 的独立实现应解析已持久化任务的真实绝对目的地，再交给系统 Shell。改变全局下载目录后，历史任务仍应定位其原目录。缺少有效路径时给出明确状态；不能退到应用工作目录后宣称成功。优先使用任务 ID 发起受约束的本机定位命令，并覆盖中文、空格、括号和逗号文件名。

## 侧栏与进度颜色

Piko 侧栏用统一主题角色表示选择：选中行采用 `secondaryContainer` 背景和 `onSecondaryContainer` 文字/图标，并切换实心图标；未选中行保持普通表面文字。展开和收起状态保持相同的选择语义，区别于悬停反馈。来源：[QuickAccess.kt，展开侧栏项](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/drive/QuickAccess.kt#L162-L229)、[收起侧栏项](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/drive/QuickAccess.kt#L271-L286)。

任务进度组件使用主题的 Material 线性进度组件，没有为各页面硬编码不同的活动颜色。完成状态文字使用专门的成功色，失败使用错误色；进度和成功不共享一个含义混乱的绿色常量。来源：[WorkMeterBar.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/WorkMeterBar.kt#L10-L22)、[LocalTransferRow.kt，状态颜色](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/LocalTransferRow.kt#L120-L125)、[Color.kt，明暗成功色](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/theme/Color.kt#L6-L18)。

颜色是可切换主题的一部分，并非固定只用一种蓝色。桌面缺少动态取色时默认采用首个内置主题 RURI：亮色 `primary=#45608A`、`secondaryContainer=#D9E3F8`；暗色分别为 `#B3C7ED`、`#323C4D`。这里只记录事实，不要求复制其调色板。NekoPak 按 [UI 规范](07_UI_SPEC.md) 统一自身的选中、活动进度、成功、警告和错误令牌，并分别检查明暗主题的对比度。来源：[Appearance.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/theme/Appearance.kt#L48-L61)、[SeedColorSchemes.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/theme/SeedColorSchemes.kt#L10-L75)。

## 其他桌面能力的实际范围

**自定义代理已经发布。** 登录/设置复用的代理对话框提供跟随系统、不使用代理、手动 HTTP 或 SOCKS5，手动字段是地址和端口；端口校验为 1–65535，填完整才可保存。本次核对的设置模型没有代理账号密码字段。界面明示对新建连接生效，视频直链和转码流不经代理；不能由此宣传“全部流量被代理”。实现修改应用进程的连接选择，不修改 Windows 系统代理或 Hosts。来源：[ProxySettingsDialog.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/ProxySettingsDialog.kt#L22-L78)、[ProxySetting.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/net/ProxySetting.kt#L8-L29)、[PikoProxySelector.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/jvmSharedMain/kotlin/dev/piko/shared/net/PikoProxySelector.kt#L11-L55)。

**托盘和 Windows 自启已经发布。** 支持系统托盘的平台提供关闭到托盘开关，默认关闭。存在后台工作或待恢复上下文时关窗保留后台会话；托盘提供显示主窗口和立即退出。自启仅在 Windows 打包启动器路径存在时提供，通过当前用户的 Run 注册项登记应用，可查询、开启和关闭；开发环境的 Java 进程不进入自启。它表示登录 Windows 后打开应用，不意味着已经支持“启动即隐藏到托盘”。来源：[DesktopPikoPlatform.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPlatform.kt#L96-L101)、[Main.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/Main.kt#L259-L339)、[WindowsLaunchAtStartup.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/winrt/WindowsLaunchAtStartup.kt#L7-L27)、[设置入口及失败反馈](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L347-L380)。

**云下载已经发布。** 提交指定链接到云端目录后，可能取得排队任务或已完成文件；发布源码对返回 ID、阶段和目标目录进行核对。请求发出后超时/取消等不确定结果保留为待确认，拦截同账号、同目录、同链接的重复提交。任务查询包括等待、运行、完成、错误；失败任务由原链接重新提交，成功后清理旧任务记录。删除/清理任务使用保留已产出文件的语义。云端进度来自服务阶段/百分比，不能冒充本机已写字节。来源：[OfflineSubmitSession.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/OfflineSubmitSession.kt#L64-L122)、[TaskRepository.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/data/TaskRepository.kt#L36-L109)、[云端进度显示](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/transfers/TaskPresenter.kt#L80-L86)。上游注释称其服务端 RETRY 路径实测不可靠，这只是上游记录，NekoPak 仍需独立核验，不能盲目复制服务调用或自动重试。

**网盘分享创建和管理已经发布。** 可选无提取码、随机码、自定义码；自定义码为 4–10 位 ASCII 字母数字；有效期为永久、7、14、30 天。默认随机码。创建成功后展示链接和提取码，避免在同一结果页继续创建另一条链接。“我的分享”支持完整分页、预览、单项/批量取消，失败有反馈。来源：[ShareCreateState.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/ShareCreateState.kt#L16-L94)、[ShareDialog.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/share/ShareDialog.kt#L52-L64)、[MySharesState.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/MySharesState.kt#L66-L117)、[取消分享](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/MySharesState.kt#L143-L185)。

桌面 `localFiles.canShare=false` 仅表示没有实现操作系统的本地文件分享面板，不能用它证明网盘分享不可用。来源：[DesktopPikoPlatform.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPlatform.kt#L202-L205)。

**已发布的同步和挂载应按具体含义比较。** Piko 有账号偏好/归档等数据跨设备同步，以及把本地目录只读映射到应用内云盘目录的能力；本地映射记录仅在本机，解除映射不删除本地内容。偏好同步明确排除下载目录和代理等设备专属配置。本次没有发现其已发布完整双向文件夹同步、Windows Cloud Files 同步根或 WinFsp 盘符的证据；NekoPak 自有的这些目标仍保留，分别实现和验收。来源：[v1.4.0 发布说明](https://github.com/NihilDigit/piko/releases/tag/v1.4.0)、[LocalMounts.kt，只读访问接口](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/data/LocalMounts.kt#L74-L81)、[登记与解除映射](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/data/LocalMounts.kt#L128-L168)、[PikoSettingsSync.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/sync/PikoSettingsSync.kt#L33-L35)、[偏好同步语义](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/sync/PikoSettingsSync.kt#L88-L95)。

## “网络增强”的具体含义

本次核对的 Piko v1.4.0 设置没有一个笼统命名为“网络增强”的入口，实际是“网络代理”“服务器域名”，以及下载设置中的“并发加速”。并发加速描述为多连接下载，不表示解除会员、配额或服务端限制。来源：[SettingsScreen.kt，网络设置](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L563-L578)、[并发开关](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L535-L540)。

服务器域名可自动选择或固定选择，界面列出最近一次请求延迟，并可手动重测。Piko 应用层在登录后调用 SDK 的域名探测，比较连接复用后的一次请求，自动模式仅在候选明显更快时切换，阈值为当前延迟的 0.8。固定选择会先切换再测供显示；延迟结果不写入磁盘，选择本身持久化。来源：[PikPakDomainDialog.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/PikPakDomainDialog.kt#L18-L73)、[PikPakDomainSelector.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/net/PikPakDomainSelector.kt#L21-L29)、[探测与选择逻辑](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/net/PikPakDomainSelector.kt#L71-L108)、[DesktopPikoPreferences.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPreferences.kt#L302-L305)。

上游说明 SDK 枚举四个根域名，并声称它们可共享账号令牌；同一说明又明确密码登录仍走默认域名，其他根域名密码登录没有实测。这不能替代 NekoPak 对端点归属、服务授权、认证范围、TLS 与实际协议的独立核验，也不能据此把凭据发送到任意可填写的主机。未取得逐地区测试证据，不能宣传某国家或运营商“保证可用”。这项研究没有执行任何带账号的域名测速。

本次核对的应用层代理、域名类和设置入口没有提供修改系统 Hosts、关闭证书校验或绕过验证码的开关；SDK 内部认证仍是单独审计范围，不能由此给整个上游协议作安全背书。NekoPak 若增加“网络兼容”开关，应默认关闭，清楚说明只影响本应用哪些请求，实际连通性失败时保留错误；不隐蔽修改全局系统设置，不把认证拒绝伪装成可用节点，不静默重试有副作用的请求。这遵循 NekoPak 的 [安全约束](06_SECURITY_AND_COMPAT.md)，是独立实现边界。

### 用户追问：Hosts 屏蔽与 Hosts 映射

**Piko 自身编辑/管理 Hosts 的实现仍未确认；用户随后提供的四条 `access.*` 规则已能确认用途，见下一节。** 不能用代理或服务器域名选择代替 Hosts 证据。屏蔽特定域名、为域名指定地址、重写下载 URL，是三个不同的动作。前两者也可能分别作用于系统文件或应用自己的网络解析层；本次没有修改网络设置，也没有读取本机 Hosts。

Piko v1.4.0 补查范围为：45 份 Markdown（含 README 与开发文档）及 806 个代码、资源、配置、构建脚本文本，共 851 个文件；搜索 `hosts`、系统 Hosts 路径、Host map/override/blocklist、adblock、DNS 覆盖及相关解析器调用后，未找到域名屏蔽清单、固定 IP 映射表、Hosts 编辑器或应用内自定义 DNS resolver 的实现。直接出现的 `hosts` 变量是界面宿主计数、文件附件所属映射；`Dns` 命中是设置页图标，均不是网络规则。来源：[固定提交完整树](https://github.com/NihilDigit/piko/tree/d61c1580783911be557d4c39026c300517187bcb)、[WindowFrame.kt，宿主计数](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/WindowFrame.kt#L253-L273)、[DriveScreenState.kt，附件所属关系](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/state/DriveScreenState.kt#L450-L481)。该扫描不包含媒体样本、图片或已打包第三方二进制的内部实现。

另检索最近 10 个正式 release 的说明，未发现 Hosts 功能说明；[v1.0.0](https://github.com/NihilDigit/piko/releases/tag/v1.0.0) 记录新增代理设置，[v1.1.0](https://github.com/NihilDigit/piko/releases/tag/v1.1.0) 记录自动选择服务器地址。检查了 main `d0bd8fe93573e8484c7db7616a41cbfcd29aa395`（提交时间 `2026-11-02T05:52:56Z`）相对稳定版的 10 个提交、200 个文件的[差异](https://github.com/NihilDigit/piko/compare/d61c1580783911be557d4c39026c300517187bcb...d0bd8fe93573e8484c7db7616a41cbfcd29aa395)，未检索到新增 Hosts 相关选项；这不等于已经审计所有历史版本或 SDK 内部。

补充核对既有参考项目 **PikPak Enhancement Master**，固定 commit `024b31b30e4c797258957862eb64fc2fa364bda9`，主脚本标记版本 `5.3.0`、许可 `AGPL-3.0-or-later`。其 [README 第 164 行](https://github.com/digbug82/PikPak_Enhancement_Master/blob/024b31b30e4c797258957862eb64fc2fa364bda9/README.md#L161-L165) 明确把“下载直链加速域名”描述为反向代理改写，提供拼接原链接、URL 参数、自定义模板三种方式。主脚本仅对 `dl-[a-z0-9-]+.mypikpak.com` 形式的下载主机应用该改写；加速地址由用户填写，默认空，开关默认关闭，另有浏览器下载、外部下载器、播放器、M3U 等适用范围。这是构造新的下载 URL，已核对的函数没有写系统 Hosts，也没有创建应用级 DNS 映射或固定屏蔽域名/IP 表。来源：[配置默认值](https://github.com/digbug82/PikPak_Enhancement_Master/blob/024b31b30e4c797258957862eb64fc2fa364bda9/PikPak_Enhancement_Master.user.js#L219-L227)、[匹配主机与 URL 重写](https://github.com/digbug82/PikPak_Enhancement_Master/blob/024b31b30e4c797258957862eb64fc2fa364bda9/PikPak_Enhancement_Master.user.js#L11810-L11892)。

增强大师源码中的加载层屏蔽是网页 DOM/CSS 操作：隐藏初始加载或 `.pikpak-loading-container`，业务对话框另有保护判断；它不构成 Hosts 屏蔽的证据。来源：[加载层处理](https://github.com/digbug82/PikPak_Enhancement_Master/blob/024b31b30e4c797258957862eb64fc2fa364bda9/PikPak_Enhancement_Master.user.js#L94-L174)。README 和主脚本的 Git blob 分别为 `934c2375b943302932d0a3c97500fa10dbd469d5`、`24ab231540498dc34ec4cd0c447c2027ea891900`，两者校验一致；本次仅引用行为与来源，未复制脚本或采用其网络方案。

上述核查未找到 Piko 或增强大师提供 Hosts 编辑/管理界面的证据。用户截图给出的具体主机则可以追踪到官方网页和 SDK 说明；下节补上这些规则的用途，不再将它们概括为未识别域名。

### 截图中的四个 access 主机：地区可用性检查

用户截图把 `access.mypikpak.com`、`access.mypikpak.net`、`access.pikpak.me`、`access.pikpakdrive.com` 指向 `0.0.0.0`。这是让这些主机的连接失败的阻断规则，不能据此称为更快服务器的 IP 映射。其实际影响应由客户端调用与失败分支判断。

**官方网页中的明确调用。** 2026-11-08 读取的 [PikPak 网页入口](https://mypikpak.com/drive/) 加载 Web 构建 `v2.30.3-9-gcc540f8e`。[5492.76e680dd.js](https://mypikpak.com/drive/5492.76e680dd.js) 的模块 `18839` 定义 `https://access.<root>/access_controller/v1/area_accessible`，调用参数为 `withCredentials=false`、`withCaptcha=false`。同一模块还定义 `https://access.<当前root>/access_controller/v1/lbsInfo`。不是仅凭 `access` 这个名字推断用途。

[3765.9a7a52c0.js](https://mypikpak.com/drive/3765.9a7a52c0.js) 模块 `36460` 的四根域名集合与截图一致。[main.a17bccef.js](https://mypikpak.com/drive/main.a17bccef.js) 模块 `49805` 并发调用四个 `area_accessible`，检查解析结果中的 `accessible`，更新 `ipBlock`；主布局依据 `ipBlock` 是否存在选择限制界面或正常路由界面。路由观察器会触发该检查，但有缓存与特定路由例外，不能说每次路由变化必定重新发送四个请求。

**源码中的失败处理：** 任一成功响应的 `accessible` 为真，或四个请求全部被拒绝/失败时，函数记录 `pp_access_to_visit` 并返回空的阻断结果；没有可访问响应且存在 `accessible=false` 时，取该响应写入 `ipBlock`。普通检查与 5 秒超时竞速，超时返回 `undefined`，也会清除阻断状态。另一个强制刷新分支使用 10 秒超时并返回 `null`，保留原来的阻断状态。这些是该固定网页构建的客户端逻辑，不是服务端许可承诺，也不是对所有客户端版本的保证。

**不带凭据的公开响应核验。** 本轮只对上述已观察到的路径发出 GET，没有发送 Cookie、Authorization、账号、密码或验证码，也没有改变任何客户端状态。2026-11-08 约 09:03 UTC 的工具网络结果为：

- `access.mypikpak.com` 的 `area_accessible` 请求发生一次 `ECONNRESET`，没有取得应用响应；不据此断言该域名长期不可用。
- 其余三个主机返回 HTTP 200，`accessible=false`；顶层字段为 `accessible`、`ip`、`prompt`、`timeZone`，其中 `prompt` 包含 `title`、`body`、`redirect`。标题明确为 “Sorry,PikPak is not available in the current region”；正文说明根据请求 IP 判断服务地区。本文不记录 IP 或带 IP 的正文。
- 对 [access.mypikpak.net 的 lbsInfo](https://access.mypikpak.net/access_controller/v1/lbsInfo) 进行一次无凭据 GET，返回 HTTP 200，字段为 `countryCode`、`countryText`，可确认它提供国家/地区信息。主机级阻断也会影响这一接口；本轮未追完其所有页面消费者。

这组结果来自工具使用的网络出口，不代表用户账号、用户浏览器或所有地区的验收。没有执行 Hosts 阻断实验；四条规则若使网页请求全部失败，会进入上述失败分支，这是由代码推导的影响。

**Piko SDK 已明确记录但不调用它。** Piko v1.4.0 声明依赖 `pikpak-kotlin 2.5.0`。该 SDK 标签固定 commit 为 `9187fafbe807695893e565d5e2b273b871a1c7c0`，其 [PikPakDomain.kt 第 52–58 行](https://github.com/NihilDigit/pikpak-kotlin/blob/9187fafbe807695893e565d5e2b273b871a1c7c0/src/commonMain/kotlin/io/github/nihildigit/pikpak/PikPakDomain.kt#L52-L58) 直接把 `area_accessible` 称为地区检查，并声明 SDK 不调用它。作者还记录过账号 API/CDN 在其测试环境中的行为；这是上游测量，不能冒充 NekoPak 的真实账号测试。本轮读取该 SDK 的六份说明/端点文件，未复制实现。SDK 是 MIT 许可的独立参考来源。

**对当前 NekoPak 的作用限制。** 静态检索 `crates`、`src-tauri`、`src` 中四个主机、`area_accessible`、`access_controller`、`lbsInfo`，没有命中主动调用；`crates/nekopak-provider-native/src/http.rs` 的认证/网盘主域分别为 `user.mypikpak.com` 与 `api-drive.mypikpak.com`。这次是源码核对，没有运行带账号的抓包。因此没有证据说明为当前原生直连增加这四个主机的阻断能修复登录、验证码、文件 API 或下载速度，也不能从网页检查失败推导为账户权限、会员或配额得到放行。

已确认的规则作用是使地区可用性检查不能返回限制结论。本轮没有证实这是服务方认可的网络兼容方案，也没有证实其不影响服务访问控制；不据此推荐 NekoPak 增加“屏蔽地区检查”的开关。现有用户可控代理和可靠的请求处理应按各自已核验行为实现，不能用“网络增强”一词混淆这几件事。

官方 JavaScript 为公开分发的客户端行为证据，不视作可复用源码许可；本轮未复制到仓库。可复核的 SHA-256 为：

- `main.a17bccef.js`：`e945e8e14157f548b59f48a063f6f1a90942e43c232a03b4f754cc1740e0040f`，150,212 字节。地区检查位于模块 `49805`（文本偏移约 109,658）；根界面 `ipBlock` 分支位于文本偏移约 106,187。
- `5492.76e680dd.js`：`3343d4a37d421070c3f3b9b80c1d3c60e0f9e84ee2a6ffc75ca0198f1af5dba7`，62,580 字节。地区接口字符串位于文本偏移约 11,771，位置接口约 14,799。
- `3765.9a7a52c0.js`：`0914dc27fc415a5915a46dfda6b7a5d0cb8716bf066940b7bbaae41f16bdfedc`，140,702 字节。四根集合位于文本偏移约 122,100。偏移指 JavaScript 字符串位置，用于寻找压缩源码，精确身份以哈希为准。

## 语言选择、切换和持久化

Piko v1.4.0 的语言选项为跟随系统、简体中文、繁体中文、韩语、日语、英语；保存的标签分别为 `system`、`zh-CN`、`zh-TW`、`ko-KR`、`ja-JP`、`en`。设置中按语言本身的名称显示，选中即保存并关闭选择框。来源：[AppLanguage](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/data/auth/PikoUserPreferences.kt#L39-L72)、[SettingsScreen.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/screens/settings/SettingsScreen.kt#L174-L188)。

桌面偏好写入本机 `ui.language`，应用启动在首屏前恢复；运行期间监听该偏好，更新应用语言与进程使用的 Locale。系统繁中地区或 Hant 标签解析为繁中，Hans 解析为简中，其他未支持系统语言回落英语；无法识别的已保存语言值回落“跟随系统”。来源：[DesktopPikoPreferences.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoPreferences.kt#L35-L39)、[Main.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/Main.kt#L165-L169)、[PikoApp.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/PikoApp.kt#L44-L55)、[PikoLocale.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/i18n/PikoLocale.kt#L28-L55)。

独立实现可先完整覆盖 NekoPak 的简中与英语两种界面，再补其余语言；应把登录、验证恢复、文件菜单、任务状态、设置、错误和无障碍标签纳入同一字典，切换后即时更新并在重启后保留。只翻译导航或开关标题不能称为整套英语界面，两种语言也不等于已通过五语种对齐。原始文件名、路径和服务端返回内容不应被当作字典键翻译。

## 外部播放器、按画质下载和指定片段

**外部播放器读原画。** Piko 先建立本机 `127.0.0.1` 流会话，让播放器读取由应用管理的原画源；缺少内容身份无法建立会话时失败，不改交一个会过期的裸云端直链。外部会话与内置播放器分开保留，发布版最多保留两个，较早会话被淘汰后会失效。来源：[ExternalPlayerStreams.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/proxy/ExternalPlayerStreams.kt#L9-L49)。

Windows 按媒体文件扩展名读取用户的默认播放器“打开”关联，再启动该播放器传入地址；避免 `.ts` 被当作 TypeScript 打开，必要时尝试 `.mp4` 关联，找不到可用命令则报告失败。此行为需要用户机器上存在可用播放器，不能用默认浏览器打开 URL 冒充完成。来源：[WindowsExternalPlayer.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/winrt/WindowsExternalPlayer.kt#L8-L39)。既有研究中概括的“外部播放器直链”应按这一具体实现理解，不能推导为必须把签名云 URL 暴露给前端。

**按画质下载有明确表示语义。** 单视频查询原画和已存在转码，逐档探测大小与可读性；没有转码则只显示原画，未知大小如实显示，已判不可读的档保留并禁用。显式选中的转码失效时报错，不偷偷换成另一档。批量/文件夹选择的是分辨率上限，发布版规则为优先不高于上限的最高可读档，否则选最低可读档；原画选项只取原画。来源：[DownloadQualityDialog.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/DownloadQualityDialog.kt#L47-L80)、[单视频选择](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/DownloadQualityDialog.kt#L130-L154)、[DownloadQualityOrder.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/DownloadQualityOrder.kt#L5-L21)、[PikoMediaRepository.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/PikoMediaRepository.kt#L681-L704)、[显式选择错误语义](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/media/PikoMediaRepository.kt#L775-L808)。

普通原文件下载保持原文件表示；转码下载先取得该转码流，再本机转封装为 MP4。发布源码描述转码源为 MPEG-TS；原画与转码按不同表示缓存，不能仅替换文件扩展名假装完成。来源：[DownloadModels.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/download/DownloadModels.kt#L43-L62)、[PikoDownloadCoordinator.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/shared/src/commonMain/kotlin/dev/piko/shared/download/PikoDownloadCoordinator.kt#L810-L819)。

**指定片段使用媒体时间。** Piko 选择起止毫秒、最短 500 ms，默认从开头到第 60 秒或片尾，也可继承信息流当前片段；单个预览在起点/终点之间切换。估算大小按所选表示的大小与时长比例计算，仅标为估算，不能拿这个比例取代实际媒体定位。来源：[SegmentDownloadSheet.kt，区间与会话](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/SegmentDownloadSheet.kt#L89-L145)、[初始化范围](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/SegmentDownloadSheet.kt#L188-L215)、[估算与提交](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/SegmentDownloadSheet.kt#L351-L381)。

桌面抽取通过随机读取接口连接 FFmpeg 的媒体读取器；按起点之前的视频关键帧开始流复制，输出 moov 前置的 MP4，HEVC 使用 `hvc1` 标记。选择一条可读视频轨及 MP4 可接受的音轨，字幕不复制，不兼容的视频编码明确失败；这不是任意格式、所有轨道无损保留。源容器由 FFmpeg 支持范围决定，发布测试包含 MKV/TS 前关键帧截取、TS 转 MP4、取消、无效区间及旧目标保护。来源：[DesktopPikoSegmentDownloader.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloader.kt#L26-L81)、[FfmpegRemuxer.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/FfmpegRemuxer.kt#L138-L204)、[关键帧和时间窗](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/FfmpegRemuxer.kt#L207-L257)、[发布测试源码](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopTest/kotlin/dev/piko/desktop/DesktopPikoSegmentDownloaderTest.kt#L37-L180)。测试名称仅是源码证据，本次没有执行这些测试。

**校正旧研究的片段面板描述：** v1.4.0 关闭片段面板会结束会话；编辑过区间时先询问放弃，选择继续编辑则保留。被其他页面覆盖时保留状态与主动关闭是两个行为。[既有研究](UPSTREAM_PARITY_RESEARCH.md) 中“收起、返回、遮罩退出时保留编辑状态”的概括不符合本次核对的关闭逻辑，应以固定提交中的 `session.end()` 分支为准。来源：[SegmentDownloadSheet.kt](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/kotlin/dev/piko/ui/components/SegmentDownloadSheet.kt#L219-L230)、[实际退出提示资源](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/ui/src/commonMain/composeResources/values/strings.xml#L885-L889)。

**实际依赖不能省略。** Piko 桌面使用 Kotlin/Compose、JDK 工具链、MediaMP 0.5.0 及对应平台 mpv 运行库；FFmpeg 从该运行库目录加载，检查 avutil/avcodec/avformat/swscale 的主版本，按 FFmpeg 8.0.1 的公开结构布局接入。NekoPak 不应复制这些 AGPL 实现或把 Piko 整套运行时放进 Lite；若用可选媒体组件独立完成同样功能，必须有可操作的发现、版本/完整性检测与错误反馈。仅有一个已安装外部播放器不等于具备片段导出能力。来源：[desktopApp/build.gradle.kts](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/build.gradle.kts#L43-L78)、[版本清单](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/gradle/libs.versions.toml#L42-L44)、[Ffmpeg.kt，版本核验与加载](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/Ffmpeg.kt#L92-L153)、[布局版本依据](https://github.com/NihilDigit/piko/blob/d61c1580783911be557d4c39026c300517187bcb/desktopApp/src/desktopMain/kotlin/dev/piko/desktop/media/Ffmpeg.kt#L187-L188)。

## 可独立实现的 NekoPak 验收

以下是下一步验收条件，均未因本文而自动通过：

1. **真实目录选择：** 设置中的“浏览/选择文件夹”直接打开关联主窗口的系统目录选择器；选择后显示并保存绝对路径，重启保留；取消保持原值。使用中文、空格、不可写目录和已失效目录验证反馈，弹窗过程中 UI 保持响应。
2. **单次目的地：** 若按本次反馈增加下载确认框，展示文件/数量及目的地，提供系统目录选择按钮；确认后才入队，取消不创建任务。勾选“设为默认”才修改后续默认值；每个任务保存自己的目标，不能被之后的设置修改挪走。
3. **真实测量：** 用受控本地 HTTP 文件验证字节递增、测得速度、暂停归零、恢复重新采样、完成状态；页面和应用重启后从 Rust 快照恢复。已知零速率、正在连接、未知总量与服务不提供速率分别显示，云任务不计入本机网络总速率。
4. **本机定位：** 完成任务定位实际文件；尚未生成最终文件的任务打开已存在的目标目录；路径缺失或无效给出可见错误。修改默认目录后旧任务仍定位原目录。记录至少一次 Windows 资源管理器的真实动作验证，Mock 提示不算通过。
5. **颜色一致：** 侧栏当前页、悬停、键盘焦点可区分；正常下载进度使用统一活动色，完成/失败有自己的状态文字和颜色。亮暗主题、125%/150% 缩放及窄窗口截图均检查可读性。保留 NekoPak 自身的布局和设计令牌。
6. **桌面设置有效：** 自定义代理通过受控代理服务器验证实际路由、无效配置反馈和恢复；说明影响哪些请求。托盘/自启需验证真实 Windows 集成和设置读回；只改开关外观不算实现，测试结束清理由测试创建的自启项。
7. **云下载与分享：** 协议 fixture 覆盖真实响应形状、分页、权限/验证码、缺少 ID、失败和提交结果不确定。副作用请求不能自动盲重试。完成客户端配置与离线测试后，由用户在应用内做账户验收，不要求用户向聊天发送密码/令牌。
8. **范围与门禁：** Piko 已发布但 NekoPak 尚缺的代理、托盘、云下载、分享、视频指定片段等继续进入对齐清单；阶段预览可写明状态，但禁用标签不是完成实现。安全同步、Cloud Files、WinFsp 与本地只读映射分别验收，不能互相替代。
9. **可切换网络兼容：** 默认关闭，说明代理/连接兼容的具体范围，开关与配置持久化；使用受控服务验证开关确实改变本应用的新请求。保持 TLS、认证和配额语义；未经核验不发送凭据到其他域名，不改系统 Hosts，不声称某地区一定可用。
10. **可用的语言系统：** 至少简中/英语完成主要用户流程全覆盖，切换即时生效，重启首屏正确；错误提示、验证流程和辅助标签随语言更新。未知保存值安全回落，系统语言解析可测；其余上游语言继续记录为差距。
11. **外部播放器：** 实际发现用户播放器、打开授权媒体并支持 seek；无播放器、关联错误、来源缺失有反馈，不以打开浏览器代替。流会话有账号隔离和可解释生命周期，云端令牌不进入日志。
12. **画质与片段：** 显示真实表示和可读性，显式档位不静默替换；原画/转码缓存和输出区分。远端片段验证随机读取、MP4/MKV/TS、关键帧、音轨/字幕损失说明、取消及旧目标保护；未完成的格式/来源明确禁用，不能用按字节比例切割或本地样片成功冒充云端媒体功能完整。

## 复核方法

本次只读请求 `releases/latest`、`git/ref/tags/v1.4.0`、annotated tag 对象和递归 commit tree，随后读取固定提交的 `raw.githubusercontent.com` 文本。对每个文本按 UTF-8 字节构造 Git blob 头部后计算 SHA-1，与 tree 中的对应条目比较；初次 63/63 一致，Hosts 补查扩大后 851/851 一致，未截断源码树。增强大师本轮的两份文本也分别校验通过。全程保持 TLS 验证。

文档检查执行 `git diff --no-index --check -- /dev/null docs/PIKO_DOWNLOAD_UX_REFERENCE.md`，无空白格式诊断（新增文件与 `/dev/null` 比较有差异，退出码 1）。独立文本扫描未发现尾随空白；固定提交源码引用的路径与行号均按本次读取的对应文件核对。未运行应用测试；本次没有修改业务代码。

关键来源 blob SHA 供再次核对：

- `desktopApp/.../winrt/FolderPicker.kt`：`f0ca2513f8ddb3d9aa7e71d4f10b65556cfcd8fd`。
- `desktopApp/.../DesktopPikoPlatform.kt`：`02218b43bf3cb9e62fd13caad63aa2ecf176a0a6`。
- `ui/.../screens/settings/SettingsScreen.kt`：`0796a50085aff197f8ad42b9a4406e4387f54bd4`。
- `ui/.../components/DownloadLauncher.kt`：`e3f4b08d3914db3075f9aa87fff12ec6f321a7d0`。
- `shared/.../download/PikoDownloadCoordinator.kt`：`903ade7a93591adba9f289f83eb79eb2d1ccbbf5`。
- `ui/.../screens/transfers/LocalTransferRow.kt`：`cf38999d0640b1ef8261bb2cb273fad624e41150`。
- `ui/.../screens/drive/QuickAccess.kt`：`76e5222ecab6966c2e5c2868cc28c2dca3bcf2f2`。
- `ui/.../components/WorkMeterBar.kt`：`e47da920714969ec0f2551e89169698e1b8c4c62`。

这些缩写路径只用于索引，完整原始路径见上文固定提交链接。未复制任何对应文件到 NekoPak；后续若采用代码复用，应重新经过许可证门禁，不能把本文的行为参考记录当作 AGPL 源码复用许可。
