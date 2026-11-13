<p align="center">
  <img src="public/brand/app-icon.svg" width="80" height="80" alt="NekoPak 猫箱图标">
</p>

# NekoPak

轻量的 PikPak 桌面与 Android 客户端。Rust 负责账号、文件、传输和任务恢复；**Windows 使用 Slint 原生界面**，支持 GPU 渲染与 CPU 回退，无需 WebView2。Android 使用系统 WebView 和独立的手机界面。

*A lightweight, Rust-powered PikPak client for Windows and Android.*

[下载 0.2.0 预览版](https://github.com/NekoPak/NekoPak/releases/tag/v0.2.0) · [使用指南](docs/USER_GUIDE.md) · [截图](docs/SCREENSHOTS.md) · [更新记录](CHANGELOG.md) · [问题反馈](https://github.com/NekoPak/NekoPak/issues)

> **Pre-alpha 预览。** Windows x64 与 Android ARM64 提供可安装包；真实账号、实体手机和完整上游功能仍在逐项验收。NekoPak 是独立社区项目，与 PikPak 官方无隶属关系。

## 下载

在 [0.2.0 Release](https://github.com/NekoPak/NekoPak/releases/tag/v0.2.0) 下载：

| 平台 | 文件 | 说明 |
|---|---|---|
| Windows 安装版 | [Setup.exe](https://github.com/NekoPak/NekoPak/releases/download/v0.2.0/NekoPak-0.2.0-windows-x64-setup.exe) | Windows 10 1709+，x64；当前用户安装，带快捷方式和卸载入口，无需 WebView2 |
| Windows 便携版 | [Portable.zip](https://github.com/NekoPak/NekoPak/releases/download/v0.2.0/NekoPak-0.2.0-windows-x64-portable.zip) | 完整解压后运行 `NekoPak/nekopak.exe`，保留同目录许可文件 |
| Android | [ARM64 APK](https://github.com/NekoPak/NekoPak/releases/download/v0.2.0/NekoPak-0.2.0-android-arm64-v8a.apk) | Android 7.0（API 24）起，ARM64 设备，使用系统 WebView |

每个 Release 还附带签名清单、许可证包和构建报告，核验方法见[发布与签名](docs/RELEASE_SIGNING.md)。Linux / macOS 目前只有核心 CI，没有经过桌面验收的发行包。

**升级：** 0.1.4 可在应用内更新到 0.2.0。0.1.3 及更早版本读取旧仓库，需要从这里手动安装一次。Windows 沿用原数据目录，保留会话、任务和设置；Android 沿用 APK 签名，可直接覆盖安装，无需卸载。之后在 Windows **设置 → 关于**、Android **我的 → 关于 → 应用更新**检查新版本。卸载 Android 应用会删除应用内下载和登录数据。

## 0.2.0 更新

- **原生桌面**：虚拟化文件列表与网格、传输、登录、多账号、设置、同步与挂载直接连接 Rust 核心；列表和任务状态按变化更新，Windows 文件名排序使用预计算的系统排序键。
- **浏览与预览**：图片查看器、媒体库、Windows 视频画面预览与片段标记、Ctrl+K 命令面板、F1 快捷键和文件拖放；窄窗口自动收起次要列。
- **手机体验**：更紧凑的传输卡片、行内操作按钮、照片墙和触控反馈；语言包按需加载。保留 0.1.4 的 Android 后台下载和通知暂停。

[完整发布说明](release/notes/0.2.0.md) · [原生桌面说明与验证范围](docs/NATIVE_DESKTOP.md)

## 界面

桌面支持路径导航、排序、搜索、多标签、右键操作及明暗主题。下图由 Slint 原生客户端渲染，使用应用内置的离线演示数据。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/native/files-dark.png">
  <img src="docs/screenshots/native/files-light.png" alt="NekoPak 原生桌面：侧栏、路径导航和文件列表；离线演示数据" width="1280">
</picture>

手机使用 **文件／传输／我的** 底部导航，单击打开文件夹、长按多选，下载卡片显示字节、百分比和速度。

<p>
  <img src="docs/screenshots/android/phone-files-fixture-390.png" alt="手机文件页：触屏文件列表；浏览器测试样例" width="30%">
  <img src="docs/screenshots/android/progress-native-0.1.3.png" alt="Android 原生传输页：实际接收进度与速度；本地测试服务" width="30%">
  <img src="docs/screenshots/android/release-public-update-0.1.3.png" alt="Android 发行版：关于页读取更新源" width="30%">
</p>

上方手机截图来自浏览器样例和早期版本的隔离模拟器，不含真实账号，也不代表真实网速；0.2.0 的传输卡片已改为紧凑布局。[更多截图 →](docs/SCREENSHOTS.md)

## 功能

已接入代码并完成离线或原生测试；服务端行为、账号权限与格式兼容性以实际使用为准。

- **账号与文件**：应用内邮箱密码登录、记住账号、会话恢复、多账号切换；浏览、分页搜索、最近、星标、回收站和基础文件操作。
- **本地下载**：原文件下载、暂停／继续、重启后校验续传、限速、同名保护；进度按实际接收字节刷新。
- **云下载**：链接、磁力和本地 `.torrent`（v1 与含 v1 的混合格式，≤10 MiB），提交前可预览。
- **媒体与片段**：视频预览、起止时间选择、关键帧对齐、画质选择；支持范围内的 MP4、MKV、TS，限制见[媒体说明](crates/nekopak-media/README.md)。
- **桌面整理**：目录上传、批量重命名、重复文件检查、分享、引用归档、本地只读映射和同步；Cloud Files 与可选 WinFsp 按能力检测启用，验收范围见[集成记录](docs/DESKTOP_INTEGRATION_REPORT.md)。
- **Android 后台传输**（0.1.4）：有本地任务时显示进度通知，切到后台或锁屏后继续下载，可在通知中全部暂停；可在 **我的 → 下载与传输** 关闭。
- **体验与更新**：简繁中文、英语、日语、韩语；明暗主题；Windows 托盘与自启动；签名更新通道，下载和安装都需用户确认。

### 当前限制

- Android 后台传输尚未在实体手机上验收。系统收回后台时间、划掉应用或关闭开关时任务会暂停，返回后需手动继续；Doze 和厂商省电策略可能提前结束后台运行。
- Android 下载保存在应用私有目录，完成后用 **导出文件** 保存到共享位置。普通上传、共享目录同步、本地映射和外部播放器尚未支持。
- 原生桌面的真实账号流程、托盘、自启动、系统拖入和视频预览观感尚未在 Windows 实体机人工验收。视频预览无声音，完整播放交给外部播放器；图片预览暂不支持 HEIC、AVIF、SVG。
- 上游 Piko 的 24 组必需功能尚未全部验收，预览版不等于功能对齐，进度见[功能清单](docs/UPSTREAM_PARITY.md)。

## 快速上手

1. 安装对应平台的包，在 NekoPak 中登录自己的账号；需要人工验证时按提示完成后返回。
2. Windows 在 **设置 → 下载与传输** 选择下载目录；Android 下载完成后从任务卡片导出文件。
3. 点击 **添加链接**，粘贴链接或切到 **种子文件**，确认目标目录和预览内容后提交云下载。
4. 在传输页查看任务。云下载保存到云盘，本地下载保存到设备。

详细操作和常见问题见[使用指南](docs/USER_GUIDE.md)。

## 数据与安全

- 密码只用于当次请求。Windows 会话保存在系统凭据管理器，Android 用 Keystore 密钥加密；不写入普通配置或日志。
- 任务库损坏时报错并保留原文件，不静默重置。
- Rust 直接访问社区客户端协议，不安装或调用 PikPak CLI；网络增强默认关闭，不修改系统 Hosts。
- 更新使用 NekoPak 自己的 Ed25519 签名清单，Android 另有 APK 签名。**发布清单签名不等于 Windows Authenticode 证书。**

## 从源码构建

需要 Rust 1.98+；Windows 另需 MSVC 工具链和 Windows SDK。Node.js 22+ 用于手机界面与资源生成，Python 3.12+ 用于打包。Linux 依赖见[原生桌面说明](docs/NATIVE_DESKTOP.md)。

```powershell
cargo run -p nekopak-desktop --locked
# 离线演示：
$env:NEKOPAK_BACKEND='mock'; cargo run -p nekopak-desktop --locked
```

`npm ci` 后可用 `npm run dev` 启动离线浏览器演示，不能登录真实账号。原生运行与打包见 [RUNNING.md](docs/RUNNING.md)，Android 构建见 [ANDROID.md](docs/ANDROID.md)。

提交前的检查：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm ci
npm run build
npm test
python scripts/check_parity.py
```

### 发布

版本号写在 `package.json`、`Cargo.toml`、`src-tauri/tauri.conf.json`，同步更新两份 lockfile 与接口样例中的应用版本；Android `versionCode` 写在 `src-tauri/tauri.android.conf.json`。发布说明放在 `release/notes/<版本>.md`。将本仓库的 `codex/release-<版本>` PR 合并到 `main` 后，[`release.yml`](.github/workflows/release.yml) 会构建并签名 Windows 与 Android 包，在干净 runner 上验证安装器，然后创建 `v<版本>` tag、预览 Release 和更新清单。也保留已有 tag 和手动触发方式；版本标签必须与源码一致。需要的 Secrets 见[发布与签名](docs/RELEASE_SIGNING.md)。

## 文档

- [使用指南](docs/USER_GUIDE.md)：安装、登录、下载、种子、更新与常见问题。
- [原生桌面说明](docs/NATIVE_DESKTOP.md)：结构、渲染器、系统要求、性能与验证范围。
- [文档索引](docs/README.md)：架构、接口契约、平台说明、协议研究与各版本验收记录。
- [提交问题](https://github.com/NekoPak/NekoPak/issues)：附版本、平台和复现步骤；不要提交密码、令牌或带授权参数的链接。

## 许可与致谢

主仓采用 [MIT License](LICENSE)，未复制 AGPL Piko 的实现代码。

感谢 [Piko](https://github.com/NihilDigit/piko)、[pikpaktui](https://github.com/Bengerthelorf/pikpaktui)、[pikpakcli](https://github.com/52funny/pikpakcli)、[pikpak-kotlin](https://github.com/NihilDigit/pikpak-kotlin)、[PikPak Enhancement Master](https://github.com/digbug82/PikPak_Enhancement_Master)、[MediaMP](https://github.com/open-ani/mediamp) 和 [Rclone](https://github.com/rclone/rclone) 提供的功能、协议或设计参考，以及 Slint、Tauri、Preact 和 Rust 生态。依赖与参考来源的版本和许可记录在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 与[来源审计](docs/04_SOURCES_LICENSES.md)。
