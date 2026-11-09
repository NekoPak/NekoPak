# NekoPak 研发路线图、任务拆分与验收

版本：2026-11-07 / v0.1。**每个 Gate 必须先验证后推进**；不要一次并行写完大量未经验证的模块。

**最终发布门槛更新：** 用户要求功能不少于上游且允许参考第三方 API。`UPSTREAM_PARITY.md`
和 `parity-manifest.json` 是产品验收基线；本页 M0–M5 是实施顺序。阶段预览可发布，
但未通过 `scripts/check_parity.py --release` 不得标为功能对齐版。小包优化不能删功能。

**2026-11-09 实际进展：** Windows多文件ZIP／NSIS安装器签名和安装卸载已验证；Android独立
宿主、Keystore、手机交互、私有下载／SAF导出和签名APK已进入集成验收。模拟器的Rust/JNI/
文件字节／暂停重启／系统导出检查见 `ANDROID.md`。真实账号、物理ARM64、手机上传／目录同步／
前台服务仍未完成；原有Windows 24组对齐门禁没有因本轮平台工作而降低。

## Gate 0 — 来源、许可证、可行性（先于实际编码）

- [ ] 选择路径 A（Piko AGPL Fork）或 B（Rust 独立仓 + 只读 Fork 参考）；以 `04_SOURCES_LICENSES.md` 为依据。
- [ ] 在仓库建立 LICENSE、THIRD_PARTY_NOTICES.md、来源审计模板。
- [x] 2026-11-08 淘汰官方 CLI 适配；历史探测归档。
- [ ] 审计 rust_compat 的可用客户端身份、登录/CAPTCHA、刷新、配额和 Range；逐项记录实际观测。
- [ ] 检查 `pikpaktui` Rust 模块，统计可复用部分、阻塞式代码和隐含鉴权/风控风险；任何实际拷贝都带 commit SHA。
- [ ] 记录基础参考包体积：Piko v1.4.0 Windows MSI 和 APK；明确比较对象、版本和压缩算法。
- [ ] 开始编写 mock fixtures，不依赖真实云账户就可让 UI 开工。

**完成标准：** 源码策略和许可证已书面确认，契约 v0.1 冻结，输出 `docs/IMPLEMENTATION_NOTES.md` 与风险列表。

## M0 — 可运行空壳 + Mock（第一里程碑）

**Astra：** 建 Cargo workspace、domain/core/mock、Tauri 命令、事件总线、结构化错误、SQLite 迁移、基础单元/契约测试。

**Opus 5.5：** 完整壳 UI：侧栏、文件列表、任务管理、同步/挂载禁用态、设置、亮暗主题；Mock 数据/失败情景；键盘快捷键和可访问状态。

**验收：** Windows x64 `cargo test`、前端检查与 Tauri build 可跑；Mock 操作 100% 不调用真实服务；全 UI 点击有对应行为或明确 disabled 原因。

## M1 — 文件操作与下载稳定闭环（首个实用版）

- 账号：应用内邮箱密码登录、Windows 凭据库、CAPTCHA 人工衔接、串行刷新、退出与真实账号验收。
- 文件浏览：分页、目录缓存、搜索、元数据；新建/移动/重命名/回收站。
- 云下载：磁力链/URL 可用能力、创建任务与进度查询，精确报错。
- 下载：小文件/大文件并发、暂停/继续、`.part`、故障恢复与校验。
- 视频片段：先本地媒体核心，再直接 API 授权随机读取、共享块缓存与持久片段任务；MP4/MKV/TS、关键帧、音轨、画质、取消及恢复都需实际样本。
- 安全：日志脱敏、路径规范化、防重复副作用、配额显示。

**验收：** 文件大小和 bytes 一致；断网、磁盘满、宕机后能恢复；连续 20 次登录/退出不遗留可读明文密钥；不把失败任务显示完成。

## M2 — Lite 体积与发布基线

- 构建 Release + portable ZIP/7z + 安装器；分析可执行文件、资源、安装器占比。
- 移除默认不需要的 TUI、图片高亮、自更新、内置播放器、所有系统外部 CLI 发行文件。
- 比较 `opt-level=s/z`, LTO, `strip` 与关键 features，记录对性能和内存的影响。
- 自动生成依赖许可证和二进制大小报告；不能为了“≤5 MiB”牺牲证书/校验/可信更新能力。
- 档位：**≤5 MiB 为挑战目标**；>5 MiB 出具依赖清单与可选的后续精简计划，**不把指标当成无条件发布阻断**。

**验收：** 记录 OS/架构/commit/toolchain/打包方式/运行时是否另装；在干净的 Windows 10/11 虚拟机安装并使用，确认 WebView2 检测/安装流程。

## M3 — 自动同步：单向→双向

1. 单向 backup：本地→云、云→本地；先 dry-run 差异计划，默认无删除。
2. 增量：文件变更监听 + 远端增量/全量扫描（能力决定），保存 journal。
3. 双向：上次共同状态的三方比较，新增/修改/删除/冲突分类；启用删除必须单独确认。
4. 容灾：配置损坏、journal 中断、同名文件、大小写差异、同一路径改名/移动、短时断联保护。

**验收：** 每轮同步 `plan` 的破坏操作数可解释；任何扫描失败不得引起删除；冲突可找到完整的两份数据；重跑相同 plan 不产生重复文件。

## M4 — 文件系统集成（可选扩展）

**Windows Cloud Files API（按需文件）：** SyncRoot 与 placeholder PoC → 读取时 hydration → 脱机/释放空间 → Explorer 状态。必须有故障卸载/回滚。

**Windows WinFsp（真实盘符）：** 外部驱动检测 → 只读挂载 → Range 读取 → 缓存/锁语义/错误映射 → 稳定卸载。测试前提 `read_range` 能力真实存在。

**Linux FUSE：** 另设后续里程碑；Linux 桌面版先能同步，不保证先有虚拟盘。

**验收：** 大目录、1TB 稀疏文件、播放器拖动、断网、挂载后关机、卸载中读写、安全路径、缓存淘汰场景。没有真实挂载能力就不出现“已挂载”状态。

## M5 — 发布、监控与社区协作

- Release Notes、来源致谢、licenses、SHA-256、可复现/可追踪构建清单、自动化测试概览。
- 用户可导出脱敏日志和诊断信息，默认不上传遥测。
- 公开 Issues：普通 bug、同步破坏性问题、高风险安全问题分级处理。
- 客户端协议变化时明确错误并禁用未验证能力，不回退外部 CLI 或静默下载组件。

## 首批 10 个 GitHub Issues（供 Astra / Opus 5.5 拆分）

1. `[core] Define domain models & typed client errors` — 冻结模型、错误码、分页/ID。
2. `[backend] Implement MockProvider with deterministic fixtures` — 七组账号/故障样本。
3. `[app] Tauri v2 skeleton + typed IPC commands` — 编译 + mock smoke。
4. `[ui] Files explorer: virtual list, breadcrumbs, actions, empty/error/loading states` — 可访问性。
5. `[core] Persistent transfer queue and crash-safe journal` — kill/断网恢复。
6. `[backend] Native API identity and live auth acceptance` — 核验客户端身份、登录/刷新/退出与真实分页；不回退 CLI。
7. `[backend] Audit and extract allowed Rust logic from pikpaktui` — SHA/许可证/注释。
8. `[ui] Transfer dashboard + live progress and task controls` — 使用完整事件/快照策略。
9. `[sync] Read-only scan and dry-run plan engine` — 无破坏性变更。
10. `[build] Size baseline, feature flags, third-party notice scanner` — 体积报表。

追加优先事项：

11. `[media] Local clip core and keyframe/MP4 validation` — 本地 MP4/MKV 实验核心；TS 定位缺口独立跟踪。
12. `[provider] Community API protocol audit and authorized Range PoC` — 参考 pikpak-kotlin/pikpaktui/Rclone，核验授权、Range/版本/配额/错误；未实现时不删需求。
13. `[media] Remote clip plan, shared cache and durable tasks` — 不默认整片下载，画质与账号隔离、进度、取消重启、输出校验。
14. `[parity] Close published Windows feature gaps` — 搜索/整理/分享/归档/压缩包/播放器/本地映射等逐项真实验收。

## 自动化测试矩阵

| 测试 | 最低集合 | 发布阻断 |
|---|---|---|
| Rust `unit` | 路径编码、状态机、错误映射、Range 规划、同步冲突 | 是 |
| Rust `contract` | Mock/Native 能力分类、分页与事件乱序 | 是 |
| 文件完整性 | 完成校验、半截文件、ETag 变化、服务端错误 Range | 是 |
| 存储恢复 | kill -9/断电模拟、SQLite 损坏、部分元数据、迁移回滚 | 是 |
| 同步安全 | 远端空响应/403/500 不触发删除、冲突副本、循环目录 | 是 |
| UI E2E | 首次引导、列表、下载任务、同步计划、不支持功能禁用态 | 是 |
| 实际服务 | Free 和 Premium 账号最小非破坏性测试 | 公开功能上线前是 |
| Windows 挂载 | WinFsp 缺失、安装提示、读范围、断网、退出卸载 | 挂载发布前是 |
| 编译依赖 | license scanner、cargo deny/advisories、来源记录 | 是 |
| 体积性能 | compressed/uncompressed、启动、内存、CPU、安装时依赖 | 报告必需，5 MiB 非无条件阻断 |

## PR 验收模板

```text
## Change
## Source attribution / license impact
## Contract changes (Yes/No + fixtures)
## Tests performed (commands and environment)
## Offline and failure modes tested
## Size impact (before/after KB)
## Security and quota impact
## Screenshots (for UI changes)
## Known limitations
```
