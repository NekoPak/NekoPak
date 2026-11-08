# 研究记录、事实边界与决策清单

核查时间：2026-11-07。此文件用于防止其他模型把网上的讨论、推断与已证实事实混淆。

## 已确认的公开信息

1. **PikPak 官方提供 CLI**：官方介绍了命令行下载、上传、流媒体、分享、AI 工具使用，并通过浏览器授权。来源：<https://mypikpak.com/en-US/cli> 和 <https://mypikpak.com/zh-CN/faq>。
2. **Connected Apps 限制**：CLI、WebDAV、Rclone 等属于已连接应用，云下载/下行/上传各共享该维度总月配额的 25%；免费账号 Connected Apps 每日下行 5 GB。来源：<https://mypikpak.com/en-US/connect-apps-faq>。
3. **WebDAV 是 Premium 功能**：可执行浏览、下载、上传、重命名、移动、回收站操作；不是普通免费用户默认可用的挂载入口。来源：<https://mypikpak.com/en-US/help-center/connected_apps/webdav/how_to>。
4. **Piko GitHub 项目**：Kotlin Multiplatform、Android/Windows/macOS/Linux；README 明确列出 `52funny/pikpakcli`、`PikPak_Enhancement_Master`、`MediaMP` 的借鉴关系；AGPL-3.0-or-later。来源：<https://github.com/NihilDigit/piko>。
5. **pikpaktui GitHub 项目**：Rust、Apache-2.0，文件浏览、云下载、上传下载、分享、播放/直链、JSON 输出和部分断点下载。注意其使用独立社区兼容接口，不等于官方 API。来源：<https://github.com/Bengerthelorf/pikpaktui>。
6. **Rclone PikPak 后端**：提供同步、挂载及特殊命令；来源：<https://rclone.org/pikpak/>。不能因为 Rclone 能做就假设 NekoPak 的直接 API 实现已验证随机读取。
7. **Windows Cloud Files API**：系统平台 API `CfRegisterSyncRoot` 支持同步根注册，Windows 10 1709+；它不是虚拟盘符生成 API。来源：<https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot>。
8. **WinFsp**：GPLv3 带 FLOSS 特别许可；来源：<https://github.com/winfsp/winfsp/blob/master/License.txt>。
9. **Tauri Windows WebView2**：系统运行时按平台现有/引导安装处理；若内置 WebView2 Offline Runtime 可能大幅增大包体，不能宣称每种分发方式都 5MiB。来源：<https://tauri.app/zh-cn/distribute/windows-installer/>。

## 需要工程实测，而非推断

- 2026-11-08 主路线改为 rust_compat：需核验已授权客户端身份、邮箱登录/CAPTCHA、会话刷新、实际分页、配额与 Range。旧 CLI 调研已停止，历史观察归档，不再做 CLI 适配或分发研究。
- PikPak 是否提供公开且稳定的通用 REST API 给普通第三方：**未发现能作为既定事实引用的完整公开文档**。
- 社区 Rust 适配器使用非公开协议的 ToS 风险、验证码/风控与地区可用性：需要审阅，不能写成“官方支持”。
- `Piko` → `NekoPak` 的 Fork 派生关系与许可证实际边界：由代码复用、分发方式决定。
- Windows 压缩包 5MiB 是否能在保留 Tauri/Rust、SQLite/TLS/文件管理后达成：**待构建实测**。
- `nekopak` 作为软件名/商标/域名的全局可用性：**尚未做正式商标检索**，GitHub 初步搜索不等同清查。

## 已选/待选决策

| ID | 事项 | 当前建议 | 状态 |
|---|---|---|---|
| D-01 | 产品名 | NekoPak / `nekopak` | 候选确认，待仓库命名 |
| D-02 | 主技术栈 | Rust + Tauri v2 + 最小 TypeScript/CSS | 建议执行 |
| D-03 | 源码路径 | 独立 Rust 仓优先，Piko Fork 只读研究；如真正从 Piko Fork 派生则遵守 AGPL | Gate 0 必须确认 |
| D-04 | 默认主连接 | rust_compat 直接客户端协议 + 原生登录；官方 CLI 完全移除 | 2026-11-08 用户确认；真实账号待验收 |
| D-05 | 轻量化 | Windows Lite ≤5MiB 压缩包为挑战目标，不破坏可靠性 | 待基准 |
| D-06 | 同步 | 单向优先，双向删除默认关 | 已确定 |
| D-07 | 挂载 | Windows Cloud Files / WinFsp 分开实现 | 已确定 |
| D-08 | 实验网络选项 | 默认关，安全透明，不自动改 Hosts 或绕过风控 | 已确定 |

## 工程上的重要注意事项

`pikpaktui` 当前某些文件包含与 Android 客户端形态相关的 `client_id`、用户代理和 captcha 参数，这些**不等于可自由复用的官方开发者凭据**。复制代码前要审计鉴权、协议条款及可能的机密/许可问题，并明确是否保留该实现。若接入条件尚未核验，保持能力禁用并记录缺口；不回退 CLI，不为“兼容更多功能”规避服务端检测。

## 每次研究更新请记录

```text
Date:
Source URL:
Commit or published date:
Observed behavior:
Verified by (mock / live Free / live Premium / docs only):
What remains unknown:
Impact on contract / feature / license:
```
