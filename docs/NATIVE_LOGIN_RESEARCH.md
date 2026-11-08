# 原生登录的公开 Web 客户端协议核对

核对日期：2026-11-08。范围：为用户可亲自登录测试的 NekoPak 版本核实客户端参数与
请求形状。本文没有进行真实账号登录、假密码登录、验证码提交或账户变更。
这是一份客户端兼容协议观察，不是 PikPak 官方通用 REST API 的承诺。

## 直接结论

当前官网 Web 客户端公开使用 `client_id = YUMx5nI8ZU8Ap8pm`。官网登录 SDK
初始化没有配置 `clientSecret`，邮箱密码登录也没有添加 `client_secret`。
因此，“必须让部署者自行提供已注册 client ID/secret，否则禁用登录”不是这条公开
Web 登录路径的必要条件。NekoPak 可以内置经核对的公开客户端标识，独立实现请求，
让用户在应用中亲自输入自己的账号与密码。

这里的公开客户端 ID 标识软件，不是用户密码、访问令牌或可独自登录账户的凭据。
用户密码、验证码、会话 token 仍必须严格保护。公开可读不等于取得官方背书或通用
第三方 API 授权；不能因此规避账户控制、配额或人工验证。

## 一手来源与追溯

官网入口 [mypikpak.com/drive/](https://mypikpak.com/drive/) 当前加载的主脚本及
其 webpack 分块给出了以下内容。脚本包含构建标记 `v2.30.3-9-gcc540f8e`。
这些公开浏览器脚本的源码许可证未建立；仅核对协议事实，没有将它们的实现复制到 NekoPak。
缓存只位于被 Git 忽略的 `target/login-audit/official-*`，不应进入发行包。

- [3765.9a7a52c0.js](https://mypikpak.com/drive/3765.9a7a52c0.js)：
  `loginInitOptions.clientId` 为上述公开 ID；`appVersion = 1.0.0`；
  `captchaRedirectPage` 为当前 Web origin 的 `/loading`；登录 API origin 为
  `https://user.mypikpak.com`。SHA-256：
  `0914dc27fc415a5915a46dfda6b7a5d0cb8716bf066940b7bbaae41f16bdfedc`。
- [use-login.033add02.js](https://mypikpak.com/drive/use-login.033add02.js)：
  实例化 `OAuth2Client`、`Captcha`、`Auth`；这条 Web 初始化路径未配置
  `clientSecret`。SHA-256：
  `bd9d55a0d04e3678bd0791aac14410237307c4840ca8e3a37de6568497e052d7`。
- [7991.9cc3c273.js](https://mypikpak.com/drive/7991.9cc3c273.js)：
  `Auth.signIn`、`OAuth2Client._defaultRefreshTokenFunc`、公共请求头生成器。
  SHA-256：`4f3af4dcfb21e62ae1f078e82fd7c6015e9218800c6e8c4b739d6ac6ebaf3622`。
- [684.56935e84.js](https://mypikpak.com/drive/684.56935e84.js)：
  CAPTCHA init、token 缓存、人机验证 URL 和完成回调处理。
  SHA-256：`fbac7afd41c030bc361bba3d81504072f2a3bfc289dcc099d715551a253987ba`。
- [pikcloud_login_ui_component.0ef07aff.js](https://mypikpak.com/drive/pikcloud_login_ui_component.0ef07aff.js)：
  邮箱密码表单调用 `signInRequest: {username, password}`。
  SHA-256：`3ee54972bf3ef3686f4c0f3cd988d7819f7cf0da3a3d320241a176c658c36dcf`。
- [3877.4df17470.js](https://mypikpak.com/drive/3877.4df17470.js)：
  另含 DRIVE 请求签名元数据：相同 client ID、`clientVersion = 2.0.0`、
  `packageName = mypikpak.com`、时间戳和公开算法参数。
  SHA-256：`1dd6062fa40e9e51df5ab5268bb090b580c4595df70d1e7c6a8e450988b39930`。
- [5492.76e680dd.js](https://mypikpak.com/drive/5492.76e680dd.js)：
  使用上述 DRIVE 元数据生成 `withCaptchaMeta` 的文件服务请求包装器。
  SHA-256：`3343d4a37d421070c3f3b9b80c1d3c60e0f9e84ee2a6ffc75ca0198f1af5dba7`。
- [main.a17bccef.js](https://mypikpak.com/drive/main.a17bccef.js)：入口和动态分块映射。
  SHA-256：`e945e8e14157f548b59f48a063f6f1a90942e43c232a03b4f754cc1740e0040f`。

完整本地缓存清单为 `target/login-audit/official-source-manifest.json`。

## 初次邮箱密码登录

官网表单 → `Auth.signIn` 的实际顺序是：

1. 用户提交 `username`、`password`。
2. SDK 增加公开 `client_id`；对于包含 `@` 的账号，人机验证 metadata 使用
   `email` 字段，**不是** `username`。以 `+` 开头的账号用 `phone_number`；
   其他账号形式才用 `username`。
3. CAPTCHA 层为登录操作请求 token；通过后把 token 放进 `x-captcha-token` 请求头。
4. `POST /v1/auth/signin`，JSON 请求体为以下形状：

```json
{
  "client_id": "YUMx5nI8ZU8Ap8pm",
  "username": "<用户在 NekoPak 输入的邮箱>",
  "password": "<本次请求使用的密码>"
}
```

这条官网 Web 路径没有自动添加 `grant_type: password`、`client_secret` 或请求体中的
`captcha_token`。不要把不同 Android 社区客户端的请求体直接视作 Web 路径。

在已经核对的 SDK 调用链里，初次登录的 `withCaptchaMeta` 就是上述 email/phone/username
字段，没有使用 DRIVE 签名的 salts 数组或 `captcha_sign`。后续 DRIVE 风控请求是另一条路径。

## CAPTCHA init 与人工验证

`POST https://user.mypikpak.com/v1/shield/captcha/init` 的 JSON 形状：

```json
{
  "client_id": "YUMx5nI8ZU8Ap8pm",
  "action": "POST:/v1/auth/signin",
  "device_id": "<本客户端独立生成的 32 位设备标识>",
  "meta": { "email": "<用户在应用输入的邮箱>" }
}
```

已有 token 时，SDK 还传 `captcha_token`（初次没有时不传）；不是把 access/refresh token
放进这个字段。后续 DRIVE 研究更正：`action` 由 HTTP method 与解析后的 pathname 组成，
不含 origin/query。早期全 URL 推断错误，详见 DRIVE_PROTOCOL_RESEARCH.md；当前实现已更正。

响应处理在 `684.56935e84.js` 中可直接观察到：

- 有 `captcha_token`、无 `url`：保存 token 与 `expires_in` 后发送原始登录请求。
- 有 `url`：进入人工验证，即使同时存在 `captcha_token` 也不能视为验证完成。
  SDK 可先保存初始 token，但随后打开验证 URL，并追加 `redirect_uri` 和 `state`。
- 验证页面完成后，SDK 从最终回调 URL 的 query 中读取**新的** `captcha_token` 与
  `expires_in`，保存它们，再继续原始登录请求。
- 既没有 URL 也没有 token：协议错误，不伪装成功。

官网 Web 的回调落点为 `https://mypikpak.com/drive/loading`。原生客户端如果采用系统
浏览器，应实现自己可接收的回调；使用仅绑定 loopback 的临时监听器和不可猜测的
路径/state，是独立实现这段衔接的合理设计。必须在收到正确回调后才能将验证视为完成。
本次只读源码研究没有验证服务是否接受原生 loopback redirect，因此不能宣称回调成功。

“用户点了已完成，然后丢掉初始 token、重新 init”不是官网已经证明的成功衔接，可能重新
进入验证。密码可以按 NekoPak 的当前约定在等待人工验证时丢弃，收到回调后再让用户重新
输入；等待期只保留限时挑战元数据和必要的验证 token，不保存密码、不把 token 打日志。

SDK 没有给出固定的人机验证 URL 域名列表，URL 来自 init 响应。当前 NekoPak 的
PikPak 域名 allowlist 是保守约束，不能声称已覆盖实际服务的全部验证域名。

## 请求头与版本的准确含义

官网 SDK 公共请求头生成器给出：

- `x-client-id: YUMx5nI8ZU8Ap8pm`
- `x-sdk-version: 8.1.4`
- `x-protocol-version: 301`
- `x-client-version` 来自登录初始化的 `appVersion`，当前为 `1.0.0`。
- `x-device-id` 来自当前本地设备标识。
- Auth/Captcha 层添加 `accept-language`；登录原始请求再添加 `x-captcha-token`。
- JSON transport 设置 JSON Content-Type。

统计配置还可提供 `x-device-name`、`x-device-model`、`x-os-version`、`x-platform-version`、
`x-device-sign` 等字段。网页的 `enableLocalDeviceSignInWeb=true` 允许它用新生成的本地
随机 UUID 建立 Web 设备标识；没有必要复制用户现有设备 cookie 或伪造 Android UA。
这些字段的源代码存在不证明服务逐项强制要求，NekoPak 应保持实际平台/自身 UA 真实。

**不要混用版本**：登录头的 `1.0.0` 与 DRIVE 签名 metadata 的 `2.0.0` 来自不同配置。
复制后者到初次登录并不能证明更兼容。

## 会话刷新

官网 `_defaultRefreshTokenFunc` 使用 `POST /v1/auth/token`：

```json
{
  "client_id": "YUMx5nI8ZU8Ap8pm",
  "grant_type": "refresh_token",
  "refresh_token": "<系统凭据库中的刷新令牌>"
}
```

SDK 对任意配置支持 `clientSecret`，但当前 Web 初始化没有提供它；JSON 序列化会省略
undefined 的 `client_secret`。刷新请求同样使用公共 SDK 请求头，还设置内部 `x-action`
诊断字段。后续授权请求将响应中的 `token_type` 与 `access_token` 组合为 Authorization。
这些 token 的保存、轮换与注销应继续由 Rust 和 Windows 凭据库负责。

## 宽松许可社区实现的对照

另外通过 GitHub Contents API 核对了
[`Bengerthelorf/pikpaktui@68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`](https://github.com/Bengerthelorf/pikpaktui/tree/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c)：

- [`LICENSE`](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/LICENSE)
  是 Apache-2.0；Git blob SHA `999160cf62c7a10bfd8e5eafe539757026ad8036`。
- [`src/pikpak/mod.rs`](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/mod.rs)，
  Git blob SHA `39b0c756ec685badeefb93c10476638f26f41530`：其默认配置属于 Android
  客户端组合，登录 body 带 `client_secret`、`grant_type=password` 和 `captcha_token`。
  这能说明社区文档的来源差异，不能替代上面已核对的当前 Web 登录形状。
- [`src/pikpak/auth.rs`](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/auth.rs)，
  Git blob SHA `ef3c8b3ad11f4d7f688663bd48c08fd1070e1f28`：模型区分 access/refresh
  token、`sub` 和 CAPTCHA token/URL/过期时间。

本次没有复制这些 Rust 实现、Android secret/UA 或 AGPL/GPL 客户端代码。
raw.githubusercontent.com 仍偶发连接重置，GitHub Contents API 已成功返回上述 pinned 内容；
不能继续把上次网络失败当作这轮协议研究未完成的理由。

## 可交付与仍需用户验证

开发侧现在有足够一手证据提供默认公开 Web client ID、正确邮箱登录请求、请求头、
token 刷新以及人工 CAPTCHA 回调的独立实现，不应再交付默认无法点击登录的测试包。
用户在应用中完成真实密码与人工验证后，才能确认实际账号成功、会话过期、验证码回流和
真实文件分页。这里的源码核对不等于已通过这些真实账号验收，也不证明无需人工验证。

## TLS 初次对照与后续不稳定复验

同日的初次成对测试显示：TLS 1.2 不广告 ALPN 曾成功，广告 `http/1.1` 的对照失败
10054。但随后主代理与研究代理重复运行**同一可执行文件、同一配置**时，成功路径也失败。
因此不能将 ALPN 宣传为已经确认的唯一根因，也不能将预配置 TLS connector 标为稳定修复。
重置来自服务端还是中间网络，尚未确定。

初次实际观测（均未提交账号或密码）：

- Python `ssl` + `socket`，min=max TLS 1.2、不设置 ALPN：曾完成握手，协商
  `ECDHE-RSA-AES128-GCM-SHA256`，向 CAPTCHA init 发 HEAD 得 HTTP 501。
  增加 `set_alpn_protocols(["http/1.1"])` 的对照失败 10054。
- 同一 native-tls 0.2.18 / schannel 0.1.30，用 Rust blocking TcpStream：
  无 ALPN 曾得到 HTTP 501，有 ALPN 对照失败 10054。
  实验文件：`target/login-audit/direct-native-tls.rs`。
- 同一 reqwest 0.12.28（native-tls-alpn），不含账号的 CAPTCHA init
  `{client_id,device_id,action,meta:{}}`：普通 TLS 1.2 + http1_only 失败，
  预构建 native TLS connector 不请求 ALPN 曾得到 HTTP 200，响应含 captcha_token 字段，
  无 url 字段。token 未打印或保存。实验文件：`target/login-audit/reqwest-no-alpn.rs`。

随后研究代理在一个 PowerShell 调用中依次执行：

```powershell
& .\target\login-audit\reqwest-no-alpn.exe
& .\target\login-audit\direct-native-tls.exe
& .\target\login-audit\reqwest-no-alpn.exe
```

结果依次为：reqwest 默认 10054 / 预配置 HTTP 200；direct native TLS 两种配置均 10054；
再次 reqwest 两种配置均 10054。中间没有修改任何环境变量。此时 HTTP_PROXY、HTTPS_PROXY、
ALL_PROXY、NO_PROXY 的大小写形式均不存在；DNS 仅返回 `43.160.170.231`。
这不能用简单的“两个 shell 代理配置不同”解释。

接着四次新建 Python SSL context、min=max TLS 1.2、无 ALPN 的 HEAD 检查，彼此间隔
2 秒，全部在 63–94ms 内返回 10054；之后同一 reqwest 二进制两种配置也都失败。
记录位于 `target/login-audit/tls-repeat-observations.json`。这些结果说明原来的成功现象
不可稳定复现，不能据此承诺真实登录连通性。

仍然成立的库源码事实：reqwest 0.12.28 的
[`src/async_impl/client.rs`](https://github.com/seanmonstar/reqwest/blob/v0.12.28/src/async_impl/client.rs)
第 531–544 行，在 native-tls-alpn feature 下仍为 Http1 分支调用
`request_alpns(&["http/1.1"])`；第 618 行的 BuiltNativeTls 分支跳过该注入。
`use_preconfigured_tls` 是正常公开接口，但绕过 ALPN 注入**不等于解决此次不稳定连接**。

所有实验保留证书链、域名校验；未改系统代理/DNS/Hosts，未执行 signin、虚假密码请求
或验证码完成提交。当前失败发生在连接/TLS 阶段，尚未到达账号密码校验。
后续如加入重试，应严格区别发送前的连接失败与收到 HTTP/登录错误后的重放，不能盲目
重复副作用请求或通过修改身份/关闭 TLS 校验来“修复”连通性。
