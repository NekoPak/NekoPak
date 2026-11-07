# NekoPak 安全、账号、配额与实验性兼容功能

状态：安全基线草案；任何声称绕过官方限制的功能必须单独风险评审，不能暗中启用。

## 1. 凭据与会话

- 官方 CLI 优先采用其浏览器授权，不要求用户把 PikPak 密码粘贴进 NekoPak。
- CLI 子进程只在可信的已安装路径调用；避免通过 shell 字符串拼接参数，防止命令注入；若二进制有更新，先检查版本/数字签名（如可用）。
- 如果兼容层确实需要 token，仅使用操作系统安全存储：Windows DPAPI/Credential Manager、macOS Keychain、Linux Secret Service（不可用则明确告知用户，不回退明文）。
- 不在日志/崩溃报告/前端事件、错误文本中记录 refresh token、cookies、真实下载直链、PAT、密码；支持脱敏日志导出。
- 授权刷新串行化且带超时；失败显示 `AUTH_EXPIRED`，不无限重试；退出登录清除本地凭据及缓存中的敏感 URL。
- 不提供明文自动提取官方客户端 cookie 的功能。

## 2. 外部服务与权限

- 独立社区项目，不声称被 PikPak 许可或推荐；实际分发前需复核 ToS 与第三方接口合法接入条件。
- 官方 CLI、MCP、WebDAV 不等于有通用公开 REST API；不要将社区反向工程的通信称作“官方 API”。
- WebDAV 已公开面向 Premium，且属于 Connected Apps。CLI/其他已连接应用的配额规则以官方帮助中心为准。
- 执行删除、分享和更改权限需要明确的 UI 确认，并区分“移至回收站”和“永久删除”。
- 不在 NekoPak 内置自动多账号轮转以规避配额，也不对同一失败请求无限切后端。

## 3. 配额状态机

| 配额维度 | 官方 Connected Apps 规则摘要 | NekoPak 行为 |
|---|---|---|
| 月度下载下行 | 所有已连接应用共享该维度有效月配额的 25% | 显示剩余额度与节流预期；达到上限不通过切后端来重置 |
| 月度上传 | 共享对应月度额度的 25% | 达上限暂停新上传，允许用户查看任务历史 |
| 月度云下载 | 共享对应月度额度的 25% | 达上限禁止创建新任务，允许查看已运行任务 |
| 免费账号每日下行 | 20GB 总日限额中 Connected Apps 最多 5GB，按新加坡时间 00:00 重置 | UI 标记每日与每月双重限制，较早触发的限制先生效 |

官方解释：<https://mypikpak.com/en-US/connect-apps-faq>；<https://pikpakdrive.com/en-US/help-center/transfer_quota/connected_apps_quota/difference_from_monthly_quota>

配额数据**不能凭本地计算宣称精确**，若后端缺少可靠 `remaining` 字段，显示“不可获取/估计值”，不伪造余额。

## 4. 文件系统和同步安全

- 规范化远端 ID 和本地 path，阻止 `../`、绝对路径、保留设备名/符号链接穿越，拒绝写出配置根目录。
- Windows 大小写不敏感、保留名称、末尾空格/句点、超长路径；Linux Unicode 正规化差异必须测试。
- 下载用 `.part` 和元数据标识，完成校验后原子提交；失败时保留可追溯状态。
- 自动同步默认无删除，冲突文件保留副本；连接失败不得导致删除计划。
- 挂载必须保证系统回调不死锁，不持有全局锁执行网络请求；缓存设配额、支持退出清理。

## 5. “实验性网络兼容”开关

用户曾提出某些地区的连接兼容问题，可保留**安全、透明、默认关闭**的高级实验入口，但不应以规避官方地区检查、验证码、设备风险控制作为产品特性。

建议设置：

```toml
[experimental_network]
enabled = false
proxy_mode = "system"       # system | none | user_provided
# user_proxy_url should live in protected settings, not in public config
custom_dns_mode = false     # disabled until audited; don't silently change system DNS
connection_diagnostics = true
```

严格要求：

- 可选系统/用户提供的代理、DNS/连通性诊断、重试/超时与错误报告；不自动修改系统 Hosts、注册表、防火墙或证书仓库。
- 不附带默认“屏蔽检测服务器”的域名列表，不伪造设备身份、不绕过验证码或风险验证。
- 若未来仅为特定合法网络故障提供目标域名过滤，必须有明确技术依据、用户确认、日志/撤销机制，并经服务条款评估；不可悄悄施加到其他应用。
- Tauri/Rust 内可控制自己发出的网络请求；不能声称这些设置能影响**独立运行的官方 CLI**。网络策略作用范围要精确展示。
- README 可简要描述“高级网络诊断与兼容性设置”，无需作为产品卖点；但项目内规范、UI 和行为不得对用户隐瞒重大影响。

## 6. 威胁模型 & 复核

- 恶意下载 URL / 文件名；日志泄漏下载令牌；不可信后端输出；被篡改的 `pikpak` 可执行文件；共享路径导致意外覆写；竞态执行双向同步；第三方组件更新供应链风险。
- 若用户安装外部 WinFsp/Rclone，明确列出版本和权限要求；软件首次不应自行提权或安装驱动。
- 默认禁用遥测，任何异常报告需要用户主动确认并脱敏。
- 高风险事项（令牌、删除、远程代码执行/子进程、提权）需要独立的 PR review。
