# 工作进度

仓库：`NekoPak/NekoPak`。0.2.0 已合并并公开发布（2026-11-13）。

## 用户要求

- Windows 使用 Slint 原生界面；没有把握不要大改已成型的 Windows UI。
- 持续完善 Android，保留后台传输和任务恢复。
- **禁止创建定时任务或自动唤醒。** 用户自行开新会话，靠本文续做。
- 按合理默认推进，及时记录实际验证范围，不把未验证写成通过。
- 用户已明确授权本轮 PR 合并、Release、tag 和 README 收尾；NekoPak 组织的 GitHub App 授权已于 11-13 完成。

## 已完成

- [PR #1](https://github.com/NekoPak/NekoPak/pull/1) 已合并到 `main`，合并提交 `c153f382f2adf5e52f6157e02a365cea23d72f55`。
- [`v0.2.0` 预览 Release](https://github.com/NekoPak/NekoPak/releases/tag/v0.2.0) 已公开，tag 指向同一合并提交。
- 保留原始提交作者和历史，整合 Claude 原生桌面分支 `d3bfe4f`、桌面性能分支 `ba9f0a1`、已发布的 Android 后台传输版本 `v0.1.4`（`00fe237`）。
- README 已改用原生桌面截图、本仓库 0.2.0 下载链接、Windows 10 1709+ 要求及准确的升级说明。Android versionCode 200，沿用 0.1.4 的 APK 证书。
- 10 份 Windows 接口样例中的应用版本已同步为 0.2.0，Windows 导出一致性检查通过。
- 签名发布支持合并同仓库的 `codex/release-<版本>` PR 到 `main`：两端构建、签名及安装器验收成功后才创建 tag 和公开 Release；也保留 tag / 手动触发。
- 发布上下文校验覆盖未合并 PR、外部 fork、其他分支、版本不匹配和不安全输入。命令行令牌过期后，本轮通过有效的 GitHub 连接完成提交和合并。

## 最终验证

- 检查过的 PR head：`52ed417174c3b967acb8b83728394a3c7e486b28`；Windows、Linux、macOS 和 Android 整合 CI 均通过。
- [Portable Rust core 43395169216](https://github.com/NekoPak/NekoPak/actions/runs/43395169216)：Linux / macOS 核心、Linux 原生桌面单元测试及两种渲染器的界面冒烟通过。
- [Windows desktop checks 43395169144](https://github.com/NekoPak/NekoPak/actions/runs/43395169144)：全工作区检查和测试、接口样例、浏览器与手机布局、原生发行版启动、便携包及 NSIS 安装器打包通过。
- [Android preview checks 43395169111](https://github.com/NekoPak/NekoPak/actions/runs/43395169111)：ARM64 APK 构建与包内验证通过。
- [签名发布 43399829886](https://github.com/NekoPak/NekoPak/actions/runs/43399829886)：Windows / Android 签名、两端清单验证、Windows 实际安装和卸载、公开发布均通过。
- 已从公开下载链接重新下载 10 项产物；两端平台清单与更新清单的 Ed25519 签名、所有文件长度和 SHA256、ZIP 中可执行文件、APK 中原生库及两端源码/lockfile 报告均通过。
- Android 证书指纹与已验签的 0.1.4 报告一致，versionCode 从 104 增至 200。报告和包内容均绑定干净的合并提交。
- 证据：[0.2.0 发布验收](evidence/release-0.2.0-publication.json)。[11-12 本地预检](evidence/release-0.2.0-preflight.json) 是当时的历史快照，其中的权限阻塞已解决。

## 保留的边界

- Windows 实体机人工验收仍待做：托盘、关闭到托盘、单实例、自启动、系统拖入、视频预览观感。
- 真实账号流程与 Android 实体机后台运行未在本轮验证；Doze / 厂商省电可能提前结束后台任务。
- 补充的 Linux 生产更新客户端测试受本云环境的 HTTPS 检查证书阻断（Rustls `UnknownIssuer`，对端颁发者为 OpenAI）；已单独验证公开更新清单和安装包签名，不能把此补充测试写成通过。
- 保持 Windows 10 1709+；不做未经旧系统验证的 `cldapi.dll` 加载改造。
- Android 普通上传与共享目录同步尚未支持。Linux / macOS 暂无桌面发行包。

原生桌面结构与测试方法见 [NATIVE_DESKTOP.md](NATIVE_DESKTOP.md)；早期交接见 [HANDOFF_NATIVE_DESKTOP.md](HANDOFF_NATIVE_DESKTOP.md)。11-12 保存的本地收尾 ZIP 是历史归档，最终源码与产物以本次 tag 和 Release 为准。
