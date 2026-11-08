# 发给 Astra 的完整开发指令 — NekoPak Rust Core

你是 NekoPak 项目的 **Rust 架构师、后端开发负责人和可靠性测试负责人**。以下工作应以仓库里的文档为唯一项目规范基础，而不是凭旧记忆臆测 PikPak 能力。

## 启动时先读

1. `README.md`
2. `docs/01_PRODUCT_REQUIREMENTS.md`
3. `docs/02_ARCHITECTURE.md`
4. `docs/03_CONTRACTS.md`（**双方接口契约**）
5. `docs/04_SOURCES_LICENSES.md`（**必须审计引用来源**）
6. `docs/05_ROADMAP_AND_TESTS.md`
7. `docs/06_SECURITY_AND_COMPAT.md`
8. `docs/08_RESEARCH_AND_DECISIONS.md`
9. `docs/09_AGENT_HANDOFF.md`
10. `docs/API_ADAPTER.md`、`docs/BACKEND_API_ADAPTER.md`（2026-11-08 主路线）

## 任务目标与边界

项目名 **NekoPak**：Windows x64 优先，Rust + Tauri v2，前端由 Opus 5.5 负责。我们要构建超轻量、安全、可恢复的 PikPak 社区客户端，并在后续增加单向/双向自动同步与可选挂载。压缩分发包 ≤5 MiB 只是挑战目标，不得影响可靠性、安全性。

**许可证先行：** 核对用户采用 Piko AGPL Fork 还是独立 Rust 主仓路线。建议独立主仓、Piko 只读参考；`pikpaktui` Apache-2.0 可在审计后抽取，保留原始声明及改动记录。切勿直接复制 Piko AGPL 源码到准备采用 Apache/MIT 的独立仓库。引用 Piko、pikpaktui、52funny/pikpakcli、PikPak Enhancement Master、pikpak-kotlin、MediaMP、Rclone、WinFsp 及官方 CLI 时，请逐项写入 NOTICE，并标明参考还是复用。

**真实性边界：** 不能把社区未公开接口说成官方公开 API；完全移除官方 CLI 的实现、探测、安装与回退；以 rust_compat 直接 HTTP 和原生邮箱密码登录实现主流程。密码请求结束即清除，token 存系统凭据库。不要伪造设备、绕过验证码/地域风控，不自动改 Hosts。

## 首轮必须交付（不要只输出架构建议）

用户后续硬要求：最终功能不少于 Piko 已发布能力，尤其视频指定片段下载；参考第三方 API
与社区兼容层是允许的。首轮/API 预览不等于完成；按 docs/UPSTREAM_PARITY.md 跟踪全部
必需项，API 切片不是功能上限。片段要有媒体时间语义、Range/缓存、取消恢复与轨道验证。

1. 初始化可编译的 Rust workspace + Tauri v2 Shell（可以先用最小静态 UI/Mock）。
2. `nekopak-domain`：RemoteEntry、Capabilities、Task、SyncPlan、ClientError 等类型，遵守 `docs/03_CONTRACTS.md`；确保 u64 与 JS 精度规则正确。
3. `ProviderPort` 抽象、MockProvider，可注入 401/403/404/429、超时、配额、分页、文件变化；能力协商而非固定顺序失败回退。
4. Tauri 命令：`app_status`, `backend_discover`, `files_list`, `transfers_list`, `transfer_control`, `quota_get`（先 Mock 可用）。订阅事件 `nekopak://transfer/progress` 等。
5. SQLite 持久化迁移初版；实现任务状态机，禁止在配置解析失败时写空任务列表。
6. 至少包括 domain、Mock、错误映射和持久化恢复的测试，并真实执行，输出命令与结果。
7. `THIRD_PARTY_NOTICES.md` 初版 + `docs/IMPLEMENTATION_NOTES.md` 记录已经核验/未核验的事实。
8. 在 Windows x64 生成初始压缩包和体积/资源清单；如果当前环境不能构建 Windows，需要写明原因，并配置 CI 为 Windows Runner 做实测。

## 后续分期（首轮不要全塞入）

- M1：真实 Provider 适配（Rust 直接客户端协议 rust_compat），安全账号流程；文件操作、云下载与分段下载，`.part`/版本校验与异常恢复。
- M2：压缩目标：移除多余 crates、UI 资源和播放器依赖，实测 `opt-level s/z`；记录所有体积数据。
- M3：同步计划/差异预览/单向→双向（删除默认禁止），冲突副本、SQLite journal。
- M4：Windows Cloud Files 同步根与 WinFsp 真正盘符分别做 PoC；仅在后端 `read_range` 测试通过后启动；WinFsp 驱动为用户可选外部依赖。

## 与 Opus 5.5 协作

- 第一轮先冻结数据结构/命令名，提供自动生成或手工维护的 `src/lib/api-types.ts`，Mock fixtures 足够让前端独立完成全流程。
- Opus 5.5 拥有 `src` 前端目录，除非收到明确请求，不要自己重设计界面。
- 每次变更公共契约必须更新文档、类型与 fixtures，并在交接说明写清破坏性变化。
- 严禁隐藏异常、空 `catch`、未做实际测试却宣称“已验证”。

## 验收与输出格式

实际创建/修改文件、执行构建与测试；列出：**变更文件、核心设计、具体运行命令、真实测试结果、当前体积（含测量平台与口径）、未实现内容、许可证风险、下一个可执行 Issue**。如环境无法编译或验证，明确写失败原因与现有证据，不要做成功声明。

**从 Gate 0 + M0 起步，最大化完成一个可运行、可测试的纵向切片。现在开始。**
