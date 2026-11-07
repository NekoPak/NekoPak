# NekoPak

> A lightweight, Rust-powered PikPak desktop client.
>
> 独立社区项目，非 PikPak 官方客户端，与 PikPak 服务提供者无隶属关系。

**状态：** `M1 slice / CLI read-only preview`（默认离线 Mock；已增加显式启用的官方 CLI 只读适配器，真实授权目录仍待验证）  
**文档基线：** 2026-11-07  
**首要平台：** Windows x64；后续 Linux x64 / macOS；Android 不列入首版承诺。

NekoPak 的目标不是再包装一个网页，而是构建具有原生 Rust 核心的轻量云盘管理工具：文件管理、云下载、分段下载、可靠同步、按需缓存、可选的文件系统挂载。UI 采用 **Tauri v2 + 轻量 TypeScript/CSS**，利用 Windows 已有 WebView2；不捆绑 JVM、Chromium、全套 mpv/FFmpeg、Rclone 或 WinFsp。

## 设计承诺

1. **轻量而可靠：** Windows x64 Lite 的压缩发布包以 `≤ 5 MiB` 为**挑战目标**，绝非验收前的性能承诺；优先保证安全、数据完整性与下载可靠性。
2. **先验证，再复用：** 支持多后端/能力协商。官方 CLI 可以作为可选外部程序适配器；社区 Rust 代码只在许可证核查和测试后使用。不可凭想象依赖不存在的公开 REST API。
3. **默认不破坏数据：** 先只读浏览和单向备份，再做双向同步。远端不可达、令牌失效、元数据不全时禁止判定文件“已删除”。
4. **可选扩展：** 挂载、媒体播放、Rclone、WebDAV、实验性网络兼容均不计入最小基础版；只有能力检测成功后才启用。
5. **公开致谢：** README 及 `THIRD_PARTY_NOTICES.md` 明确注明参考、复用、外部依赖的来源和各自许可证。
6. **用户知情：** 不自行修改系统 Hosts、不暗中绕过平台地域或风控校验、不收集账户密码，风险选项明确显示并默认关闭。

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

- NekoPak 不是 PikPak 官方授权的软件；官方 CLI 的存在，不自动授予其二进制再分发许可，也不证明通用公开 REST API 已开放。
- PikPak 已连接应用的云下载、下行、上传配额分别计算；已连接应用共享对应月度总配额 25% 的上限。免费账号已连接应用每日下行上限为 5 GB，达到限制后的具体行为以官方说明为准。
- 文件系统挂载与“同步文件夹”是两种不同机制。Windows Cloud Files API 提供同步根目录和占位文件，**不等于独立的 `P:` 盘符**；WinFsp 类驱动才能提供通常意义的独立挂载盘。
- 任何性能、体积指标必须附构建 commit、配置、平台、测量步骤和测试数据。

## 主要参考链接

- Piko: <https://github.com/NihilDigit/piko>
- pikpaktui: <https://github.com/Bengerthelorf/pikpaktui>
- PikPak CLI: <https://mypikpak.com/en-US/cli>
- PikPak Connected Apps FAQ: <https://mypikpak.com/en-US/connect-apps-faq>
- Rclone PikPak: <https://rclone.org/pikpak/>
- Tauri Windows WebView2: <https://tauri.app/zh-cn/distribute/windows-installer/>

进一步参考请见 `docs/04_SOURCES_LICENSES.md`。**未经许可证核查请勿复制第三方源码。**

## 当前后端交付

运行与打包见 [docs/RUNNING.md](docs/RUNNING.md)，实现边界与前端交接见
[docs/IMPLEMENTATION_NOTES.md](docs/IMPLEMENTATION_NOTES.md)。Rust Core 可独立测试，
Tauri 默认验收壳展示离线样本。[官方 CLI 只读预览](docs/CLI_ADAPTER.md)已实现并完成
无账号实测。应用内登录、真实下载字节、同步和挂载仍未实现。

```powershell
npm ci
cargo test --workspace --locked
npm run check
npm test
cargo run -p nekopak-app --locked
```

本轮选择独立 Rust 仓库、MIT 许可。参考项目与实际依赖分别记录于
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。Bern 作为协议/引擎/宿主分层参考，
没有复制其源码，也没有引入 Bern 运行时。
