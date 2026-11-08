# NekoPak 上游参考、开源来源与许可证清单

核对日期：2026-11-07。此表是**研发追溯清单**，不是宣称 NekoPak 已使用这些代码。发布时必须逐条更新“实际使用状态”、精确提交 SHA 与各依赖许可证。

## 1. 逐项来源（所有已提及的项目均纳入）

| 项目 | 链接 | 查到的许可证 | 在 NekoPak 的建议角色 | 使用方式与约束 |
|---|---|---|---|---|
| **Piko** | <https://github.com/NihilDigit/piko> | **AGPL-3.0-or-later**（README 提示 ≤1.1.0 曾用 MIT） | **功能/UX 设计参考，或选择 Fork 路线时的上游** | 其新版本代码如被复用，应遵守 AGPL；不要复制新版组件后以 Apache 许可证发布 |
| **pikpaktui** | <https://github.com/Bengerthelorf/pikpaktui> | **Apache-2.0** | **Rust 核心优先审计的代码来源** | 可按条款复用模块，保留版权及许可证、必要 NOTICE、修改说明；不要直接打包全套 TUI |
| **52funny/pikpakcli** | <https://github.com/52funny/pikpakcli> | **MIT** | 接口与 CLI 实现研究 | Piko README 明确称接口实现参考本项目；复用源码须保留 MIT 许可声明 |
| **PikPak Enhancement Master** | <https://github.com/digbug82/PikPak_Enhancement_Master> | **AGPL-3.0** | 网页增强/文件操作功能参考 | 仅研究功能与交互；若复制实际 JS/代码需认真处理 AGPL |
| **Piko pikpak-kotlin SDK** | <https://github.com/NihilDigit/pikpak-kotlin> | **MIT** | Kotlin 原有行为与边界条件参考 | 不作为 Rust 运行时依赖；Piko 目前依赖其 Kotlin SDK |
| **MediaMP** | <https://github.com/open-ani/mediamp> | **Apache-2.0** | 播放交互与零拷贝渲染设计参考 | 属于 Kotlin/Compose 播放方案，不计划移植整套依赖；Piko README 表明 Windows 零拷贝渲染借鉴此项目 |
| **Rclone** | <https://github.com/rclone/rclone> | **MIT** | 同步、挂载、PikPak 后端行为参考；可选同步/挂载扩展 | 不参与普通浏览和下载，不捆绑 Rclone，兼容需检测版本，不能将其内置作为必备黑盒 |
| **WinFsp** | <https://github.com/winfsp/winfsp> | GPLv3 + FLOSS 专项例外 | Windows `P:` 虚拟盘符可选依赖 | 需要用户安装驱动/运行时；核查具体 DLL 连接与二进制再分发条件、声明版权所有者 |
| **Microsoft Cloud Files API** | <https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot> | Windows 平台 API，受微软平台条款约束 | 仅 Windows 按需文件与同步根 | `CfRegisterSyncRoot`、占位文件等，**不等价**于独立盘符 |
| **Tauri v2** | <https://tauri.app/> | 依各构件实际许可证核对 | UI Shell / IPC / 打包 | 靠系统 WebView2，仍需按官方指引处理运行时缺失情形 |
| **PikPak 官方 CLI** | <https://mypikpak.com/en-US/cli> | 未确认再分发许可 | 历史研究，2026-11-08 已移除后端 | 不执行、不安装、不分发；历史证据位于 history/ |
| **PikPak WebDAV** | <https://mypikpak.com/en-US/help-center/connected_apps/webdav/how_to> | 服务功能，不是源码许可证 | Premium 账户可选连接方式 | 启用需要 WebDAV 凭据，配额与其他 Connected Apps 共享 |
| **OpenList**（研究候选） | <https://github.com/OpenListTeam/OpenList> | **待按使用提交核对** | PikPak 驱动与错误处理对照研究 | 尚未决定复用；不得从研究候选直接复制代码 |

**备注：** 部分开源库带有多许可证/额外依赖、NOTICE 或字体/媒体资源等独立权利要求，`license` 字段仅表示仓库整体声明，不能代替完整的依赖许可证扫描。

## 2. Piko 已公开声明的上游借鉴关系

根据 Piko README 的“许可与致谢”部分：

- PikPak 接口实现**参考** `52funny/pikpakcli`；
- 多项功能设计**参考** `digbug82/PikPak_Enhancement_Master`；
- Windows 端 mpv 零拷贝渲染**由** `open-ani/mediamp` 实现；
- 桌面为 Kotlin Multiplatform/Compose；构建依赖 `io.github.nihildigit:pikpak-kotlin` 等。NekoPak **不**计划保留 JVM/Compose/MediaMP 作为发行时必需组件。

这些是 Piko 的来源关系，并不意味着 NekoPak 已经直接引用其中任何一段源码。NekoPak 必须单独记录自己的来源关系。

## 3. 两种开发路线：**先选一条，勿混用**

### 路线 A：直接 Fork Piko 并以其代码为基础改写

- 保留历史、版权、原仓库说明、AGPL-3.0-or-later 许可证和适用的源码/分发义务；不能仅因改用 Rust 就自动重授权成 Apache。
- 即使删掉 JVM 运行时，依赖 Piko 的非独立改编代码仍可能属于衍生作品。
- 如果选择本路线，**NekoPak 应按 AGPL-3.0-or-later 管理**，除非有合法明确的替代授权；仍可做到极小体积。
- 优点：GitHub Fork 溯源一目了然；缺点：平台/语言迁移极大、历史技术栈和许可证边界复杂。

### 路线 B：**建议**新建独立 Rust 主仓 + Fork Piko 作为只读研究镜像

- NekoPak 独立开发 UI、Rust 状态机与文件系统集成；把 Piko 的行为、交互与已公开设计用作规范参考，不直接复制 AGPL 实现。
- 从 `pikpaktui`（Apache-2.0）等宽松许可项目按需借用 Rust 代码，保留著作权声明、许可证、修改记录；从 MIT 项目引用代码同样保留原有 MIT notices。
- 主仓可**考虑** Apache-2.0 或 MIT，但最终仅在来源审计后决定；如果后来加入 AGPL 内容须重新评估分发义务。
- 优点：技术边界清晰、瘦身彻底；缺点：需要自己完成集成/测试，不是简单换皮。

**推荐执行：** 先采用 B；如果用户特别希望 GitHub 页面显示为 Piko 的真正 Fork，则切换 A 并自觉遵守 AGPL。两条路线都是允许公开贡献的路径，但授权义务不同。

## 4. 每次参考/复用必须有溯源记录

在 PR 描述和 `THIRD_PARTY_NOTICES.md` 至少填：

```text
Source-Project: Bengerthelorf/pikpaktui
Source-URL: https://github.com/Bengerthelorf/pikpaktui
Source-Commit: <精确 SHA>
Source-Paths: src/pikpak/download.rs (示例)
Reuse-Type: code-adapted | behavior-only | design-reference | external-tool
License: Apache-2.0
NekoPak-Paths: crates/nekopak-provider-native/src/download.rs (示例)
Modifications: async conversion, extraction from CLI, error normalization
Notice-Status: preserved / pending
Reviewed-by: <reviewer>
```

- `design-reference` 不等于 `code-adapted`；别错误声称自己“完全原创”，也别把参考项目全部写成直接代码作者。
- 任何复制代码的文件头应在必要时保留上游版权注记，不以 README 的一行链接替代源码许可义务。
- 所有发行包均包含许可证与 NOTICE；若含 GPL/AGPL 组件，遵守相应源代码提供和再分发要求。
- 不使用未经授权的 PikPak 商标或相似官方图标；NekoPak 明确写“Unofficial, community-developed”.

## 5. 正式 README 致谢段建议

> NekoPak 是独立开发的社区客户端，非 PikPak 官方产品。项目研究参考了 Piko（NihilDigit）、pikpaktui（Bengerthelorf）、pikpakcli（52funny）、PikPak Enhancement Master（digbug82）、MediaMP 与 Rclone 等开源项目。各项目代码及素材归各自权利人所有。具体直接复用模块、许可证和修改记录见 `THIRD_PARTY_NOTICES.md`。本声明不暗示上述项目作者为 NekoPak 背书。

上述是**初稿**；发布时区分实际借用代码、设计参考及第三方依赖，及时维护清单。

## 6. 官方来源核验

- Piko README（许可与致谢）：<https://github.com/NihilDigit/piko/blob/main/README.md>
- pikpaktui Cargo manifest：<https://github.com/Bengerthelorf/pikpaktui/blob/main/Cargo.toml>
- WinFsp 许可证全文及例外：<https://github.com/winfsp/winfsp/blob/master/License.txt>
- PikPak 官方 CLI：<https://mypikpak.com/en-US/cli>
- PikPak Connected Apps：<https://mypikpak.com/en-US/connect-apps-faq>

**非法律意见：** 许可证合规最终依实际源码、构建和发行材料核查。

## 7. 用户确认的第三方 API 研究范围（2026-11-07）

第三方/社区客户端协议是主适配路线；官方 CLI 已完全移除。已核对 Piko v1.4.0、
pikpak-kotlin `8eaf5587a883ed4ea9bcca7b43a9194118719e63`、pikpaktui
`68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`、Rclone
`82cb84b2014bec6a167dc28eb53233b2d4abe1b3` 的行为/协议资料。未复制实现。
具体证据在 UPSTREAM_PARITY_RESEARCH.md，真实授权/协议验收仍逐适配器完成。
