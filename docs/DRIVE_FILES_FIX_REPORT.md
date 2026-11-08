# 网盘验证与基础文件功能交付记录

2026-11-08，Windows x64。基线 commit 为 `b2f2923e0fb22cf82a60d74574d6e8e8d0c1467b`，
产物包含当前未提交工作区修改；不是完整功能对齐版。

新版入口：`dist/NekoPak-files-test.exe`；完整包
`dist/NekoPak-0.1.0-windows-x64-files-test.zip`。程序使用现有本机应用数据与会话；先正常关闭
旧NekoPak再打开新版即可，不需要清理数据库、设置客户端参数或安装PikPak CLI。
包体、SHA-256与源文件记录见 `docs/evidence/files-test-size-report.json`；校验汇总见
`docs/evidence/files-fix-checks.json`。

## 用户报告与根因

用户给出的两类诊断分别发生在 captcha_init 连接阶段和已登录后的 files_list。
截图显示已有账号会话，网盘却返回“需要安全验证”，只有权限错误，没有可操作入口。
此外最近/星标/回收站/分享和下载等前端页面没有 Rust 实现，无法支持基础使用。

先加入 `drive_security_verification_is_actionable_without_logging_out`，实际执行后失败：
期待 VERIFICATION_REQUIRED，得到 PERMISSION_DENIED。经官网公共 SDK 与固定提交社区源
核对，确认原先遗漏已登录 DRIVE CAPTCHA metadata/token/request wrapper，且错误分类与
界面恢复入口没有接通。默认 action 应为 method:path（无 origin/query），旧研究结论已更正。

当前在所有 DRIVE 路径共用验证处理：保留账号会话，获取独立验证 token；需要人工时只返回
本地 verification_id，Rust 校验并打开链接。用户完成浏览器验证后继续读取，不要求再输入密码。
真正权限不足仍保留 PERMISSION_DENIED。旧凭据没有 sub 时使用现有授权查询 user/me 补齐。

## 这次接通的实际功能

- 普通目录与元数据、最近事件、星标列表/标记、回收站列表/还原、分页文件名搜索。
  搜索对每页做实际名称匹配；未读完的分页继续可见，不把首屏零匹配当全盘无结果。
- 新建目录、重命名、移动、复制、移入回收站；批量异步响应要核对任务完成，HTTP200
  不直接代表操作成功。写入前持久保存账号范围 intent；未知结果不自动重发。
- 我的分享只读列表、存储容量、账号传输额度；服务缺少字段保持未知。used_bytes保留
  超额用量，日上限0不当耗尽。没有把客户端额度标成 Connected Apps 额度。
- 原文件下载：实际 HTTP 字节写入、进度快照、暂停/继续/取消、断网后重试、重启后暂停
  等待显式恢复、块 SHA-256、最终精确长度、Windows 句柄原子改名且不覆盖目标。
  签名 CDN 地址与账户凭据留在 Rust，下载请求不含 API Bearer/CAPTCHA。
- 下载目录与带宽上限、主题持久化；系统代理/应用内禁用代理可切换。所有设置仅作用于
  NekoPak，不修改全局网络。尚未实现的并发连接/媒体设置不接受假保存。
- 网盘和失败下载任务都能打开验证并继续；旧账号迟到结果、分页和撤销记录不污染新账号。
  原生传输事件只提示重读当前账号过滤后的快照。

主要修改目录：`crates/nekopak-provider-native`（drive/files/download/http/wire/profile）、
`crates/nekopak-core`（downloads/intents/settings/store）、领域/Provider契约、`src-tauri`、
前端 typed IPC、错误恢复、文件/下载/设置界面与对应测试。

## 实际执行的验证

- `cargo test --workspace --locked --offline`：67项通过，0失败；8项显式 ignored 不计通过
  （6项外部媒体实验、1项系统凭据库独立集成、1项匿名联网初始化）。
- 随后新增 CDN Range 拒绝回归并单独执行：正确206返回真实字节；错误范围、服务忽略
  Range返回200、截短响应均拒绝。请求断言无任何API凭据头。结果1项通过，合计68项。
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`：通过。
- `cargo fmt --all --check`、`git diff --check`：通过。
- `npm run check`、`npm run build`：通过。`npm test`：82项 Vitest + 6项 wire 通过。
- `E2E_BROWSER_CHANNEL=msedge` 后执行 `npm run e2e`：47/47通过，外部请求0。
  此套件是浏览器离线演示回归；其同步、挂载和媒体场景不证明原生后端已实现这些功能。
- `node tests/e2e/native-startup.mjs`：4/4通过；真实 WebView2 + Rust IPC，独立数据目录，
  普通构建无开发客户端参数也有可用登录表单；没有输入账号密码或发送服务请求。
- `node tests/e2e/native-functional.mjs`：3组端到端流程通过。真实 Windows WebView2、
  NativeProvider HTTP、SQLite与文件系统，服务端是本机隔离 fixture，测试使用明确合成账号。
  覆盖表单登录→网盘人工验证→保留会话继续列表→新建/改名/移动/星标/回收站恢复→最近与
  分享列表→保存下载/代理设置→原文件下载→刷新后持久化→退出后账号任务隐藏。
  实际下载2,097,289字节，与源逐字节一致；CDN请求断言没有 Authorization/CAPTCHA/device头。
  证据：[native-functional-e2e.json](evidence/native-functional-e2e.json)，
  [验证入口](screenshots/ui-v3/desktop-drive-verification.png)，
  [下载完成](screenshots/ui-v3/desktop-native-download-completed.png)。
- 6项原生文件落盘/恢复回归全部通过：暂停/断网/重启；末尾追加拒绝发布；发布后崩溃恢复；
  换号不终止worker；损坏part/遗失intent库保留现场；零文件/目标冲突；校验取消不被全局锁阻塞。
  另有 intent 持久化/账号隔离测试通过。
- `cargo run -p nekopak-core --example export_fixtures --locked --offline` 与 native fixture
  exporter：通过，保持各端契约一致。
- `cargo build -p nekopak-app --release --locked --offline`：Windows发布构建通过。
  随后因新增测试文件触发打包器的源码时间戳保护，执行
  `cargo rustc -p nekopak-app --release --locked --offline -- -C debuginfo=0` 重新链接最终程序；
  最终产物结果与哈希单独记录于 files-fix-checks.json 和 files-test-size-report.json。

Native功能E2E的可配 loopback provider、合成内存vault、验证链接接收桥仅在debug构建编译，
还要求独立数据目录与隐藏测试窗口开关。release完全不含这些测试入口；正常用户网络仍固定HTTPS。
它不访问用户账户、不读取用户凭据库、不自动解决真实验证码。

## 复核发现与关闭

独立 Standards/security 与 Spec 只读复核发现的问题已修复并加入相关回归：
POST受理后的验证码不释放intent；task身份缺失不能假完成；最终文件长度必须精确；
发布崩溃恢复；换号/取消状态不会杀整个worker；旧 CAPTCHA 迟到错误不清除新token；
已初始化actions库遗失不重建；事件与撤销记录账号隔离；配置写入失败不先改代理；
最终/恢复校验在全局控制锁外分块运行，观察revision与账号变化；配额与分享有效期准确映射；
最近跨页去重和旧查询响应隔离。
最终另一次聚焦只读复核确认全文件校验已离开全局控制锁、校验取消后不会发布，并确认
debug-only loopback/测试凭据/测试验证桥在标准release中不可用。未把静态复核当作账户验收。

## 尚未完成与真实验收边界

这次没有使用用户凭据执行真实 PikPak 请求。用户前版截图表明曾建立登录会话，不能推导
本版真实目录/验证码回流/写入/下载都已验收。本机之前的10054连接重置也没有被证明彻底消除；
当前网络设置和有限连接重试提供可用排查手段，不宣传根治。

云下载创建、上传、分享创建/取消、永久删除、媒体转码/指定片段、目录递归下载、同步、
挂载、解压等仍未实现。下载当前单任务单连接，速度字段未统计，不提供虚假速率。
任务取消保留part与记录；未知远端写结果需用户刷新核对，不承诺可自动追回远端操作。
当前原始下载没有可独立验证的远端全文件哈希承诺，记录的是HTTPS来源、版本、精确长度与
本地块SHA-256完整性，不把GCID冒充标准SHA-1。

下一实际步骤：用户运行新EXE，验证真实网盘是否能读取及人工验证能否回流，再用自建测试
文件夹和小文件验收文件操作、下载。开发侧据脱敏结果修正协议差异，随后推进云下载及
其他上游功能；旧的完整功能对齐要求继续保留。

## 来源与许可证

主仓仍MIT；没有复制官网JS/AGPL客户端实现、秘密客户端身份或验证码答案。
公开Web协议参数与源码核对见 DRIVE_PROTOCOL_RESEARCH.md / FILE_PROTOCOL_RESEARCH.md，
包含URL、固定提交、原文件路径和hash。新增 md-5 0.10.6（MIT OR Apache-2.0）仅用于
公开协议计算，文件校验使用SHA-256。完整发行包包含实际依赖许可证，不含PikPak CLI。
