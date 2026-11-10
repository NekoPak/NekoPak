# 文档索引

[项目首页](../README.md) · 当前公开版本：[0.1.3 预览](https://github.com/NekoPak/NikoPak/releases/tag/v0.1.3)

## 使用应用

- [使用指南](USER_GUIDE.md)：安装、登录、文件、云下载、种子、更新和常见问题。
- [界面截图](SCREENSHOTS.md)：桌面与手机截图、来源和复现命令。
- [版本更新记录](../CHANGELOG.md)：每个预览版的变化。
- [Android 平台说明](ANDROID.md)：手机范围、存储、生命周期、构建及验收。
- [发布与签名](RELEASE_SIGNING.md)：产物、密钥类别、更新协议和 CI。

## 最新实现和验收

历史报告按时间保留，其中的“尚未实现”指当时状态。新版本结论优先参照对应报告，不能把浏览器演示当作服务或设备实测。

- [0.1.3：应用更新与实时进度](UPDATES_PROGRESS_REPORT.md)、[结构化检查汇总](evidence/updates-checks.json)。
- [0.1.2：速度、种子和软键盘](SPEED_TORRENT_IME_REPORT.md)。
- [0.1.1：手机界面与连接恢复](MOBILE_RECOVERY_REPORT.md)。
- [桌面整体集成基线](DESKTOP_INTEGRATION_REPORT.md)、[原生媒体支持范围](../crates/nekopak-media/README.md)。
- [API 接入当前状态](API_ADAPTER.md)、[后端接口说明](BACKEND_API_ADAPTER.md)、[实现说明](IMPLEMENTATION_NOTES.md)。

## 构建与贡献

- [运行、检查与打包](RUNNING.md)。
- [Android 构建](ANDROID.md)、[平台研究依据](ANDROID_RESEARCH.md)。
- [Rust 工作指引](../prompts/ASTRA_BACKEND.md)、[前端工作指引](../prompts/OPUS_FRONTEND.md)。

## 设计与契约

- [产品需求](01_PRODUCT_REQUIREMENTS.md)、[架构](02_ARCHITECTURE.md)。
- [Rust／前端契约](03_CONTRACTS.md)：命令、数据、事件和错误的共同基准。
- [路线图与测试门槛](05_ROADMAP_AND_TESTS.md)。
- [安全与兼容约束](06_SECURITY_AND_COMPAT.md)、[UI 规范](07_UI_SPEC.md)。
- [研究与决策](08_RESEARCH_AND_DECISIONS.md)、[历史交接](09_AGENT_HANDOFF.md)。
- [上游功能对齐要求](UPSTREAM_PARITY.md)、[机器可检查清单](parity-manifest.json)、[上游研究](UPSTREAM_PARITY_RESEARCH.md)。

## 来源与许可

- [MIT 主仓许可证](../LICENSE)。
- [第三方公示与实际使用关系](../THIRD_PARTY_NOTICES.md)。
- [来源与许可证审计](04_SOURCES_LICENSES.md)、[前端依赖](UI_SOURCES_AND_DEPENDENCIES.md)。
- [登录协议研究](NATIVE_LOGIN_RESEARCH.md)、[文件协议](FILE_PROTOCOL_RESEARCH.md)、[种子导入](TORRENT_IMPORT_RESEARCH.md)。

`history/` 内的 CLI 文档仅为历史记录。当前产品直接使用 Rust 客户端协议，不安装、调用或回退到 PikPak 官方 CLI。
