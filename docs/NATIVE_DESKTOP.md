# 原生桌面客户端

桌面版（`crates/nekopak-desktop`，可执行文件 `nekopak`）是原生程序：界面由
[Slint](https://slint.dev) 直接绘制，不内嵌浏览器，也不依赖 Microsoft Edge WebView2。
Android 版不受影响，仍由 `src-tauri` 以系统 WebView 承载手机界面。

## 结构

```text
crates/nekopak-desktop/
  ui/            Slint 界面：tokens、widgets、shell、overlays、pages/*
  ui/generated/  由脚本生成的配色与图标（勿手改）
  locales/       由网页端文案生成的译文 + native.json（原生界面新增文案）
  src/
    main.rs        启动：运行时、宿主、窗口、事件循环
    host.rs        数据目录、后端选择、后台任务（下载/上传/媒体/同步）
    app.rs         应用上下文、能力门控
    nav.rs         路由、标签页与历史、侧栏
    files.rs       文件列表：加载分页、排序筛选、多选、键盘
    actions.rs     文件操作（幂等键、撤销）
    transfers.rs   传输页与侧栏活动摘要
    overlay.rs     菜单、对话框、表单、提示
    panel.rs       通用面板页（设置、同步、挂载、分享等共用）
    …              auth、cloud、media、tools、sync_mount、settings_page、updates、desktop
```

界面线程只做渲染和状态更新；所有网络与磁盘工作在 Tokio 运行时执行，结果回到界面线程。
核心通过 `nekopak-core::AppService` 直接调用，没有 IPC 序列化。

渲染器：默认使用 GPU（OpenGL，`gpu` 特性），创建失败时由 Slint 回退到内置的 CPU
渲染器，因此远程桌面、虚拟机和没有可用显卡驱动的机器也能运行。

数据目录沿用旧版标识 `io.nekopak.desktop`（Windows 为 `%LOCALAPPDATA%\io.nekopak.desktop`），
会话、任务库和设置无需迁移。界面偏好中与设备相关的部分保存在同目录的 `desktop-ui.json`。

## 运行

```powershell
cargo run -p nekopak-desktop --locked
$env:NEKOPAK_BACKEND = 'mock'; cargo run -p nekopak-desktop   # 离线演示数据，不连接 PikPak
```

Linux 构建需要 `libfontconfig1-dev`、`libxkbcommon-dev`；运行需要 `libxkbcommon-x11-0`
和一款中日韩字体。

## 资源生成

配色、图标和译文与网页界面共用同一份来源：

```bash
node scripts/gen-native-assets.mjs          # 重新生成
node scripts/gen-native-assets.mjs --check  # CI：提交的生成物是否过期
```

原生界面新增的文案写在 `locales/native.json`。单元测试
`native_ui_copy_is_translated_in_every_locale` 会扫描 `src/` 与 `ui/` 中的全部文案，
缺少任何一种语言的译文都会失败。

## 测试

```bash
cargo clippy -p nekopak-desktop --all-targets --features automation -- -D warnings
cargo test -p nekopak-desktop
cargo build -p nekopak-desktop --features automation
python3 tests/native/run.py --keep out/      # 无头界面冒烟测试，两种渲染器
```

`automation` 特性提供脚本化驱动（`NEKOPAK_UI_SCRIPT`，见 `src/automation.rs`）：
点击、按键、输入和截图。冒烟测试在 Xvfb 下用离线演示数据走一遍主要页面，
并检查每张截图确实渲染了内容。发布构建不启用该特性。

## 已验证范围

- Linux：clippy（警告即错误）、单元测试、两种渲染器的无头冒烟测试，以及针对命令面板、
  查看器、媒体库、拖放、选段标记的脚本化界面走查（离线演示数据）。
- Windows（GitHub Actions `windows-latest`）：全工作区 clippy 与测试、发行版构建、
  便携包与 NSIS 安装器打包均通过；视频预览解码器有一项端到端测试——本地 H.264 片段经
  回环媒体会话由 Media Foundation 解码、定位和读到结尾。CI 还会启动发行版可执行文件并
  记录出窗时间与内存（仅供参考，不设阈值）。
- **仍未在真机上人工验收**：托盘、关闭到托盘、单实例、开机自启、安装与卸载流程、
  系统文件拖入、鼠标侧键。这些代码在 Windows 上编译并通过静态检查，但没有人操作过。
- 真实账号流程（登录、验证、下载、上传、同步、挂载）未用真实服务验证；它们调用的核心
  接口未改动。

## 离线演示

`NEKOPAK_BACKEND=mock` 使用 `MockProvider::demo`：示例目录可以搜索、重命名、移动、
复制、加星标、移入回收站和还原（都只在内存里），“照片”文件夹里的图片在运行时生成，
因此图片查看器和媒体库无需联网即可体验。侧栏“演示”标签或命令面板可以切换场景、
注入一次性故障。契约夹具（`MockProvider::new`）保持不变。

## 应用内预览

| 内容 | 实现 |
|---|---|
| 图片（PNG / JPEG / GIF / WebP / BMP） | 经核心的受校验媒体源整文件读取（上限 64 MiB），在后台线程解码并按 EXIF 方向校正；缩放、上一张 / 下一张 |
| 媒体库 | 文件夹内图片或视频的缩略图墙；只为可见行加载缩略图，同时 3 个，缓存上限 160 张 |
| 视频 | 容器信息来自内置解析器；Windows 上用 Media Foundation 解码画面，可拖动定位、静音预览、标记片段起止点（I / O）后转到“下载指定段落” |

Media Foundation 的库在运行时按需加载，不做静态导入：在没有该组件的系统上（Windows N
未装媒体功能包、未启用该功能的 Server）程序照常启动，只是没有画面预览。任何解码失败
都会退回“用播放器打开”。预览不播放声音；完整播放仍交给外部播放器。HEIC、AVIF、SVG
不在应用内解码。

## 系统要求（Windows）

x64，Windows 10 1709 或更新版本。可执行文件静态导入 `cldapi.dll`（云文件占位符，
1709 起提供）；更早的 Windows 10 无法启动。不需要 WebView2、.NET 或 VC++ 运行库安装包。

## 待决定

应用更新源和 README 中的下载链接仍指向 `NekoPak/NikoPak` 的发行页，因为已发布的
0.1.3 安装包和更新清单在那里。迁移到 `NekoPak/NekoPak` 需要先在新仓库发布带签名
清单的版本，否则已安装的客户端会检查不到更新。
