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

- Linux：clippy（警告即错误）、单元测试、两种渲染器的无头冒烟测试。
- Windows：仅交叉编译类型检查（`x86_64-pc-windows-gnu`）。**尚未在真实 Windows 上
  运行、安装或打包。** 托盘、单实例、开机自启、关闭到托盘、NSIS 安装器
  （`mainBinaryName` 指向原生可执行文件）都需要在 Windows CI 和真机上验收。
- 真实账号流程（登录、验证、下载、上传、同步、挂载）未在此次改动中用真实服务验证；
  这些流程调用的核心接口未改动。
- 离线演示后端不提供重命名、分享、搜索等能力，因此这些对话框只经过编译与逻辑测试，
  没有界面截图级验证。

## 尚未移植

| 功能 | 现状 |
|---|---|
| 应用内视频 / 图片预览 | 改为外部播放器；未配置时用系统默认应用打开本机回环会话地址 |
| 选段时的画面预览 | 以时间输入 + “检查对齐”显示实际关键帧起点和估算大小 |
| 媒体库视图 | 以“只看视频”筛选代替 |
| 拖放移动、拖入上传 | 未实现；可用剪切/粘贴、“移动到…”和上传对话框 |
| 命令面板、剪贴板链接提醒 | 未实现 |
| 演示场景切换面板 | 未实现；可用 `NEKOPAK_MOCK_SCENARIO` 环境变量 |

## 待决定

应用更新源和 README 中的下载链接仍指向 `NekoPak/NikoPak` 的发行页，因为已发布的
0.1.3 安装包和更新清单在那里。迁移到 `NekoPak/NekoPak` 需要先在新仓库发布带签名
清单的版本，否则已安装的客户端会检查不到更新。
