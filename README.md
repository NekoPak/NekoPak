# NekoPak

> A lightweight, Rust-powered PikPak client for desktop and Android.
>
> 独立社区项目，非 PikPak 官方客户端，与 PikPak 服务提供者无隶属关系。

**状态：** `Pre-alpha / Desktop + Android integration`（Rust 核心与手机宿主已接入；真实服务与全部上游功能仍逐项验收）
**文档基线：** 2026-11-10 · `0.1.1` 预览
**平台：** Windows x64 已通过自动构建、签名清单及安装／卸载测试。Android 已接入独立宿主、Keystore 凭据库、小屏界面和 APK 构建流程，完整验收状态见 [Android 说明](docs/ANDROID.md)。Linux / macOS 已有核心 CI，尚未完成桌面验收。

**验收结论：** Windows 打包通过不等于产品功能验收完成。`python scripts/check_parity.py --release` 当前仍拒绝功能对齐发布，24 组必需能力均未完成全部验收。真实账号由用户在应用内验证。

NekoPak 的目标不是再包装一个网页，而是构建具有原生 Rust 核心的轻量云盘管理工具：文件管理、云下载、分段下载、可靠同步、按需缓存、可选的文件系统挂载。UI 采用 **Tauri v2 + 轻量 TypeScript/CSS**，利用 Windows 已有 WebView2；不捆绑 JVM、Chromium、全套 mpv/FFmpeg、Rclone 或 WinFsp。

## 设计承诺

**用户确认的最终门槛：功能不少于上游，包含视频指定片段下载。** 以
[功能对齐基线](docs/UPSTREAM_PARITY.md)和 [逐项清单](docs/parity-manifest.json)
验收；当前预览远未完成对齐。主连接为 Rust 直接调用 PikPak 客户端接口的
`rust_compat`，开发与集成状态见 [API 接入说明](docs/API_ADAPTER.md)。

1. **轻量而可靠：** Windows x64 Lite 的压缩发布包以 `≤ 5 MiB` 为**挑战目标**，绝非验收前的性能承诺；优先保证安全、数据完整性与下载可靠性。
2. **直接 API 接入：** 彻底停用官方 CLI 的探测、安装引导和子进程适配；普通文件浏览与下载走 Rust 核心。使用社区客户端协议不等于获得官方公开 API 承诺。代码、登录标识和能力都需单独核验。
3. **默认不破坏数据：** 先只读浏览和单向备份，再做双向同步。远端不可达、令牌失效、元数据不全时禁止判定文件“已删除”。
4. **可选组件、完整能力：** 媒体处理和外部后端可按需配置，只有检测通过才启用；最终上游功能对齐版不能用“可选”或小包目标删除媒体、片段、分享等已发布功能。Cloud Files/WinFsp 分别验收。
5. **公开致谢：** README 及 `THIRD_PARTY_NOTICES.md` 明确注明参考、复用、外部依赖的来源和各自许可证。
6. **凭据保护：** 应用内邮箱密码登录；密码仅用于当前请求，结束即清除。Windows 会话存系统凭据管理器；Android 使用系统 Keystore 密钥加密后存入不备份的私有目录。不保存明文到配置或任务库。不修改系统 Hosts、不绕过账号风控；高级网络功能默认关闭。

## 文档导航

| 文件 | 用途 | 负责人 |
|---|---|---|
| [`AGENTS.md`](AGENTS.md) / [`CLAUDE.md`](CLAUDE.md) | 两位 Agent 的仓库级开发约定 | 双方 |
| [`docs/01_PRODUCT_REQUIREMENTS.md`](docs/01_PRODUCT_REQUIREMENTS.md) | 产品规格、功能优先级、非目标、验收 | 双方 |
| [`docs/02_ARCHITECTURE.md`](docs/02_ARCHITECTURE.md) | Rust 分层、后端策略、同步与挂载、轻量化 | Astra |
| [`docs/03_CONTRACTS.md`](docs/03_CONTRACTS.md) | Rust ↔ Tauri ↔ 前端稳定接口 | 双方先冻结 |
| [`docs/04_SOURCES_LICENSES.md`](docs/04_SOURCES_LICENSES.md) | **完整参考项目、许可证、致谢义务** | 双方 |
| [`docs/05_ROADMAP_AND_TESTS.md`](docs/05_ROADMAP_AND_TESTS.md) | 分阶段实施、验收与体积基准 | 双方 |
| [`docs/06_SECURITY_AND_COMPAT.md`](docs/06_SECURITY_AND_COMPAT.md) | 授权、配额、隐私、实验性兼容 | Astra |
| [`docs/07_UI_SPEC.md`](docs/07_UI_SPEC.md) | 页面、组件、交互、设计令牌 | Opus 5.5 |
| [`docs/08_RESEARCH_AND_DECISIONS.md`](docs/08_RESEARCH_AND_DECISIONS.md) | 已证实的事实、待查问题、决策记录 | 双方 |
| [`docs/API_ADAPTER.md`](docs/API_ADAPTER.md) / [`docs/BACKEND_API_ADAPTER.md`](docs/BACKEND_API_ADAPTER.md) | 当前直连实现与 v3 接口交接资料 | 双方 |
| [`prompts/ASTRA_BACKEND.md`](prompts/ASTRA_BACKEND.md) | 可直接交给 Astra 的任务指令 | Astra |
| [`prompts/OPUS_FRONTEND.md`](prompts/OPUS_FRONTEND.md) | 可直接交给 Opus 5.5 的任务指令 | Opus 5.5 |
| [`templates/THIRD_PARTY_NOTICES.md`](templates/THIRD_PARTY_NOTICES.md) | 项目公示用来源与署名模板 | 发布前维护 |

## 建议开发顺序

- **Gate 0:** 先明确源码路径：若在 Piko Fork 上改写并沿用其代码，遵守其 AGPL-3.0-or-later；若独立建仓，仅复用 Apache/MIT 许可代码且未引入 AGPL 代码，可考虑 Apache-2.0。授权最终须基于**实际复用代码**决定。
- **Gate 1:** 先冻结 `docs/03_CONTRACTS.md` 的类型、错误码和事件。Astra 建立可运行的 Rust Mock Backend，Opus 5.5 对 Mock 开发 UI。双方无需等待真实账号。
- **Gate 2:** 实现最小实用闭环：授权/登录、文件列表、磁力云下载任务、下载进度、暂停恢复、断电恢复。
- **Gate 3:** 单向同步、预览差异、冲突与配额；之后双向同步与 Cloud Files。
- **Gate 4:** 真正的虚拟盘符（Windows WinFsp / Linux FUSE），作为可选模块和独立验收项目。

## 重要边界

- NekoPak 不是 PikPak 官方授权的软件；客户端接口可能变化，真实账号可用性必须实测。默认构建内置官网公开的 Web 登录标识与协议版本，不内置 client_secret、用户凭据或 Android 身份。
- 配额必须按实际连接方式、账号和服务响应区分。Connected Apps 的公开规则仅适用于该类连接，不能未经验证直接套到客户端协议，也不能因更换接入方式宣称无限额。
- 文件系统挂载与“同步文件夹”是两种不同机制。Windows Cloud Files API 提供同步根目录和占位文件，**不等于独立的 `P:` 盘符**；WinFsp 类驱动才能提供通常意义的独立挂载盘。
- 任何性能、体积指标必须附构建 commit、配置、平台、测量步骤和测试数据。

## 主要参考链接

- Piko: <https://github.com/NihilDigit/piko>
- pikpaktui: <https://github.com/Bengerthelorf/pikpaktui>
- pikpakcli 协议研究: <https://github.com/52funny/pikpakcli>
- PikPak Connected Apps FAQ: <https://mypikpak.com/en-US/connect-apps-faq>
- Rclone PikPak: <https://rclone.org/pikpak/>
- Tauri Windows WebView2: <https://tauri.app/zh-cn/distribute/windows-installer/>

进一步参考请见 `docs/04_SOURCES_LICENSES.md`。**未经许可证核查请勿复制第三方源码。**

## 当前后端交付

运行与打包见 [docs/RUNNING.md](docs/RUNNING.md)，实现边界与前端交接见
[docs/IMPLEMENTATION_NOTES.md](docs/IMPLEMENTATION_NOTES.md)。Rust Core 可独立测试，
Tauri 使用导入的 v3 产品界面，默认主连接为 `rust_compat`，没有后端选择或 CLI 安装页。
邮箱密码、Windows 会话保存/刷新/退出、网盘独立 CAPTCHA 与人工验证恢复已实现。
目录、最近、星标、回收站、搜索分页、分享列表、基础文件修改、容量/额度查询已接入 Rust。
原文件下载写入实际字节，支持暂停、恢复、重启后校验续传、限速与不覆盖同名目标。
默认登录无需配置环境变量；真实账号和验证码由用户在应用内操作。用户已反馈前版能建立
会话但网盘验证受阻，本版针对该故障补齐 DRIVE 协议与恢复入口，不因此声称真实目录验收。
后续已接云任务、上传、原生媒体选段/转封装、分享与归档、同步、Cloud Files及WinFsp适配。
实际验证范围和仍需验收的格式/服务/平台边界见 [整体集成记录](docs/DESKTOP_INTEGRATION_REPORT.md)。

```powershell
npm ci
cargo test --workspace --locked
npm run check
npm test
npm run build
cargo run -p nekopak-app --locked
```

浏览器 `npm run dev` 使用离线演示数据；桌面离线验收须显式设置
`$env:NEKOPAK_BACKEND = 'mock'`。桌面不会回退到浏览器 Mock。
当前 Windows 发行形式为 `dist/NekoPak-0.1.1-windows-x64-portable.zip`（解压为完整多文件目录）
和 `dist/NekoPak-0.1.1-windows-x64-setup.exe`（安装/快捷方式/卸载）。目录中的
`NekoPak/nekopak.exe` 是程序入口；无需单独复制程序以外的系统依赖。
本轮目录选择、速度、语言、网络和托盘记录见
[DESKTOP_UX_FIX_REPORT.md](docs/DESKTOP_UX_FIX_REPORT.md)。此前文件、登录与迁移记录保留在
DRIVE_FILES_FIX_REPORT.md、LOGIN_FIX_REPORT.md 和 API_MIGRATION_REPORT.md。

本轮选择独立 Rust 仓库、MIT 许可。参考项目与实际依赖分别记录于
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。Bern 作为协议/引擎/宿主分层参考，
没有复制其源码，也没有引入 Bern 运行时。

发布仓库：[NekoPak/NikoPak](https://github.com/NekoPak/NikoPak)。持续集成与发布签名方案见
[RELEASE_SIGNING.md](docs/RELEASE_SIGNING.md)。私钥只保存在受保护的本机备份和 GitHub Actions Secrets，源码只包含公钥和工作流。

## Android 构建与当前范围

Android 7.0 / API24 起，首个手机发行目标为 ARM64；x86_64 用于模拟器验收。
共享 Rust 登录、文件、云任务和媒体核心，使用系统 Android WebView，不捆绑浏览器或 JVM。
APK 内包含原生代码、Android 宿主和第三方许可材料；手机包大小不套用 Windows Lite 的挑战值。

`0.1.1` 使用独立的 **文件／传输／我的** 底部导航、单击打开／长按多选的文件列表、
传输卡片与设置子页。桌面侧栏、标签页和多段工具栏不再进入手机组件树。Android 启动图标
从项目猫箱图标生成，打包时核对 APK 中的实际像素和自适应／圆形／主题图标引用；跟随系统
语言读取 Android 原生配置。修复与本轮验证见 [手机与连接恢复记录](docs/MOBILE_RECOVERY_REPORT.md)。

登录页支持 **记住账号／保持登录**，用户名及会话偏好由系统加密凭据库保存，密码仍只用于
当前请求。代理恢复后可直接重试登录或重新连接；临时网络故障不会清除可恢复会话或统一显示
为密码错误。设置中的域名选择和测速只针对固定认证／网盘 API，不改写下载链接。
下载断线后有限自动重试，并遵守服务端等待时间；界面显示实际接收速度。已完成、失败、取消
的任务可以移除记录，保留下载文件及任务回执，跨账号和损坏记录会被拒绝。

手机本地下载写入应用私有目录，完成后通过任务菜单的 **导出文件** 调用系统保存选择器。
卸载应用会删除未导出的私有文件。应用切到后台会暂停本地任务，返回后手动继续；当前没有
前台服务，不承诺锁屏连续下载。上传、共享目录同步、本地映射、桌面托盘／自启动、
外部播放器和 Windows 挂载在 Android 上明确禁用，后续适配要求保留。

开发环境需要 JDK21、SDK36、Build Tools36、NDK27.2.12479018，命令与验证证据见
[docs/ANDROID.md](docs/ANDROID.md)。`android.yml` 生成明确标为 debug 的测试 APK；
`release.yml` 分别构建 Windows ZIP/Setup.exe 与独立签名的 Android APK。
Actions 产物与公开 GitHub Release 是两个步骤；未通过功能对齐门禁的版本仅可标为预览。
