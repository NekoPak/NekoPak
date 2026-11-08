> 导入的 v3 前端作者报告，仅作为交接资料。下文历史测量不是当前 Windows 构建结果；当前验证见 API_MIGRATION_REPORT.md，真实功能门禁仍按 UPSTREAM_PARITY.md。

# 前端 M0 交接报告（Opus 5.5）

日期：2026-11-07，2026-11-08 更新 v3　分支：`feat/ui-shell`　契约：v0.1 基线 + 提案 UI-001 §P0–§P23（全部增量，未改已有字段）

M0 目标：完整 UI 壳 + Mock 工作流，全部按钮要么真实调用（Mock 或 IPC），要么禁用并写明原因。本报告按 `docs/09_AGENT_HANDOFF.md` 的模板填写，所有数字都来自实际执行。

---

## v3：功能补齐，去掉 CLI，应用内登录（2026-11-08，按用户反馈）

**用户的决定**：官方 CLI 太重，不再作为后端；主攻 PikPak 接口（`rust_compat`）。界面不展示连接方式名称，不放免责声明式的说辞；登录必须是应用内表单，不弹网页窗口。后端适配说明已经写给 Astra：`docs/BACKEND_API_ADAPTER.md`，`prompts/ASTRA_BACKEND.md` 和 `AGENTS.md` 开头都指向它。

**界面变化**

- 侧栏最上方是单独的“下载”按钮（显示速度、进行中数量和需处理数量），原来的“文件 | 任务”切换去掉了。
- 去掉了后端列表、“后端”设置页和“Mock / CLI / 直连”等字样；“演示”标记只在浏览器预览中出现。
- 登录（`src/pages/Auth.tsx`）：账号、密码（可显示 / 隐藏）、行内错误提示，以及人机验证和短信 / 邮箱验证码两种后续步骤。密码只传给核心一次，请求结束后立即清空。
- 文案清理：挂载页、关于页、配额与错误提示中的 CLI、路线图编号、Connected Apps 和“不会……”一类说辞都已删除。

**新增功能**（逐项对照 Piko 见 `docs/FEATURE_PARITY.md`）：片段下载（预览画面、双滑块、±1 秒、关键帧对齐提示）、指定画质下载和默认画质、文件夹下载、星标与星标页、命令面板（`Ctrl+K`）、批量重命名、查找重复、空间分析（矩形树图）、压缩包浏览与云端解压、分享与“我的分享”、添加链接前预览并挑选文件、外部播放器、拖放移动 / 复制、剪切 / 粘贴、框选、`Ctrl+滚轮` 调整网格大小、撤销（`Ctrl+Z`），以及下载设置（默认画质、每个任务的连接数）。

**新增接口**：全部是增量提案 §P13–§P23，见 `docs/CONTRACT_PROPOSAL_UI_001.md` 第二批。

**代码拆分**：新页面和对话框按需加载（`src/app/lazyPage.tsx`、`src/pages/lazy.ts`），启动时加载的 JS 只比 v2 多约 9 KB gzip。

**截图检查中发现并修复的问题**

- 查找重复页的分组卡片被压扁，看不到组内文件。原因是滚动网格把行高压缩到容器高度；改为 `grid-auto-rows: max-content`，另外三个滚动列表也做了同样的修改。
- 片段面板默认只选中 60 秒，长视频上两个滑块重叠；现在默认选中全片。
- 片段面板的“下载片段”按钮被挤出可视区域；现在固定在面板底部。
- 下载按钮的数量角标原来是红色，看起来像错误；现在用中性色，只有失败且没有速度时才用错误色。
- 添加链接面板中，部分选中的类型按钮看起来像未选中；现在显示“14/15”，并设置 `aria-pressed="mixed"`。

**v3 验证（实际执行）**

| 检查 | 结果 |
|---|---|
| `tsc --noEmit` | 0 错误 |
| `vitest run` | **63 / 63**（新增 `features-pure`：片段时间换算、批量重命名冲突检查、矩形树图、模糊匹配；新增 `mock-features`：登录、片段 / 画质、文件夹下载、星标、查重、空间、压缩包、分卷、链接预览、分享、下载设置） |
| E2E | **47 / 47**；离开 localhost 的请求 0；4 种缩放 × 浅色 / 深色，13 个页面均无横向溢出 |
| 体积（gzip） | 桌面版启动加载 **106.4 KB**（JS 85.3 + CSS 15.3 + Tauri 模块 5.0 + HTML 0.8）；新页面每个 1.5–5.1 KB，打开时加载；演示数据块 17.8 KB 只在浏览器预览中加载。详见 `docs/ui-size-report.md` |

**仍然是限制**：所有新功能在桌面版都要等后端实现对应命令，见 `FEATURE_PARITY.md` 中标为“等后端”的项目。PikPak 的验证码流程和压缩包列表接口还没有实测（适配说明中标为“待验证”）。依然没有在 Windows / WebView2 中运行过。

---

## v2 视觉重做（同日，按用户反馈）

第一版界面被评价为“一看就是 AI 随手写的”。v2 推倒视觉层重做，业务逻辑、Mock、契约与测试逻辑保持不变。

**参考来源**：Piko（`NihilDigit/piko@d0bd8fe`）公开截图与 `ui/AGENTS.md` 设计规范，作为**设计参考**，未复制其 Kotlin/Compose 代码或素材（详见 `docs/UI_SOURCES_AND_DEPENDENCIES.md`）。

**主要变化**

- **窗口外壳**：自绘标题行（后退 / 前进 / 上一级、胶囊地址栏、搜索、最小化 / 最大化 / 关闭），整窗为外框底色，内容是一块 16 px 圆角的画布，岛间距 8 px，较浅的面以圆角压在较深的面之上。浏览器预览中窗口按钮为禁用态并说明原因；桌面端调用 `@tauri-apps/api/window`（需要 §P12 中的 capability）。
- **侧栏**：顶部“文件 | 任务”连体切换，任务按钮第二行实时显示下载速率、待处理数或“已限速”；快速访问只列用户固定的文件夹（右键“固定到快速访问”）；底部账号行显示今日剩余下行配额；`Ctrl+B` 收起为窄轨，窗口宽度不足 1000 px 时自动窄轨。
- **文件页**：地址栏可点击各级路径，下拉列出最近访问位置；命令栏为“新建 ▾ / 排序 / 类型筛选 / 全选”，最右端主操作“添加链接”；多选时左侧替换为“退出多选 + 已选计数 + 批量操作”，位置不变、不重排。新增**网格视图**（同样虚拟化）。列表前导类型色块在悬停时变为勾选框，文件名不发生位移。
- **右侧面板**：“添加链接”和“属性”改为右侧非模态面板，列表仍可操作。
- **右键菜单**：顶部快捷图标行（下载 / 复制路径 / 重命名 / 移动 / 复制），其后是分组列表，“属性”单独一组，“移至回收站”置底并用错误色。
- **任务页**：标题行放类别胶囊（全部 / 下载 / 上传 / 云下载，原“云下载”页并入此处），按“需要处理 / 进行中 / 已完成 / 已取消”分组，按列对齐名称、状态、定宽进度、容量、速率、目标位置；底栏显示上下行速率、限速开关（直接写入 `bandwidth_limit_kib_s`）、预计剩余时间与下行配额条。
- **设置页**：左侧分类，右侧 4 px 间隔的卡片行（图标 + 标题 + 说明 + 控件）；“外观”新增 6 种强调色与“减少动画”；“关于”并入设置。
- **配色与图标**：用 Google `material-color-utilities` 从 NekoPak 自选的 6 个种子色生成 Material 3 TonalSpot 浅 / 深配色（`npm run gen:theme`）；图标改为 Material Symbols Rounded，只提取用到的 86 个路径（`npm run gen:icons`），常态描边、选中态实心。
- **首次启动**：左侧品牌区位于外框底色上，右侧表单岛浮于其上。

**修复的真实问题（E2E 与截图检查中发现）**：总大小未知时进度条显示为满格红条；选择后命令栏换行导致右键菜单落到上一行；网格卡片文件名在扩展名中间断行；菜单容器多画了一圈焦点框；文件右键菜单出现“查看属性”和“属性”两个重复项。

**v2 验证（实际执行）**：`tsc` 0 错误；`vitest` 34/34；E2E 32/32，连续两次，离开 localhost 的请求 0；四种缩放 × 浅 / 深色无横向溢出。

**v2 体积**（gzip）：始终加载 89 KB（JS 76 + CSS 12.5 + HTML/脚本 0.8），桌面端额外按需加载 Tauri 模块约 5 KB，Mock 块 11 KB 仅浏览器预览加载。比 v1 增加约 25 KB，主要来自图标路径数据（68.6 KB 原始，gzip 后 13.6 KB）和新增样式。下面“体积”一节的表格是 v1 数字，以 `docs/ui-size-report.md` 为准。

---

## 完成

### 运行与构建命令

```bash
npm ci                 # 安装依赖（锁定版本）
npm run dev            # Vite 开发服务器 http://localhost:5173（浏览器中自动使用 Mock）
npm run check          # tsc --noEmit（strict）
npm test               # vitest 单元测试
npm run build          # tsc + vite build → dist/
npm run e2e            # 构建产物上的 Playwright 端到端测试（需要 playwright + chromium）
npm run size           # 静态资源体积报告 → docs/ui-size-report.md
```

在浏览器打开时可用 `?scenario=free|premium|unauthorized|quota_exhausted|network_down|huge_dir|name_conflict` 指定 Mock 场景，也可以点状态栏右下角“Mock 数据”切换场景和故障注入。在 Tauri WebView 中自动改用真实 IPC（`VITE_NEKOPAK_TRANSPORT=mock|tauri` 可强制）。

### 测试结果（确实执行过）

环境：Linux x64 容器，Node 22.23.4，无头 Chromium（Playwright）。**不是 Windows / WebView2。**

| 检查 | 结果 |
|---|---|
| `tsc --noEmit`（strict, noUnused*） | 0 错误 |
| `vitest run` | **34 / 34 通过**（3 个文件：u64 与格式化、文件名校验、脱敏、事件对账、虚拟列表、路由、Mock 契约行为） |
| `vite build` | 成功 |
| E2E `tests/e2e/run.mjs` | **32 / 32 通过**，最终修改后连续三次；**离开 localhost 的网络请求：0**；控制台错误：0 |

E2E 覆盖（每项一个用例，详见 `docs/e2e-report.json`）：

- 首次启动：非官方声明 → 已连接（账号类型 + 配额维度）→ 进入云盘
- 未授权：外部授权的取消 / 网络失败 / 成功三种结果
- 文件：双击进入、面包屑、Backspace 返回、Shift/Ctrl 键盘多选、Ctrl+A、Esc；**选择时列表不发生位移**
- 右键菜单：Shift+F10 打开、方向键、Esc；属性中显示 `9,007,199,254,740,993 字节`
- 重命名：保留设备名警告、同名阻止、成功提交
- 新建文件夹（Ctrl+Shift+N）、移至回收站（需确认）、还原、永久删除（需勾选确认）
- 超大目录：12,000 项分页加载完毕，DOM 中渲染的行数 < 120
- `NOT_FOUND`：已删除文件夹显示原因并可返回上一级
- 下载：加入队列；`Report.pdf` / `report.pdf` 目标大小写冲突 → `CONFLICT`
- 传输：暂停 / 继续；乱序事件被丢弃（页脚计数）
- 故障：37% 断网 → 重试等待；完整性校验失败（保留 `.part`）；模拟重启后从快照恢复
- 云下载：无效链接禁止提交；提交后提示“提交成功不代表下载完成”；元数据未就绪显示“总大小未提供”，进度条不填充
- `QUOTA_EXCEEDED`：横幅、下载/云下载禁用并说明原因、列表仍可浏览、无“切换后端恢复额度”入口
- `TRANSIENT_NETWORK`：列表错误可重试、任务进入重试、同步扫描失败且不生成任何操作
- `AUTH_EXPIRED`：横幅、任务历史保留、重新授权后恢复
- 同步：首次同步无删除 → 执行；含删除的计划需勾选确认；未解决冲突时禁止执行
- 挂载：同步根与 `P:` 分开展示，前置检查未通过时两个入口都禁用
- 设置：主题切换；高级网络默认关闭、开启需确认、自定义 DNS 禁用；后端能力列表
- 关于：非官方声明与来源清单
- Tab 键可到达搜索框、侧栏、工具栏、文件列表
- 缩放：100% / 125% / 150% / 200% × 浅色 / 深色，9 个页面均无横向溢出
- 深色模式文字对比度抽查 ≥ 4.5:1

### 体积（`npm run size`，gzip -9 / brotli -11）

| 文件 | 原始 KB | gzip KB | brotli KB | 加载时机 |
|---|---:|---:|---:|---|
| assets/index-*.js | 165.30 | 58.92 | 49.14 | 始终 |
| assets/style-*.css | 40.76 | 8.48 | 7.56 | 始终 |
| assets/backend-*.js（Mock） | 31.50 | 11.21 | 9.54 | 仅 Mock 模式动态加载 |
| assets/core-*.js + event-*.js | 3.69 | 1.60 | 1.36 | 仅 Tauri 模式动态加载 |
| index.html + theme-init.js | 1.06 | 0.73 | 0.52 | 始终 |
| **合计** | **242.31** | **80.93** | **68.12** | |

Tauri 运行路径（不含 Mock 块）约 **69.7 KB gzip**。其中 Preact 约 4.5 KB，其余是界面代码与中文文案（UTF-8 每字 3 字节）。没有图片、字体或图标集文件。

---

## 结构

### 组件树

```text
src/
  main.tsx                      挂载 + 样式入口
  app/App.tsx                   路由出口、全局快捷键、全局横幅、首次引导、对话框/菜单/提示宿主
  app/Shell.tsx                 Topbar（搜索/账号/设置）· Sidebar（7 页 + 设置/关于）· StatusBar
  pages/
    Files.tsx                   文件页：分页加载、面包屑、工具栏、筛选/排序/行距、FileTable
    Lists.tsx                   搜索结果 · 最近 · 回收站
    Transfers.tsx               传输中心（三组标签 + 类型筛选 + 批量）· 云下载页 · TaskRow
    Sync.tsx                    配置列表 · 配置表单 · 扫描进度 · 计划预览 · 执行确认 · 历史
    Mount.tsx                   同步根 / 虚拟盘符两个区块与逐项检查
    Settings.tsx                通用 · 账号 · 后端 · 配额 · 传输与缓存 · 高级网络诊断 · 快捷键 · 开发者
    Auth.tsx                    后端选择 + 外部授权等待（从不收集密码）
    Onboarding.tsx              声明 → 连接账号 → 完成
    About.tsx                   非官方声明、来源与许可证、前端依赖
    DevPanel.tsx                Mock 场景与故障注入（仅 Mock）
    fileActions.tsx             幂等的 files_mutate 封装、下载、复制路径、右键菜单项
  components/
    FileTable.tsx               虚拟化 ARIA grid、选择模型、键盘导航、BigInt 排序
    dialogs.tsx                 确认 · 命名 · 文件夹选择 · 属性 · 新建云下载 · 快捷键
    hosts.tsx                   DialogHost（焦点陷阱/还原）· MenuHost（键盘可达）· ToastHost
    ui.tsx                      Btn · ErrorPanel · Empty · Progress · Badge · Field · Switch · Segmented
    Icon.tsx / Brand.tsx        原创 SVG 图标与标志
  lib/
    contract.ts                 03_CONTRACTS §2/§4 原样
    contract-proposals.ts       UI-001 提案类型 + 命令表 CommandMap
    api.ts                      唯一的后端入口：typed 命令、事件、超时、错误规范化
    transport/tauri.ts          @tauri-apps/api 懒加载
    mock/fixtures.ts, backend.ts  确定性 Mock（七组场景 + 故障注入）
    errors.ts                   ClientError 规范化、脱敏、文案、诊断文本
    format.ts / names.ts        u64(BigInt) 格式化、文件名校验与 Windows 兼容提示
    store.ts / virtual.ts       极简 store + 虚拟列表计算
  state/
    app.ts                      启动、能力、授权、配额、设置、能力门控 gate()
    transfers.ts / reconcile.ts 快照优先 + 事件缓冲回放 + revision 去重
    route.ts / ui.ts            hash 路由、提示/对话框/菜单
  styles/                       tokens · base · shell · components · pages
```

### 路由

`#/files[/<folder_id>]` · `#/search/<q>` · `#/recent` · `#/transfers[/done|/failed]` · `#/cloud` · `#/sync[/<config_id>]` · `#/mount` · `#/trash` · `#/settings[/general|account|backends|quota|transfers|network|shortcuts|developer]` · `#/about`

### 关键设计决定

- **能力门控**：每个操作按钮都经过 `gate(op)`：未登录 / 后端不可用 / 后端未声明该能力 / 配额已用尽，任一不满足就禁用并给出原因。禁用按钮仍可聚焦（`aria-disabled`），点击时以提示说明原因。
- **快照优先**：启动时先挂监听器并缓冲，再读 `transfers_list`，快照落地后回放缓冲事件并按 `revision` 丢弃旧事件；未知任务的事件触发重新读快照。
- **幂等**：每次用户意图一个 `idempotency_key`；从错误提示重试时复用。云下载对话框里每行链接各自保存 key，重复提交不会创建重复任务。
- **大数**：所有字节数用 BigInt 处理，排序、求和、百分比都不经过浮点。
- **不编造数据**：总大小未知时进度条为静态斜纹并写“总大小未提供”；速率缺失写“未提供”；配额 `unavailable` 写“不可获取”。
- **视觉**：低饱和冷灰 + 一个青绿强调色，系统字体，4px 基线，圆角 6/8/12。唯一的“签名”细节是当前页的猫耳形指示，以及状态栏顶边随本地传输总进度增长的细线。对话框、菜单、提示使用克制的半透明磨砂，其余实底。

---

## 页面交互清单

| 页面 | 可交互（→ 调用） | 禁用及原因 |
|---|---|---|
| 首次启动 | 继续（本地标记）· 选择后端 · 在浏览器中授权 → `auth_begin` · 取消 → `auth_cancel` · 重新检测 → `backend_discover` · 进入云盘 | 未安装/不可用的后端（显示 limitations）；WebDAV（需安全存储命令） |
| 云盘 | 进入/返回/面包屑 → `files_list` `files_path` · 新建文件夹 · 重命名 · 移动 · 复制 · 移至回收站 → `files_mutate` · 下载 → `transfer_start` · 新建云下载 → `cloud_task_create` · 属性 → `files_stat` · 复制路径 · 筛选 · 排序 · 行距 · 加载全部 | 上传（M1）；外部播放器（P1，`stream_url` 未提供）；文件夹下载（M0） |
| 搜索 / 最近 | `files_search` / `files_recent` · 打开所在文件夹 · 同云盘右键菜单 | 同上 |
| 回收站 | `trash_list` · 还原 / 永久删除（勾选确认）→ `files_mutate` | — |
| 传输 | 三组标签 · 类型筛选 · 暂停 / 继续 / 取消 / 重试 → `transfer_control` · 全部暂停/继续 · 重新读取快照 · 复制诊断 | 云下载不能暂停（服务端执行）；配额用尽时重试；“打开所在文件夹”（需 reveal 能力） |
| 云下载 | 新建 · 任务列表与云下载配额 | 配额用尽时新建 |
| 自动同步 | 新建/保存配置 → `sync_config_save` · 选择云盘目录 · 扫描 → `sync_scan` + 事件 · 计划 → `sync_plan` · 按操作筛选 · 执行（删除需勾选）→ `sync_apply` · 历史 → `sync_history` | 双向（M3）；首次同步开启删除；未解决冲突时执行；本地目录选择对话框（需 dialog 插件） |
| 挂载 | 重新检测 → `mount_capabilities` | 创建同步文件夹、只读挂载为 P:（前置检查未通过） |
| 设置 | 主题 · 行距 · 后端重新检测 · 配额刷新 · 传输与缓存保存 → `settings_update` · 高级网络开关（需确认）· 代理模式 · 诊断日志 · 退出登录 → `auth_logout` | 界面语言（仅简体中文）；清理缓存、连通性检查、导出诊断（命令缺失）；自定义代理（需安全存储）；自定义 DNS（未审计） |
| 关于 | 外部链接（Tauri 下需 opener，失败时复制链接） | 源码仓库地址（Gate 0 后填写） |

全局快捷键：Ctrl+F、Ctrl+1…7、Ctrl+,、F5、F1/?；列表内：方向键、Shift/Ctrl 组合、Home/End/PgUp/PgDn、Space、Enter、Alt+Enter、Backspace、Alt+↑、F2、Delete、Ctrl+A、Ctrl+C、Shift+F10/菜单键、Esc、Ctrl+Shift+N。帮助见 F1 或“设置 › 快捷键”。

---

## 截图

`docs/screenshots/`，均为本仓库 Mock 构建在无头 Chromium 中的**真实运行截图**（Linux 字体，与 Windows 的 Segoe UI / 微软雅黑观感略有差异），不是效果图。

| 文件 | 内容 |
|---|---|
| 01-sign-in | 应用内登录 |
| 02-files-light / 03-files-dark | 云盘根目录，浅色 / 深色 |
| 04-context-menu | 右键菜单（含禁用项原因） |
| 05-rename-validation | 重命名同名阻止 |
| 06-huge-dir | 12,000 项目录 |
| 07-transfers-active / 08-transfers-failed | 传输进行中 / 失败（含错误码与诊断） |
| 09-cloud-dialog | 新建云下载 |
| 10-sync-delete-confirm | 含删除的同步计划确认 |
| 11-mount | 挂载能力检测 |
| 12-settings-quota | 配额（精确值与“不可获取”并存） |
| 13-quota-exhausted / 14-network-down / 15-auth-expired | 三种错误态 |
| 16-about / 20-settings-network-dark | 关于与致谢 / 高级网络诊断（深色） |
| 21-clip-download | 片段下载面板 |
| 22-quality-dialog | 指定画质下载 |
| 23-batch-rename | 批量重命名预览 |
| 24-duplicates | 查找重复 |
| 25-space | 空间分析 |
| 26-archive | 压缩包浏览（已输入密码） |
| 27-command-palette | 命令面板 |
| 28-share | 创建分享 |
| 29-add-link-preview | 添加链接并挑选文件 |
| 31-grid-large / 32-starred | 大网格 / 星标页 |
| scale-{100,125,150,200}-{light,dark}-files | 四种缩放比例 |

---

## 待处理

### 仍未实现（不伪称已完成）

- **没有在 Windows / WebView2 / Tauri 中实际运行过**。`src-tauri` 归 Astra，本环境也没有 Windows。Windows 缩放是用等效 CSS 视口 + deviceScaleFactor 模拟的（1280×720 物理像素在 125% 下 = 1024×576 CSS 像素，以此类推）。
- 上传（含拖拽上传）。外部播放器和文件夹下载在 v3 中已有界面，但要等后端实现。
- 依赖平台能力的功能（§P9）：打开外链、在资源管理器中显示、选择本地文件夹、安全存储（WebDAV 凭据 / 自定义代理）、清理缓存、连通性检查、导出诊断。
- 同步冲突的逐项处理界面（契约中没有对应命令；目前冲突会阻止执行，需重新扫描）。
- 挂载的启动/停止流程（`mount_start` 永远不会被启用，直到前置检查真实通过）。
- 多语言（仅简体中文；文案集中在组件内，尚未抽成资源文件）。
- 只做了 ARIA 结构与键盘测试，**没有用 NVDA / 讲述人做读屏实测**。

### 风险与许可证

- 前端不含任何第三方源码片段；运行时依赖只有 Preact（MIT）与 @tauri-apps/api（Apache-2.0 OR MIT）。详见 `docs/UI_SOURCES_AND_DEPENDENCIES.md`。
- Mock 块（31.5 KB）目前也会随 `dist/` 一起发布（Tauri 模式下永不加载）。建议 M2 增加构建开关把它从正式包中剔除。
- `localStorage` 只保存界面偏好（主题、行距、排序、声明已读、Mock 场景），读写都有 try/catch，丢失不影响功能。

### 接口变化

**有，均为增量提案**：见 `docs/CONTRACT_PROPOSAL_UI_001.md`（§P0–§P12，v3 增补 §P13–§P23）。没有修改 `03_CONTRACTS.md` 中任何已有字段名或含义；`contract.ts` 与契约文档逐字段一致。

---

## 供 Astra 下一步使用

- **可调用命令（前端已接好）**：§3 全部 20 个命令 + 提案中的 `auth_cancel` `files_search` `files_recent` `files_path` `trash_list` `sync_configs_list` `sync_config_save`，以及 v3 的 21 个命令（§P14–§P23，列表见 `PROPOSED_COMMANDS`）。对应的 PikPak 接口见 `docs/BACKEND_API_ADAPTER.md`。签名集中在 `src/lib/contract-proposals.ts` 的 `CommandMap`。
- **Mock fixtures**：`src/lib/mock/fixtures.ts`（场景、样本树、任务）与 `backend.ts`（行为、错误映射、故障注入）。Rust MockProvider 若沿用相同场景与样本 ID，E2E 可直接复用（§P11）。
- **最先需要的回复**：§P0 根目录值、§P7 IPC 参数风格、§P3 `revision` 字段。
- **建议的下一个 Issue**：`[app] Tauri v2 skeleton + typed IPC commands`（Issue #3）落地后，前端以 `VITE_NEKOPAK_TRANSPORT=tauri` 对 Rust Mock 跑一遍 E2E，修正两边差异，然后冻结契约 v0.2。
