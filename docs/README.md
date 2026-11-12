# 文档索引

[项目首页](../README.md) · [Releases](https://github.com/NekoPak/NekoPak/releases) · [更新记录](../CHANGELOG.md)

## 使用

- [使用指南](USER_GUIDE.md)：安装、登录、文件、云下载、种子、更新和常见问题。
- [界面截图](SCREENSHOTS.md)：桌面与手机截图、来源和复现命令。
- [Android 平台说明](ANDROID.md)：手机范围、存储、后台传输、构建及验收。

## 构建与发布

- [运行、检查与打包](RUNNING.md)。
- [原生桌面客户端](NATIVE_DESKTOP.md)：Slint 界面、渲染器、系统要求、性能与验证范围。
- [发布与签名](RELEASE_SIGNING.md)：产物、密钥类别、更新协议和 Actions 发布流程。
- [Android 平台研究](ANDROID_RESEARCH.md)：SDK、签名与系统约束的依据。

## 设计与契约

- [产品需求](01_PRODUCT_REQUIREMENTS.md)、[架构](02_ARCHITECTURE.md)。
- [Rust ↔ 前端契约](03_CONTRACTS.md)：命令、数据、事件和错误的共同基准。
- [路线图与测试门槛](05_ROADMAP_AND_TESTS.md)、[安全与兼容约束](06_SECURITY_AND_COMPAT.md)、[UI 规范](07_UI_SPEC.md)。
- [研究与决策](08_RESEARCH_AND_DECISIONS.md)。
- [上游功能对齐要求](UPSTREAM_PARITY.md)、[机器可检查清单](parity-manifest.json)、[上游研究](UPSTREAM_PARITY_RESEARCH.md)、[Piko 下载交互参考](PIKO_DOWNLOAD_UX_REFERENCE.md)。

## 实现说明

- [Rust 直接 API 接入](API_ADAPTER.md)、[接口对照](BACKEND_API_ADAPTER.md)。
- [多账号会话](ACCOUNTS_IMPLEMENTATION.md)、[视频片段](MEDIA_SEGMENTS.md)、[挂载](MOUNT_IMPLEMENTATION.md)、[引用归档](REFERENCE_ARCHIVE_IMPLEMENTATION.md)、[云任务／分享／上传](NATIVE_EXTENDED_PROTOCOLS.md)。

## 协议研究

- [原生登录](NATIVE_LOGIN_RESEARCH.md)、[网盘人机验证](DRIVE_PROTOCOL_RESEARCH.md)、[文件与下载](FILE_PROTOCOL_RESEARCH.md)。
- [分享媒体](SHARED_MEDIA_PROTOCOL_RESEARCH.md)、[引用归档](REFERENCE_ARCHIVE_PROTOCOL_RESEARCH.md)、[种子导入](TORRENT_IMPORT_RESEARCH.md)、[网络恢复](NETWORK_RECOVERY_RESEARCH.md)。

## 验收记录

按版本保留，其中的“尚未实现”指当时状态；浏览器演示不等于服务或设备实测。

- 0.1.4：Android 后台传输，见 [ANDROID.md](ANDROID.md#background-transfer-keep-alive-014) 与 [检查记录](evidence/background-transfer-checks.json)。
- 0.2.0：[原生桌面与手机界面改进](../release/notes/0.2.0.md)，见 [原生桌面说明](NATIVE_DESKTOP.md)。
- 0.1.3：[应用更新与实时进度](UPDATES_PROGRESS_REPORT.md)。
- 0.1.2：[速度、种子和软键盘](SPEED_TORRENT_IME_REPORT.md)。
- 0.1.1：[手机界面与连接恢复](MOBILE_RECOVERY_REPORT.md)。
- 0.1.0 及以前：[桌面集成](DESKTOP_INTEGRATION_REPORT.md)、[桌面下载体验](DESKTOP_UX_FIX_REPORT.md)、[网盘与文件](DRIVE_FILES_FIX_REPORT.md)、[登录修复](LOGIN_FIX_REPORT.md)、[CLI 移除与 API 迁移](API_MIGRATION_REPORT.md)。

## 来源与许可

- [MIT 许可证](../LICENSE)、[第三方公示](../THIRD_PARTY_NOTICES.md)。
- [来源与许可证审计](04_SOURCES_LICENSES.md)、[前端依赖](UI_SOURCES_AND_DEPENDENCIES.md)、[UI 体积](ui-size-report.md)。

当前产品直接使用 Rust 客户端协议，不安装、调用或回退到 PikPak 官方 CLI。
