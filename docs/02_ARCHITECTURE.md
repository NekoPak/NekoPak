# NekoPak 技术架构设计（ADR-0001）

状态：草案，供 Astra / Opus 5.5 联合评审。**不宣称当前这些模块已实现。**

## 0. 架构原则

- Rust 作为唯一业务核心，Tauri v2 只做宿主与 IPC，前端 TypeScript/CSS 专注交互和呈现。
- **能力优先于后端优先级：** 不能假设所有后端提供相同功能。适配器必须对每个操作声明 capability，并携带明确错误类型。
- 运行时可选安装外部工具，基础包不捆绑官方 CLI、Rclone、WebDAV 服务器、WinFsp 或媒体解码库。
- 官方 CLI 路线已完全移除。主连接为 Rust 直接 HTTP 客户端协议 rust_compat；Mock 仅用于离线开发。登录为原生表单，token 由系统凭据库保存。
- 不依赖未公布的“官方通用 REST API”；社区实现访问的非公开接口要在文档和 UI 中说明，并做隔离。
- 先可用、再可靠、最后小型化；体积减少不得破坏安全与数据一致性。

## 1. 分层关系

```text
┌──────────────────────────────────────────────────────────┐
│ UI: TypeScript + CSS (可选 React + Vite)                │
│ 文件浏览 │ 任务中心 │ 同步中心 │ 设备挂载 │ 高级设置 │
└───────────────────┬──────────────────────────────────────┘
                    │ Tauri invoke / event (typed contracts)
┌───────────────────▼──────────────────────────────────────┐
│ NekoPak App Shell (Tauri v2)                            │
│ command handlers │ event gateway │ secure storage       │
└───────────────────┬──────────────────────────────────────┘
                    │ Rust service interfaces
┌───────────────────▼──────────────────────────────────────┐
│ NekoPak Core (Rust, Tokio as needed)                    │
│ Auth/Session │ Fs Index │ Task scheduler │ Downloader    │
│ Sync engine  │ Cache    │ SQLite repo    │ Quota & Policy │
└───────┬───────────────────────┬──────────────────────────┘
        │ ProviderAdapter       │ PlatformAdapter
┌───────▼───────────────┐  ┌────▼─────────────────────────┐
│ rust_compat (primary) │  │ Windows Cloud Files API     │
│ mock (offline dev)    │  │ WinFsp mount (opt)          │
│ webdav (Premium/opt)  │  │ Linux FUSE (later/opt)      │
│ rclone (opt/external) │  │ system player integration   │
│ mock (tests)          │  └──────────────────────────────┘
└───────────────────────┘
```

## 2. 建议 workspace

```text
nekopak/
  README.md
  LICENSE                       # 由实际来源路径决定，不得想当然
  THIRD_PARTY_NOTICES.md
  docs/
  crates/
    nekopak-domain/             # 无网络纯模型/错误码/能力定义
    nekopak-core/               # 任务调度/下载/同步/缓存
    nekopak-provider/           # ProviderPort 抽象和 router
    nekopak-provider-mock/      # 静态模拟数据/错误注入
    nekopak-provider-native/    # 主连接：直接 HTTP、会话、系统凭据库
    nekopak-sync/               # 同步规划器与状态机
    nekopak-mount/              # trait + 只读抽象；平台 feature
  src-tauri/                    # Tauri commands, security
  src/                          # TypeScript 前端
  tests/                        # 端到端/契约/故障注入
  scripts/                      # 构建体积基准与合规检查
```

注：初期按实际复杂度可以合并 crate，**不为抽象而拆碎依赖树**。`domain` 尽量零外部依赖。

## 3. ProviderPort 设计

统一 `list`, `stat`, `search`, `mkdir`, `rename`, `move`, `copy`, `trash`, `upload`, `download`, `cloud_task`, `quota`, `read_range` 等能力。关键点：

- `discover()`：探测后端版本、授权状态、账号类型、可用功能、是否支持随机读取；结果可过期，定时刷新。
- `capabilities` **按单个操作**声明，必要时再细分：例如 `read_range`, `stream_url`, `server_copy`, `offline_task`。
- 普通浏览/下载固定使用 rust_compat，按操作检查能力；不提供后端选择或自动失败回退。WebDAV/Rclone 仅作为同步/挂载扩展。
- 主适配器的 read_range 必须独立验收；接口资料中出现下载链接不代表随机读取已验证，未通过则禁用挂载。
- `AuthExpired`：要求用户重新授权；`PermissionDenied`：提示权限；`QuotaExceeded`：展示配额；`NotFound`：终止任务；`TransientNetwork`：指数退避有限重试；`Unsupported`：可尝试其他已授权后端。
- 若文件在后端间切换，先比较 remote ID、期望长度、已下载分块校验信息；不满足一致性则另存/重下，禁止在不验证情况下续写旧 `.part`。

**技术债风险：** `pikpaktui` 使用 `reqwest::blocking` 和与命令行耦合的结构，需提炼逻辑、适当改为异步；不能将其所有文件整体复制到 GUI 内。

## 4. 下载引擎

视频“片段下载”是按媒体时间范围裁剪/转封装，与下载器字节分块并发是两件事。
采用 Provider 的授权随机读取 + 内容/账号/表示身份隔离的共享缓存 + 独立媒体处理核心。
直接 API 的每项能力统一通过 ProviderPort 测试和真实账号验收。
不能按总字节乘时间比例替代媒体 seek，也不能默认整片预下载后宣称节省片段流量。
本地媒体核心起步实现见 `MEDIA_SEGMENTS.md`，整体上游对齐见 `UPSTREAM_PARITY.md`。

- 后台下载使用有界并发和排队；连接数默认保守且可调，遇到限流不增加连接数猛冲。
- 任务状态：`queued → running → paused|retrying|verifying → completed|failed|cancelled`。中断后持久化状态，不依赖 UI 存活。
- 使用 `file.part` + `.part.meta`；写入块级状态并校验服务器 `Content-Range`、文件长度、ETag/版本（如果可用）。
- 目标文件只能在文件大小/校验合格后原子替换；无法原子替换则写同目录临时文件，并做好回滚记录。
- 无完整可靠 hash 时只声明完成长度/版本校验，不夸大为“强完整性保证”。
- 带宽限制、网络变更、睡眠恢复、磁盘已满、同名文件、无写权限都应有可读错误。

## 5. 同步引擎：与挂载分开

### 5.1 同步数据源

- 本地文件信息：规范化路径、size、mtime、可选 hash、file ID（平台允许则记录）。
- 远端文件信息：remote ID、父目录 ID、name、size、mtime/hash（如有）、etag/version（如有）。
- `sync_journal` 表存放上次共同状态，用**三方比较**（上次→本地、上次→远端）识别新增/删除/修改/冲突。
- 两端不支持稳定文件 ID 时，重命名判断降级为启发式，并提示可能重新下载。

### 5.2 三步提交

1. `scan`：可取消、分页、非破坏扫描；扫描失败不能产生删除动作。
2. `plan`：输出结构化 `SyncPlan`，包含 `new`, `update`, `conflict`, `would_delete`, `unknown`; 默认仅预览。
3. `apply`：上传/下载先落临时项，再提交、更新 journal；删除必须单独开启且有回收站或恢复策略。

### 5.3 保护规则

- 初次同步禁止删除；不允许将网络故障解释为远端空目录。
- 冲突保留两个版本或交由用户选择；绝不默认最后写入覆盖。
- 忽略临时文件、锁文件、符号链接（默认不追踪）和危险特殊路径。
- 防止两个同步根目录相互嵌套/循环同步；不允许本地目录与缓存目录重叠。
- 仅靠文件监听不能发现远端变化；本地 watcher + 远端分页轮询，支持断线重扫。

## 6. 挂载与按需下载

这不是同一个功能，需分别立项验收。

**Windows Cloud Files API：** 注册 SyncRoot，将目录映射到文件管理器；占位文件通过系统回调获取内容。先 Windows 平台 PoC，逐步补全 hydration/dehydration、脱机恢复和 Explorer 状态。Cloud Files 机制不会自动生成 `P:` 盘符。

**Windows WinFsp：** 用户自行安装受支持的 WinFsp 运行时，NekoPak 可选 feature 适配 `read`, `getattr`, `readdir`, `flush`, `unmount` 等；首个版本只读。严格验证随机读取/块缓存，避免播放器一次打开就把所有文件下载完。WinFsp 有 GPLv3 + FLOSS 特例，不能当成无条件 MIT 依赖。

**Linux FUSE：** 通过系统 libfuse / FUSE 设备使用，与 Windows 挂载模块分开编译。Linux 先支持下载+同步，FUSE 延后。

**缓存：** LRU 上限、每个文件版本标识、块级校验、预取窗口/最大并发控制；离线标记、淘汰、空间不足可诊断。别把大量视频数据存进 SQLite。

**降级：** 若后端不支持 `read_range`，隐藏/禁用真实挂载入口；仍可提供同步文件夹或普通下载。真实盘符不是必达 MVP。

## 7. 持久化

- 使用 SQLite/WAL +事务保存下载任务、同步 journal、文件索引、缓存元数据（按需迁移表结构）；可再以 `rusqlite` 最小 features 或其他方案按实测决定。
- 强制持久化边界：任务状态迁移、写块确认、同步计划提交、远端删除执行前。
- 启动时若存储损坏：只读保留损坏文件、提示修复，不自动写空记录覆盖；支持 `PRAGMA integrity_check` 及可回滚迁移。
- `settings` 与 `task state` 分离存储，配置损坏不应丢下载历史。

## 8. 体积与运行指标

| 指标 | 目标（非承诺） | 说明 |
|---|---|---|
| Windows x64 Lite 压缩分发包 | **挑战 ≤ 5 MiB** | 仅包含 NekoPak 核心与轻量 UI；外部程序不计入须如实披露 |
| 无法达到 5 MiB 时 | 记录实测与分解 | 先比较 8–12 MiB 可行性，不降低安全性 |
| 首次启动时间 | CI 记录 p50/p95 | 同一硬件多次测量，区分冷热启动 |
| 空闲内存 | 记录私有工作集/总工作集 | WebView2 子进程计入完整应用实际占用 |
| 下载吞吐 | 对同后端同文件对照 | 禁止通过规避服务端限额“优化”指标 |
| 下载恢复 | 断电/kill/断网均恢复 | P0 强约束 |

编译优化基线：`opt-level = 's'/'z'` A/B、LTO、strip、限制 feature 和静态资源、避免重复 TLS/HTTP runtime；必要依赖不因为变大就随意移除。

## 9. 接口与测试

接口详见 `03_CONTRACTS.md`；开发安排详见 `05_ROADMAP_AND_TESTS.md`。第一阶段所有后端行为都可由 Mock 注入 401/403/404/429、超时、配额、磁盘已满、脏任务、文件改名来做完全离线测试。

## 10. ADR（需要决策）

- ADR-0001：选 Tauri v2 + Rust Core，Windows x64 先行（暂定）。
- ADR-0002（2026-11-08 替代旧决定）：完全移除官方 CLI，主连接 rust_compat + 原生登录，见 API_ADAPTER.md。
- ADR-0003：Piko 参考/复用路径及 NekoPak 许可证需要 Gate 0 先决定，不能强行兼容不同许可（待定）。
- ADR-0004：同步先只读/单向；双向删除默认关闭（已确定）。
- ADR-0005：Windows Cloud Files 与 WinFsp 为不同功能模块（已确定）。
