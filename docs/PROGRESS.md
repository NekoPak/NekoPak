# 工作进度

仓库：`NekoPak/NekoPak`。当前收尾分支：`codex/release-0.2.0`。
原生桌面的模块与测试见 [NATIVE_DESKTOP.md](NATIVE_DESKTOP.md)，历史交接见 [HANDOFF_NATIVE_DESKTOP.md](HANDOFF_NATIVE_DESKTOP.md)。

## 用户要求

- Windows 必须使用 Slint 原生界面；没有把握不要大改已成型的 Windows UI。
- 持续完善 Android，保留已实现的后台传输和任务恢复。
- **禁止创建定时任务或自动唤醒。** 用户会自行开新会话，靠本文续做。
- 不要停下来反复询问；按合理默认推进，及时记录实际验证范围，不把未验证写成通过。
- 2026-11-12：用户要求合并 Claude 的工作，随后明确授权完成 Release、tag 与 README，离线休息。

## 0.2.0 收尾（2026-11-12）

- 仓库没有现成 PR，`main` 仍停在 0.1.3。新建整合分支，保留所有原始提交作者。
- 已整合 Claude 原生桌面分支 `d3bfe4f`：Slint、媒体预览、命令面板、拖放、紧凑手机传输页、按需语言包与窄窗口列。
- 已整合桌面性能分支 `ba9f0a1`：列表 / 网格 / 传输增量更新、Windows 系统排序键及对应测试、Windows 启动与内存测量记录。
- 已整合已发布的 `v0.1.4`（`00fe237`）：Android 后台传输、原签名身份、迁至本仓库的发布与更新源，以及文档整理；避免新版本回退已发布功能。
- 两处文档冲突已解决；根据合并后的网页文案重新生成原生语言资源。
- 版本准备为 0.2.0，Android versionCode 200；README 使用原生桌面截图、本仓库下载链接和准确的升级说明。
- 本地验证已通过：生产前端构建，Vitest 118 + API 6，Python 24（另 4 跳过），桌面浏览器 57（0 外部请求），手机浏览器 12；原生资源生成检查 202 文件；版本、原分支历史及功能保留、文档相对链接检查通过。
- Rust 1.99.1：`cargo fmt --all --check`、核心测试 117（另 15 显式忽略）及 `cargo check -p nekopak-provider-native --locked` 均通过。详见 [预检证据](evidence/release-0.2.0-preflight.json)。
- **远程阻塞：GitHub 写入均返回 403**。Git push 被拒绝；GitHub connector 的创建 Git blob、创建 PR 也返回 `Resource not accessible by integration`。当前连接需要恢复对 `NekoPak/NekoPak` 的写权限；本会话没有可接管的登录浏览器。
- **尚未推送整合分支，尚未创建 PR、合并、tag 或 Release**。远端 `main` 仍为 `a95c4a7`；最近公开版本仍为 `v0.1.4`。禁止把本地准备写成已发布。
- 完整收尾包保存到 `/workspace/shared/downloads/NekoPak-0.2.0-handoff.zip`，包含自足 Git bundle、相对 main 的二进制补丁、发布说明、拟用 PR 描述和本地验证日志。
- 恢复 GitHub 写权限后的顺序：推送 `codex/release-0.2.0` → 创建 PR → 等待整合后的 Windows / Linux / macOS / Android CI 全绿 → 按检查过的 head SHA 合并 → 从合并后的 main 创建并推送 `v0.2.0` → 跟进签名发布工作流 → 重新下载两端产物及清单，验签并检查更新源。

## 已有验证证据

- Claude 最后代码提交 `540295d` 的 Windows CI `43127986864` 和 Portable Rust core `43127986839` 全绿（包括 macOS、Linux、原生 UI 冒烟）。顶部 `d3bfe4f` 只改进度文档。
- 桌面性能提交 `87ba601` 的 Windows CI `43130743570` 与 Portable Rust core `43130743556` 全绿。顶部 `ba9f0a1` 只补测量文档。
- 0.1.4 签名发布 `43181049417` 成功，Windows 与 Android 包已在本仓库发布。
- 上述属于各源分支的 CI 证据；不能替代尚未运行的整合后跨平台 CI。本轮本地结果单独记在上面的预检证据中。

## 保留的边界

- 本地未编译或运行 Slint 桌面 GUI 冒烟；执行器缺 fontconfig / xkbcommon 开发库及 Xvfb。Windows / Android 整合 CI 因无法推送而未运行。
- Windows 真机人工验收仍待做：托盘、关闭到托盘、单实例、自启动、系统拖入、视频预览观感。
- 真实账号流程与 Android 实体机后台运行未在本轮验证；Doze / 厂商省电可能提前结束后台任务。
- 保持 Windows 10 1709+；不做无法在旧系统验证的 `cldapi.dll` 加载改造。
- Android 普通上传与共享目录同步尚未支持。Linux / macOS 只有核心检查与 Linux 原生 UI 测试，暂不发布桌面包。
