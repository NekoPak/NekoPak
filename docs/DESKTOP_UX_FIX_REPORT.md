# 桌面下载体验修复交付记录

2026-11-08，Windows x64；基线 commit `b2f2923e0fb22cf82a60d74574d6e8e8d0c1467b`
加当前未提交工作区。承接「废弃 CLI 方案」中断时的目录选择、速度、配色、语言、
网络、托盘和图标修改。不是完整上游功能对齐版本。

入口：`dist/NekoPak-desktop-test.exe`；带使用说明和依赖许可证的完整包：
`dist/NekoPak-0.1.0-windows-x64-desktop-test.zip`。先正常退出旧程序（包含托盘进程），
再打开新版，沿用原应用数据与凭据库，不需清空任务。体积和哈希见
`docs/evidence/desktop-test-size-report.json`；检查汇总见 `desktop-ux-checks.json`。

## 本次行为

- 下载目录可点击路径或“选择文件夹”打开 Windows 系统选择器；取消保留原目录。
  设置支持保存默认路径及“每次下载询问保存位置”；任务菜单可打开该任务的原目的目录。
- Rust 按实际接收字节和单调时钟统计约三秒窗口速度。任务行、底栏与侧栏使用同一快照，
  暂停/完成清除速率，断点已有字节不计入新速度。进度仍为已验证提交的字节。
- 下载导航与其他侧栏项目使用一致的选中语义；非当前页不维持选中背景。进度填充至少
  可见，未完成轨道不再显示容易误认成进度位置的末端点；明暗主题保留独立颜色角色。
- 语言偏好为跟随系统/简体中文/English。主要登录、文件、任务、设置和导航文案接入词典，
  切换即时更新，无需页面刷新，Rust 保存为准。部分长提示、功能页和服务错误仍保留中文；
  不能宣称全量英语或 Piko 的五语种对齐已经完成。
- 网络增强默认关闭，开启后可选择系统代理、不使用代理或自定义 HTTP(S)/SOCKS5(H)代理。
  可选兼容连接限定认证/网盘 API 的 TLS 1.2；保留证书校验，不承诺解决特定地区连通性。
  不保存代理账号密码，不修改系统 Hosts、DNS、代理或地区检查。
- 托盘具备显示、隐藏、下载任务、退出菜单；关闭到托盘默认关闭。启用后关闭窗口保留进程
  和下载队列。UI、EXE、窗口和托盘共用 `public/brand/app-icon.svg` 生成的猫盒图标。
  后续换图标：替换该 SVG → `npm run gen:app-icons` → 重新构建。

## 中断后关闭的缺陷

新写并实际运行的五项定向测试中，四项在修复前失败：认证先于网络配置、切语言触发刷新、
模块初始化时固定侧栏文案、浏览器 Mock 重建后丢语言。修复后五项通过。

Rust 宿主在提供网络 IPC/启动下载 worker 之前恢复代理/TLS配置；前端在认证和能力读取前
恢复设置。语言状态改为驱动重新渲染，模块常量标签在读取时翻译，日期格式跟随当前语言。
WebView 存储不可用时不再刷新循环；浏览器 Mock 从设备语言偏好初始化。
全量浏览器回归还发现选中计数字符串缺少空格，恢复原格式后重跑。

## 实际验证

- `cargo test --workspace --locked --offline`：71 项通过，0 失败，8 项 ignored；忽略项为
  6 项显式外部媒体实验、1 项系统凭据库集成、1 项匿名联网初始化，不计入通过数。
- 随后追加并运行
  `cargo test -p nekopak-provider-native --locked --offline custom_proxy_routes_requests_and_disabling_it_restores_direct_connections`：1 项通过。
  使用合成请求，本机 HTTP 代理确实收到两次请求；禁用代理后两次请求到达直连服务器。
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`、
  `cargo fmt --all --check`、`git diff --check`：通过。
- `npm test`：88 项 Vitest + 6 项 wire 通过。`npm run build` 包含 TypeScript 检查，通过。
- `node tests/e2e/native-functional.mjs`：三组真实 WebView2 + Rust HTTP + SQLite/文件系统
  流程通过，使用本机服务与合成账号，下载 2,097,289 字节逐字节一致，无外部服务请求。
  覆盖实际速度、颜色、语言即时切换、禁用 localStorage 后启动/切换、托盘存在和关闭后
  进程/会话保留；CDN 不携带 API 凭据。详细证据在 `evidence/native-functional-e2e.json`。
- `node tests/e2e/native-startup.mjs`：4/4 通过，正常构建登录入口、能力禁用与主题持久化。
- 目录选择系统对话框沿用本任务前一窗口 2026-11-08 17:08 的实际选择/取消/持久化验证，
  记录为 `evidence/native-folder-picker.json`。本窗口未重跑该原生 UI 自动化。
- `E2E_BROWSER_CHANNEL=msedge npm run e2e`（PowerShell 用 `$env:` 设置变量）：47/47通过，
  外部请求0；包含浅/深主题和100%/125%/150%/200%缩放、键盘与框选。
- `cargo build -p nekopak-app --release --locked --offline` 与
  `python scripts/package_release.py`：通过。最终 EXE 10,142,720 字节（9.67 MiB），
  带许可证的 ZIP 约5.30 MiB，未达到≤5 MiB挑战目标。Windows 依赖许可证文本缺项为0。
  从最终 EXE 提取的32px图标已目视确认是猫盒图标，见 `screenshots/ui-v3/desktop-release-icon.png`。
  最终精确包体与 SHA-256 在 `evidence/desktop-test-size-report.json`；
  检查汇总在 `evidence/desktop-ux-checks.json`。浏览器 Mock 的媒体/同步/挂载通过不代表原生实现。
- `python scripts/check_parity.py`：清单校验通过，正式 Windows 功能对齐为 **0/24** 完整组。
  本报告不把部分完成或界面模拟计为完整功能组通过。

电脑控制插件在本窗口初始化成功，但 `list_windows` 连续返回
`Computer Use native pipe is unavailable ... os error 2`，重建会话后仍失败。
因此没有新验证托盘菜单的真实鼠标点击，也没有检查 Windows 任务栏图标缓存刷新。
已通过宿主状态/IPC、原生构建与打包图标资源检查的内容应与这些未测项分开理解。

## 文件与来源

主要代码：`crates/nekopak-core/src/{downloads,rates,settings}.rs`、Provider 下载观察器及
`crates/nekopak-provider-native/src/{download,http}.rs`；`src-tauri/src/{main,desktop}.rs`；
`src/lib/i18n.ts`、`src/lib/locales/en.ts`、启动状态/导航/设置/文件/任务界面；
`public/brand/app-icon.svg`、`src-tauri/icons`；对应 unit/native E2E 与打包脚本。
共享增量契约在 `03_CONTRACTS.md` §12。具体未提交文件以 Git 工作区为准，没有替换此前修改。

主仓仍 MIT。Piko v1.4.0 为行为参考，没有复制 AGPL 实现或图标。
来源固定提交与路径见 `PIKO_DOWNLOAD_UX_REFERENCE.md`；新增 dialog/rfd/tray 等依赖的精确
版本、许可证文本见发行包 inventory，未捆绑浏览器、JVM、FFmpeg、mpv 或 PikPak CLI。

## 仍需继续

用户在应用内进行真实账号/网络与下载验收；本轮未使用用户凭据。外部播放器、按画质下载、
视频指定时间片段、分享创建/取消、云下载创建、上传、目录递归下载、同步/挂载、自启动等
仍未完成。原有硬要求全部保留在 `UPSTREAM_PARITY.md` 和 `parity-manifest.json`。
下一开发 issue：按已核验的表示/时间语义实现媒体探测及受控外部播放，再接画质与片段任务，
继续落实持久化、取消恢复与原画/转码身份校验；不能把“打开 URL”或线性截字节当作完成。
