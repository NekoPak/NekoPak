# 登录恢复、账号记忆与域名测速

核查与实现日期：2026-11-10。本文记录独立实现依据和真实执行的离线验证；不代表真实账号、
地区连通性或四个域名的服务端功能均已通过用户验收。

## 上游来源和许可证

已通过 GitHub Contents API 实际读取 MIT 项目 `NihilDigit/pikpak-kotlin` 的固定提交
`8eaf5587a883ed4ea9bcca7b43a9194118719e63`：

- [`PikPakDomain.kt`](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/PikPakDomain.kt)，
  blob `fc8e3fab333493d010decff0da9e96d0d16a7c0d`。
- [`PikPakDomainMockTest.kt`](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonTest/kotlin/io/github/nihildigit/pikpak/PikPakDomainMockTest.kt)，
  blob `50401d8f79a8ece435509c740b5368bfed6d20b4`。
- [`LICENSE`](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/LICENSE)，
  blob `c8a78c915234ea3cac680f1c3d1af159c62779f4`，MIT，Copyright (c) 2026 nihildigit。

来源中的四个根域名为 `mypikpak.com`、`mypikpak.net`、`pikpak.me`、`pikpakdrive.com`。
其记录说明 API `user`、`api-drive`（以及其 WebDAV 设置 API `api-dav`）可以随根域名选择，
服务返回的 CDN 下载链接、上传地址和分享地址应按原值使用。NekoPak 本轮只处理自己已实现的
`user` 与 `api-drive`，不添加 WebDAV 后端，不改写签名 CDN URL。

源码记录中的跨域名账号测试属于上游作者的观测，不是 NekoPak 本次真实账号实测。尤其上游
文档明确说其他根域名的密码登录没有在其观测中验证，因此本仓库不据此承诺所有域名登录可用。
本轮研究的是协议和行为，Rust 代码独立编写，未复制 AGPL Piko 实现或上游 Kotlin 源码。

## 测速和域名选择行为

`network_domains_probe` 为每个候选根域名创建独立、校验证书、禁止重定向的探测客户端，依次读取：

1. `https://api-drive.<domain>/drive/v1/about`，记录首次连接耗时；
2. 相同请求，记录连接复用时耗时；
3. `https://user.<domain>/v1/user/me`，验证认证主机也能到达。

这些请求没有账号 token、设备 ID、CAPTCHA token 或 client ID 请求头。每次响应限 4 KiB，
每步限时 8 秒；只有 HTTP401 且 JSON `error_code=16` 才判为该服务网关的预期未登录响应。
HTML、重定向、成功页面、其他错误码、超时均不能误标可用。界面只展示 Rust 的实际耗时。

`auto` 默认仍从当前固定主域名开始；只有显式测速后才依据本轮可用结果选择最快域名。
手动选择立即作用于后续请求。已发送的远端写入不会因为测速失败自动更换域名重放，CDN URL
不变。测速期间如果用户另改网络或域名，迟到结果不覆盖新选择。域名参数只接受固定枚举，
不接受任意 API origin、URL、端口或通配符。

## 网络恢复和错误分类

HTTP 客户端在构建时捕获系统代理匹配配置。对应版本的实际实现见本机锁定依赖：
[`reqwest 0.12.28 proxy.rs`](https://docs.rs/reqwest/0.12.28/src/reqwest/proxy.rs.html) 和
[`hyper-util 0.1.21 matcher`](https://docs.rs/hyper-util/0.1.21/src/hyper_util/client/proxy/matcher.rs.html)。
因此仅保存相同的 `system` 设置值不足以重读外部变动后的系统代理。

- 显式 `network_reconnect` 重建客户端连接池并递增网络 generation。下一次读账号状态时清除
  上一连接路径产生的短期 CAPTCHA challenge / DRIVE verification，保留系统凭据库里的会话。
- 用户再次发起登录时，系统代理最多每 2 秒重新读取一次；正常元数据请求继续复用健康的 TLS
  连接。连接／传输故障也会为后续尝试重建客户端，不自动重发这次已可能发送正文的请求。
- HTTP408／429／5xx 的分类优先于响应内的 `invalid_grant` 或 `invalid_credentials` 字样。
  临时刷新失败保留 access/refresh token，不销毁会话。
- 下载CDN先分类HTTP状态，再对成功响应核对ETag，避免无ETag的503错误页被误判为文件变化。
  403/404与412/416保持权限／不存在／完整性错误，不进入自动断线重试。
  下载重试最多连续6次，1/2/4/8/16/30秒退避，并持久保存服务端Retry-After等待下限；
  HTTP日期与秒数均可解析，重新连接不能跳过限流等待。已确认字节继续接受来源和块校验。
- 只有真实 `/v1/auth/signin` 交换可以将凭据拒绝映射为账号／密码错误。CAPTCHA 初始化未通过
  显示验证／连接问题，因为当时密码尚未发送；刷新拒绝显示会话过期。原始服务诊断不会跨 IPC。

## 账号记忆

`auth_preferences_get/update` 的数据为 `remember_account`、`restore_session` 与可选 `username`。
偏好和邮箱位于已有 OS 加密凭据库账号索引中，不在 SQLite、普通配置或 WebView localStorage。
关闭“记住账号”清除保存的用户名。密码仍只用于当前请求，不进入偏好或会话记录。

关闭“保持登录”在下次启动时先把当前会话安全归档再撤销自动激活，保留手动选择的已保存账号；
不会中断当前已经登录的会话。重新启用可恢复后续启动的会话自动恢复。旧索引缺少偏好字段时
使用默认值，损坏索引失败并保留，不重建空记录。

## 已执行验证

2026-11-10，Windows x64，合成账号和本机 HTTP 服务；没有真实密码或真实账号操作：

- provider 默认测试套件通过，包含凭据错误分类、临时刷新失败后无密码恢复、网络重连后
  清除旧 challenge 且保持账号、加密用户名偏好和关闭／重新启用自动恢复。
- 新增受控错误传输测试：先给客户端注入已失效代理，确认请求失败且密码没有到达测试服务；
  下一次用户尝试在同一进程成功，signin 只实际发送一次。没有改系统代理或全局环境变量。
- 域名路由测试覆盖四个允许根域名、相似恶意主机与签名 CDN URL 保持原值；探测测试确认
  不发送账号请求头，并拒绝 HTTP200、错误网关码和生产探测客户端上的明文 HTTP。
- `cargo test -p nekopak-provider-native --locked --offline` 与针对该 crate 的 Clippy 结果
  由本轮最终检查日志记录，不把忽略的真实服务／Windows 凭据 opt-in 测试当通过。

未执行真实用户的四域名登录／刷新／CAPTCHA验收；测速只证明当次匿名网关请求，不证明实际
下载速度、会员额度或任意网络环境中的可用性。
