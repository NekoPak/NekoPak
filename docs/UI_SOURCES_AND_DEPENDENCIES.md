# UI 来源、素材与依赖许可证

核对日期：2026-11-07，2026-11-08 复核　范围：`src/`、`public/`、`index.html`、`tests/`、`scripts/`（Opus 5.5 负责的前端部分）

## 1. 运行时依赖（进入发布包）

| 包 | 版本 | 许可证 | 用途 | 体积依据 |
|---|---|---|---|---|
| preact | 10.27.2 | MIT | 组件渲染与 hooks | 与 React 相同的写法，gzip 约 4.5 KB；React + ReactDOM 约 45 KB |
| @tauri-apps/api | 2.9.0 | Apache-2.0 OR MIT | `invoke`、事件监听、窗口控制 | 仅 `core`、`event`、`window` 三个模块，按需动态加载（约 5 KB gzip）；浏览器 / Mock 模式不加载 |

没有使用 UI 组件库、状态管理库、CSS 框架或字体文件；图标只提取了少量路径数据，见 1.1。

### 1.1 构建前提取 / 生成的素材（进入发布包，但不含原始依赖）

| 来源 | 版本 | 许可证 | 用法 |
|---|---|---|---|
| Material Symbols Rounded（`@material-symbols/svg-400`） | 0.48.1 | Apache-2.0（Google） | `scripts/gen-icons.mjs` 只提取用到的 120 个图标的 SVG 路径到 `src/components/icons.generated.ts`，文件头保留版权与许可证说明；不打包整套图标或字体 |
| material-color-utilities（`@material/material-color-utilities`） | 0.4.0 | Apache-2.0（Google） | 仅构建期运行：`scripts/gen-theme.ts` 用 TonalSpot 算法从 NekoPak 自选的 6 个种子色生成 `src/styles/schemes.css`；运行时不包含该库 |

## 2. 开发依赖（不进入发布包）

| 包 | 版本 | 许可证 |
|---|---|---|
| vite | 7.1.12 | MIT |
| vitest | 3.2.4 | MIT |
| typescript | 5.9.3 | Apache-2.0 |
| playwright（E2E，可选，未写入 package.json） | 环境预装 | Apache-2.0 |

传递依赖（esbuild、rollup 等）同样只在构建期使用。发布前应由 Astra 的许可证扫描（`cargo deny` + npm 许可证清单）统一核对。

## 3. 素材

| 素材 | 来源 | 说明 |
|---|---|---|
| 图标（`src/components/Icon.tsx`） | Material Symbols Rounded 路径数据 | 见 1.1；`Icon.tsx` 只负责渲染，v1 的自绘线性图标已全部替换 |
| 猫箱标志与空状态插图（`src/components/Brand.tsx`） | NekoPak 原创绘制 | 刻意与 PikPak 官方标志无关 |
| 字体 | 系统字体栈 | Segoe UI Variable / Segoe UI / 微软雅黑等，未嵌入任何字体文件 |
| 截图（`docs/screenshots/`） | 本仓库 Mock 构建的真实运行截图 | Linux 容器中的无头 Chromium，字体与 Windows 不同 |

## 4. 与参考项目的关系（前端部分）

| 项目 | 许可证 | 前端实际关系 |
|---|---|---|
| Piko | AGPL-3.0-or-later | **界面设计参考（v2 视觉）**。参考了仓库 `d0bd8fe93573e8484c7db7616a41cbfcd29aa395` 中公开的截图（`docs/screenshots/*.jpg`）、界面规范文档（`ui/AGENTS.md`、`ui/.../screens/drive/AGENTS.md`）与外框几何常量（`ui/.../theme/Frame.kt`、`Shape.kt` 中的圆角与间距数值）：圆角岛外框、“浅色面以圆角压在深色面之上”、侧栏“文件 / 任务”连体切换、命令栏最右端主操作、任务页分组与定宽进度条、设置页卡片行密度、右键菜单快捷图标行。**没有复制任何 Kotlin/Compose 代码、图标、图片、字体或配色表**：前端是 Preact + CSS 的独立实现，配色由 material-color-utilities 从 NekoPak 自选种子色重新生成。 |
| PikPak Enhancement Master | AGPL-3.0 | 未参考代码。批量操作、复制路径等便利功能是常见的文件管理器交互。 |
| Piko（v3 功能对照） | AGPL-3.0-or-later | 只参考 README 与公开截图中的**功能清单**（片段截取、批量重命名、查找重复、链接预览等），交互与实现都是独立编写；对照表见 `docs/FEATURE_PARITY.md`。 |
| pikpaktui（`Bengerthelorf/pikpaktui@68b7a6b`） | Apache-2.0 | 前端未使用。`docs/BACKEND_API_ADAPTER.md` 以它为首选参考，整理 PikPak 接口路径与参数，供 Astra 实现 `rust_compat`。 |
| rclone `backend/pikpak`（`rclone/rclone@82cb84b`） | MIT | 同上，只用于核对接口行为（文件、分享、云下载、解压）。 |
| pikpakcli（`52funny/pikpakcli@560f263`） | MIT | 同上。 |
| PikPakAPI（`Quan666/PikPakAPI@c233bd3`） | GPL-3.0-only | 只读核对接口名称，**不得复制代码**；适配说明中没有摘录它的源码。 |
| 官方 PikPak CLI | — | 2026-11-08 起不再作为后端，界面不再出现。 |
| pikpak-kotlin / MediaMP / Rclone / WinFsp | 见 `docs/04_SOURCES_LICENSES.md` | 前端未使用其任何代码；仅在“关于与致谢”页列出。 |

因此前端目录目前**不含任何 AGPL 源码片段**；Apache-2.0 素材（Material Symbols 路径数据）已在生成文件头保留声明，发布包需附带 Apache-2.0 许可证文本。界面布局与交互模式本身属于设计参考，此判断不构成法律意见，发布前请按 `docs/04_SOURCES_LICENSES.md` 再次核对。

## 5. 建议并入 `THIRD_PARTY_NOTICES.md` 的条目（由 Astra 维护）

```text
Source-Project: preactjs/preact
Source-URL: https://github.com/preactjs/preact
Source-Commit: npm 10.27.2
Reuse-Type: runtime-dependency
License: MIT
NekoPak-Paths: src/** (import)
Notice-Status: pending (bundle the MIT license text with the release)

Source-Project: tauri-apps/tauri (@tauri-apps/api)
Source-URL: https://github.com/tauri-apps/tauri
Source-Commit: npm 2.9.0
Reuse-Type: runtime-dependency
License: Apache-2.0 OR MIT
NekoPak-Paths: src/lib/transport/tauri.ts (import)
Notice-Status: pending

Source-Project: google/material-design-icons (Material Symbols)
Source-URL: https://github.com/google/material-design-icons
Source-Commit: npm @material-symbols/svg-400 0.48.1
Reuse-Type: code-adapted (SVG path data of 120 glyphs, extracted by scripts/gen-icons.mjs)
License: Apache-2.0
NekoPak-Paths: src/components/icons.generated.ts
Notice-Status: preserved in file header; LICENSE text to be bundled

Source-Project: NihilDigit/piko
Source-URL: https://github.com/NihilDigit/piko
Source-Commit: d0bd8fe93573e8484c7db7616a41cbfcd29aa395
Source-Paths: docs/screenshots/*, ui/AGENTS.md, ui/src/commonMain/kotlin/dev/piko/ui/theme/Frame.kt, Shape.kt
Reuse-Type: design-reference
License: AGPL-3.0-or-later
NekoPak-Paths: src/styles/*.css, src/components/Frame.tsx, src/app/Shell.tsx (independent implementation)
Modifications: n/a (no code copied)
Notice-Status: credited in About page
```
