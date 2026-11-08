# 默认登录入口修复与用户测试版

> 历史登录测试包记录。用户后续反馈网盘安全验证没有入口，当前修复与新增实际功能见
> [DRIVE_FILES_FIX_REPORT.md](DRIVE_FILES_FIX_REPORT.md)，请使用 files-test 新包。

日期：2026-11-08；Windows x64。代码基线为
`b2f2923e0fb22cf82a60d74574d6e8e8d0c1467b`，本次产物包含工作区未提交修改。
本报告替代最初 API 迁移包中“默认禁用登录”的交付状态。

## 新版应用与打包结果

- 可直接运行：`dist/NekoPak-login-test.exe`；与压缩包内的 `nekopak.exe` 完全一致。
- 完整便携包：`dist/NekoPak-0.1.0-windows-x64-login-test.zip`，包含许可证和使用说明。
  解压后双击 `nekopak.exe`，不需要 CLI、编译器、环境变量或开发者注册。
- `cargo build -p nekopak-app --release --locked --offline`：Windows x64 发布构建成功。
- `python scripts/package_release.py`：通过。最初发现 tauri-plugin 2.7.1 crate 缺少
  许可证文本，已按其 `.cargo_vcs_info.json` 的上游提交
  `30da1fd6e17de6107ecc850c95dfb16b5729f2dd` 补齐 Apache/MIT 文本，验证 Git blob SHA
  并记录 SHA-256；没有复制实现源码。Windows 依赖许可证文本缺口为 0。
- ZIP CRC 校验通过，包内程序与独立程序 SHA-256 一致；包内仅有一个 `nekopak.exe`，
  没有附带 PikPak CLI 或其他外部可执行文件。
- EXE：8,816,640 字节；ZIP：5,077,513 字节（4.84 MiB）。这只是当前测试包的实测，
  不构成最终功能对齐版的体积承诺；运行需系统 WebView2。
- EXE SHA-256：`a171cceceef83e47c856c1675d6bbe372f2312399e7303337c9d5751b0f1ca96`。
- ZIP SHA-256：`5a2ef927e26193e899d0e45cd5a911920abab9e2da093554ab6887d22c4dafa8`。

机器可读记录：[login-fix-checks.json](evidence/login-fix-checks.json) 与
[login-test-size-report.json](evidence/login-test-size-report.json)。完整多平台依赖图
另外保留 6 个未使用平台的许可证文本缺口；没有因此声称其他平台已完成发行审计。

## 问题与修复

此前版本要求开发者环境变量，导致普通启动时账号、密码和登录按钮被禁用，用户无法
进行真实测试。这是开发侧配置与交付问题，不能以“账号尚未实测”为理由保留该门槛。

- `crates/nekopak-provider-native/src/profile.rs`、`lib.rs`：默认采用从官网公开
  Web SDK 核对的客户端标识和协议元数据，无需用户配置；不含客户端 secret 或用户凭据。
- `src/http.rs`：按 Web 协议发送登录正文和 `x-captcha-token` 请求头；使用系统 TLS，
  保留证书及域名验证。最多四次重试仅限连接建立失败；收到 HTTP 或传输错误后不重放登录。
- `src/captcha.rs`：人工验证打开系统浏览器，使用随机回环端口、路径与 state 接收新
  CAPTCHA token；没有有效回调不能继续。挑战取消/过期时释放监听，等待期间不保存密码。
- `src-tauri`：注册仅允许 HTTPS 的 opener；登录 IPC 为初始化和登录两个阶段提供
  有限超时，默认构建直接提供登录能力，没有 CLI 回退。
- `src/state/app.ts`、`src/pages/Auth.tsx`：区分会话读取失败与登录提交失败，避免
  密码错误或过早继续验证码时丢失表单；密码在请求结束后清空，错误诊断可脱敏复制。
- 更新默认 native fixture、请求协议与回调回归、默认启动测试、运行和接入文档。

## 实际执行结果

- 新增 `windows_default_build_offers_native_sign_in` 回归：先确认旧默认构建失败，
  修复后通过。此项验证不依赖开发者客户端环境变量。
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`：通过。
- `cargo test --workspace --locked --offline`：53 项通过，0 失败；8 项 ignored
  不计入通过（6 项外部媒体实验、1 项显式 Windows 凭据库集成、1 项匿名联网初始化）。
  其中 native provider 19 项通过，覆盖精确请求正文/头、人工回调、退出取消、
  凭据隔离和连接失败限次重试；HTTP 503 登录请求仅发一次。
- `cargo run -p nekopak-provider-native --example export_fixture --locked --offline`：通过。
- `npm run check`、`npm run build`：通过。
- `npm test`：80 项 Vitest 和 6 项 wire 测试通过。
- `node tests/e2e/native-startup.mjs`：4/4 通过；真实 Windows WebView2 + Rust IPC，
  全新隔离数据目录，不设置客户端 ID/secret，不接触用户账户。确认输入框和登录按钮
  可用，空表单提交显示本地校验，未实现操作仍禁用，外观设置刷新后保留。
  证据：[desktop-native-v3.json](evidence/desktop-native-v3.json)；
  [窗口截图](screenshots/ui-v3/desktop-native-sign-in.png)。
- 设置 `$env:E2E_BROWSER_CHANNEL='msedge'`、`$env:E2E_FILTER='登录'` 后执行
  `npm run e2e`：2/2 通过，外部请求 0。
  这是浏览器离线登录流程覆盖，不是账号验收；其中短信流程仅为 fixture 场景，
  native 短信登录未启用。证据：[ui-v3-e2e-focused.json](evidence/ui-v3-e2e-focused.json)。
- `cargo fmt --all --check`：通过。`python scripts/check_parity.py`：清单有效，
  0/24 必需功能组完成，当前仍是测试版，不能声明上游功能对齐。
- `git diff --check`：通过。

真实 WebView2 检查使用调试构建，因为需要隔离测试目录和调试端口；发布构建不会默认
开启这些测试开关。此前迁移阶段完整浏览器套件 47/47 通过，记录在 API_MIGRATION_REPORT.md；
本次针对变更的登录路径执行上述回归，没有将旧结果记作新一轮全量运行。

两位独立只读复核者分别检查规格与安全：默认登录、回调约束、密码生命周期、取消和
请求重放均未发现剩余交付阻断项；复核指出的旧 Android 请求/默认禁用文档已更正。
复核没有执行 GUI 或账户测试，不替代上述实跑结果。

## 真实服务与用户测试边界

没有使用真实账号、假密码或代替用户完成验证码。官网脚本核对和无账号匿名初始化
只能验证公开协议与部分连通性，不能证明实际账号可用。具体来源和 SHA-256 见
[NATIVE_LOGIN_RESEARCH.md](NATIVE_LOGIN_RESEARCH.md)。

本机对认证域名的匿名探测既观察到 HTTP 200，也多次遇到连接阶段 WinError 10054。
同一配置重复运行不稳定，根因未确定；当前 TLS 配置与有限连接重试不能被称作稳定修复。
相关记录见 [login-tls-compat.json](evidence/login-tls-compat.json)。收到错误会在应用显示，
不会伪装登录成功或自动切换到 Mock。

用户在新版 Windows 应用内输入邮箱密码，按服务提示完成验证码并验证会话与目录。
验证码服务是否接受 loopback redirect、Free/Premium 实际登录、刷新与真实文件分页仍需
用户验收。反馈使用结果及脱敏诊断即可，不通过聊天提供密码、token 或验证码。
没有完整实现真实上传/下载、远端写入、配额、片段、同步或挂载；这些限制保持可见。

## 许可证与后续工作

主仓继续 MIT。仅独立实现公开协议事实，没有复制官网 JS、AGPL 云盘代码、Android
secret 或验证码解题逻辑。官网脚本仅缓存于被忽略的研究目录，不进入发行包。
新增 native-tls / schannel 与 tauri-plugin-opener 等依赖进入 Cargo.lock、许可清单与
THIRD_PARTY_NOTICES.md；不打包或执行 PikPak CLI。

后续工作是结合用户提供的脱敏登录结果修正实际服务兼容差异，随后继续持久写入意图与
真实下载队列。账号测试由用户进行，协议修复与可运行产物仍由开发侧负责。
