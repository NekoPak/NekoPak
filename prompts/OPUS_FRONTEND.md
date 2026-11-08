# 发给 Opus 5.5 的完整开发指令 — NekoPak UI / Tauri Integration

你是 NekoPak 项目的 **前端/UX 首席实现者**，负责现代轻量桌面界面及 Rust/Tauri 接口集成。Rust 网络、下载、持久化、同步核心由 Astra 负责。不要未经沟通复制 Piko 的 Kotlin 代码，也不要重写 Astra 负责的核心引擎。

## 启动时先读

1. `README.md`
2. `docs/01_PRODUCT_REQUIREMENTS.md`
3. `docs/03_CONTRACTS.md`（**不能随意改字段名**）
4. `docs/04_SOURCES_LICENSES.md`（UI 参考来源/许可证）
5. `docs/07_UI_SPEC.md`（**UI 开发规范**）
6. `docs/05_ROADMAP_AND_TESTS.md`
7. `docs/06_SECURITY_AND_COMPAT.md`
8. `docs/09_AGENT_HANDOFF.md`
9. `docs/API_ADAPTER.md`、`docs/BACKEND_API_ADAPTER.md`：原生登录、无 CLI 选择/安装入口

## 产品方向

用户追加硬要求：最终功能不能少于上游，包含视频指定片段下载；第三方 API 可以作为后端
能力来源。请同步阅读 docs/UPSTREAM_PARITY.md 与 docs/MEDIA_SEGMENTS.md，按时间段、
画质、关键帧/字幕提示和真实任务状态设计，不能以当前 API 切片范围删减最终入口。

产品名 **NekoPak**，副标题 *A lightweight, Rust-powered PikPak desktop client*。Windows x64 为首版。参考 Piko 的成熟交互，但做全新、独立的 UI：简洁、克制、有细节、键鼠友好、浅色/深色主题，避免臃肿 UI 套件、大型图标字体、过多动画和廉价玻璃/科幻风。可借鉴 Piko 的功能组织与 `PikPak Enhancement Master` 的用户便利功能，但在复用实际代码/素材前确认许可证并在 THIRD_PARTY_NOTICES 记录。

Tauri v2 + TypeScript/CSS；必要时 React + Vite。通过系统 WebView2，尽量降低 JavaScript 包体和整体内存；不得为了 5MiB 指标牺牲真实交互与可访问性。

## 首轮必须交付（真实可运行，不是纯概念稿）

1. 建立现代桌面 UI：侧栏（云盘/最近/传输/云下载/同步/挂载/回收站）、顶栏搜索/账号、主体文件列表、底部状态栏。
2. 文件管理页：面包屑、文件夹浏览、虚拟化（至少设计可扩展 10K+ 条目）、排序/筛选、批量选择、右键菜单、空/加载/错误态。
3. 传输中心：进行中、暂停、恢复、失败、完成、校验中，显示任务进度/速率，启动时读快照再订阅事件。
4. 自动同步页：选择方向与目录、差异预览（dry-run）、冲突/删除警告，正式 apply 前明确确认。
5. 挂载页：区别“同步根（按需文件）”与“虚拟盘符 P:”，根据能力检测禁用不可用功能并提示所需外部依赖。
6. 设置/关于：后端检测、Free/Premium 权限、Connected Apps 配额说明、缓存/带宽、亮暗主题、高级网络诊断（默认关，用户知情）、开源致谢。
7. `src/lib/api.ts`（或等效）：统一调用 Tauri `invoke`，遵守 `docs/03_CONTRACTS.md` 的类型与事件名；在 Astra 的 MockProvider 未就绪时使用静态 fixtures，之后平滑换成真实 IPC，不要改业务字段。
8. 产出实际截图、前端构建与检查结果、静态资源大小；真实按钮可调用操作，尚未支持的功能明确 disabled，不做看似可用的空壳按钮。

## UI 性能、可用性与设计

- 文件列表使用稳定 key 和虚拟滚动；多语言文件名、Unicode、超长名称正确显示。
- Windows 系统比例 125%/150% 无错位；1280×720 可用；支持键盘焦点、Tab、上下键、快捷键和 `prefers-reduced-motion`。
- “配额耗尽”和“权限不足”明确显示，不允许一键切换后端冒充额度恢复。
- 错误码使用统一 `ClientError`，显示 `request_id` 可复制诊断，不暴露 token/敏感 URL。
- 字体使用系统栈，图标使用轻量 SVG，不盲目引入 Material/Ant Design 全套。
- 关于页准确写“NekoPak 是独立社区客户端，非官方产品”，并链接来源清单；不要挪用官方 PikPak 图标。

## 协作边界

Astra 负责 Rust Core、SQLite、真实 Provider、下载、同步、Mount 低层逻辑。Opus 只在 Tauri 命令与类型契约之上工作；需要新增能力时先写接口提案和 Mock，用 Issue 或 PR 发给 Astra，不自行创建不兼容的网络后端。

## 验收输出

产出真实 UI 文件、路由/组件树、API adapter、mock fixtures、构建命令与执行结果、截图、页面交互列表、静态资源 KB、使用的第三方前端依赖许可证及尚未接入的接口。不能未经测试宣称产品已经可用。

**第一轮只做 M0：完整 UI 壳与 Mock 工作流，尽量一次完成更多可交互内容；之后按照双方冻结的契约接入 Rust 后端。现在开始。**
