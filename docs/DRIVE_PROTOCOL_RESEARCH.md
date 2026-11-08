# 已登录 DRIVE 请求的人机验证协议核对

核对日期：2026-11-08。触发问题：用户已经登录，但文件列表返回 `captcha_required`，
界面将其显示成普通权限错误，没有继续验证入口。

本轮只读取官网公开脚本缓存与公开源码。没有读取用户凭据库、浏览器存储或账号令牌，
没有发送账户 API 请求，没有验证码解题或新增连通性快速探测。
下述内容是客户端兼容协议观察，不是官方公开 API 的稳定性或接入授权承诺。

## 实现应先掌握的结论

- 列表默认 action 是 `GET:/drive/v1/files`；stat 是 `GET:/drive/v1/files/<id>`。
  默认 action **只包含 HTTP method 和 URL pathname，不包含 origin、query 或 fragment**。
- DRIVE 客户端会提供 `captcha_sign`、`client_version`、`package_name`、`user_id`、
  `timestamp` metadata，然后使用返回的 token 作为原始请求的 `x-captcha-token`。
- `meta.client_version` 为 `2.0.0`；不要由此推断 HTTP `x-client-version` 也应为 2.0.0。
- `user_id` 是当前账号的 `sub`。不能拿邮箱、显示名、文件所有者 ID 或随机值代替。
- `captcha_required`、`captcha_invalid` 需要验证码续流程；普通 403 仍是权限错误，
  `unauthenticated` 仍是会话错误，不能将所有情况混成退出登录。
- init 返回 URL 时，必须让用户完成验证；仅有客户端签名不能算人工验证通过。

## 官网来源、版本与完整性

研究使用之前以未登录请求获取的官网 Web 脚本缓存；缓存构建标记为
`v2.30.3-9-gcc540f8e`。本次重新计算文件 SHA-256 与缓存清单一致。

- [3877.4df17470.js](https://mypikpak.com/drive/3877.4df17470.js)：module 65558
  是公开客户端签名元数据；module 22550 获取当前设备 ID；module 87827 获取当前用户 sub。
  SHA-256：`1dd6062fa40e9e51df5ab5268bb090b580c4595df70d1e7c6a8e450988b39930`。
- [5492.76e680dd.js](https://mypikpak.com/drive/5492.76e680dd.js)：module 42410
  计算签名并包装 DRIVE HTTP 请求。
  SHA-256：`3343d4a37d421070c3f3b9b80c1d3c60e0f9e84ee2a6ffc75ca0198f1af5dba7`。
- [684.56935e84.js](https://mypikpak.com/drive/684.56935e84.js)：Captcha SDK 的
  `_innerRequest`、`_fetchShield`、缓存、错误恢复和 URL 回调。
  SHA-256：`fbac7afd41c030bc361bba3d81504072f2a3bfc289dcc099d715551a253987ba`。
- [7991.9cc3c273.js](https://mypikpak.com/drive/7991.9cc3c273.js)：OAuth 请求层、
  Authorization 和公共 SDK 请求头。
  SHA-256：`4f3af4dcfb21e62ae1f078e82fd7c6015e9218800c6e8c4b739d6ac6ebaf3622`。
- [3765.9a7a52c0.js](https://mypikpak.com/drive/3765.9a7a52c0.js)：module 53456 的
  Web 登录配置；module 25391 的 origin 精确匹配函数。
  SHA-256：`0914dc27fc415a5915a46dfda6b7a5d0cb8716bf066940b7bbaae41f16bdfedc`。

这些脚本没有已核验的开源源码许可证。它们仅作协议行为与公开配置参考，不复制程序实现，
不作为运行时依赖。忽略目录 `target/login-audit/official-*` 只是研究缓存，不进入发行包。

## action 的准确构造

Captcha SDK 先把相对 URL 按配置 API origin 解析，再读取 `new URL(...).pathname`。
如果调用者没有显式传 `captchaAction`，则拼接 `method + ":" + pathname`。
DRIVE wrapper 没有替文件列表另外指定 action。

例如请求：

```text
GET https://api-drive.mypikpak.com/drive/v1/files?parent_id=&limit=500&page_token=opaque
```

对应 action 为：

```text
GET:/drive/v1/files
```

分页 cursor 和过滤条件变化不会改变该 action。stat 的文件 ID 属于 pathname，因此会反映
在 stat action 中；但服务返回的 token 是否可跨 action 使用，不能仅凭 action 字段推断。

**纠正旧笔记：** `NATIVE_LOGIN_RESEARCH.md` 早先把 SDK 默认 action 推断成完整 URI，
这是不准确的；当前缓存 SDK 默认使用 pathname。某条已经实际成功的登录路径接受完整 URI，
不等于官网默认形状如此，更不能用它证明 DRIVE 列表也应传完整 URL。

## 公开签名配置与数学规则

module 65558 的公开元数据为：

```text
clientId      = YUMx5nI8ZU8Ap8pm
clientVersion = 2.0.0
packageName   = mypikpak.com
timestamp     = 1793961611384
```

这里 timestamp 来自已发布 Web 配置。官网 wrapper 在此处直接取配置值，**不是每次调用
Date.now()**。客户端版本、包名、时间戳与算法参数应作为同一版本配置管理，不能随意混搭。

签名的输入拼接顺序：clientId、clientVersion、packageName、当前 device_id、timestamp，
中间没有分隔符。然后依次对“上一步字符串 + 当前 salt”做 MD5，以上一步的 32 位小写
十六进制结果作为下一步字符串。最后加 `1.` 前缀。`user_id` 不在这个签名 seed 中。

当前公开参数数组含 15 项，全部标记为 md5；顺序与空字符串均有意义：

```json
[
  "C9qPpZLN8ucRTaTiUMWYS9cQvWOE",
  "+r6CQVxjzJV6LCV",
  "F",
  "pFJRC",
  "9WXYIDGrwTCz2OiVlgZa90qpECPD6olt",
  "/750aCr4lm/Sly/c",
  "RB+DT/gZCrbV",
  "",
  "CyLsf7hdkIRxRm215hl",
  "7xHvLi2tOYP0Y92b",
  "ZGTXXxu8E/MIWaEDB+Sm/",
  "1UI3",
  "E7fP5Pfijd+7K+t6Tg/NhuLq0eEUVChpJSkrKxpO",
  "ihtqpG6FMt65+Xk+tWUH2",
  "NhXXU9rg4XXdzo7u5o"
]
```

用标准库 hashlib、上述公开配置和合成设备 ID 离线计算的两个测试向量：

- `00000000000000000000000000000000` → `1.d9941f4c807b846f52c4ff39f7becc3a`
- `7e60450667fa41f4838a1bac6ec204da` → `1.4301f15b165ad1427c894397d4682e4e`

这不是服务端验收结果，也不是任何用户设备或验证码答案。

这些值随官网公开静态脚本交付，未登录也可以取得，不依赖任何用户凭据。应将它们记录为
**公开客户端协议元数据**，而非把它们误当成用户密码、访问令牌或服务器保密密钥。
计算这种客户端请求签名也不等于完成人工 CAPTCHA；服务器仍可返回 URL，必须继续由用户
操作。这里不复制 proprietary/AGPL 客户端实现，不引入 Android 身份或其他客户端的 secret。

## 已登录 DRIVE init 的 metadata 与请求头

wrapper 提供如下形状：

```json
{
  "client_id": "YUMx5nI8ZU8Ap8pm",
  "action": "GET:/drive/v1/files",
  "device_id": "<当前 NekoPak 会话设备标识>",
  "captcha_token": "<此前 token，可缺省>",
  "meta": {
    "captcha_sign": "1.<按公开参数计算的摘要>",
    "client_version": "2.0.0",
    "package_name": "mypikpak.com",
    "user_id": "<当前账号 sub>",
    "timestamp": "1793961611384"
  }
}
```

该 JSON 送往 `https://user.mypikpak.com/v1/shield/captcha/init`。
`_fetchShield` 本身只显式设置 accept-language，没有开启 withCredentials；OAuth base
默认为 false，所以官网初次 init 不主动携带 Bearer。OAuth 层遇到 unauthenticated 可以
再尝试带当前凭据。不能把社区实现主动携带 Bearer 的策略声称为这里官网必然执行的行为。

原始已登录 DRIVE 请求带 Authorization 和 `x-captcha-token`，DRIVE wrapper 显式加
`x-device-id`。它不显式设置 HTTP `x-client-version=2.0.0`。
OAuth base 只对相对 URL 或匹配其 apiOrigins 的 URL 加通用 SDK 统计头；官网配置的
apiOrigins 是 `https://user.mypikpak.com`，其 origin 匹配函数使用精确比较。
因此该分支不会为绝对 api-drive URL 自动增加这些统计头。对相对 auth-host init URL，
公共头仍来自登录配置：x-client-version=1.0.0、x-sdk-version=8.1.4、
x-protocol-version=301、x-client-id。**HTTP 头与 metadata 的版本含义必须分开。**

## 当前用户 ID 与旧会话平滑补齐

官网 module 87827 读取当前 client 的 credential 记录中的 `sub`，并用它填 `meta.user_id`。
这只是说明网页自己的字段来源；NekoPak 应使用自己 Rust 核心持有的登录/刷新结果，
不读取其他客户端或浏览器的账户存储。

已通过公开源码交叉确认：

- [Rclone api/types.go](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/backend/pikpak/api/types.go)
  436–450 行明确 `GET https://user.mypikpak.com/v1/user/me` 返回 User，用户 ID 字段
  是 **sub**；582–589 行 Token 模型也包含 sub。Git blob SHA：
  `c97829cfb20c5f4f97cccaa67a9c32c3c1f252e7`。
- [Rclone COPYING](https://github.com/rclone/rclone/blob/82cb84b2014bec6a167dc28eb53233b2d4abe1b3/COPYING)
  已读取并核对 MIT 文本；Git blob SHA `8c27c67fd0a12a1e18db983f19450abed435e689`。
- [pikpaktui account.rs](https://github.com/Bengerthelorf/pikpaktui/blob/68b7a6bdf6e4abe2a049974dec2c38e736f2a25c/src/pikpak/account.rs)
  37–44 行同样使用 Bearer GET auth-host `/v1/user/me`。Git blob SHA：
  `1428a0a5e68f667cc293567ed35963c9656f382c`。许可证为既有审计中的 Apache-2.0。
- [pikpak-kotlin UserEndpoint.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/UserEndpoint.kt)
  定义 UserProfile.sub，注释与 Session.sub 相同；Git blob SHA
  `b303d646439b2d71f7e30856aadef7371153b0d7`。
- [pikpak-kotlin AuthApi.kt](https://github.com/NihilDigit/pikpak-kotlin/blob/8eaf5587a883ed4ea9bcca7b43a9194118719e63/src/commonMain/kotlin/io/github/nihildigit/pikpak/internal/AuthApi.kt)
  刷新 endpoint 为 `/v1/auth/token`，刷新和登录都经同一 toSession 解析器读取 sub。
  Git blob SHA `a536159ff8e5fa70969fbf80634686929856dde0`。许可证见既有 MIT 来源审计。

以上模型使用可缺省/空串的 sub，不能据此保证每一次刷新响应都一定带非空 sub。
NekoPak 可新增可选的持久 `user_id`：新登录/刷新有非空 sub 时保存；刷新缺字段时保留
已知值；旧会话缺值时由 Rust 使用自己的现有会话请求 `/v1/user/me` 并读取 sub。
正常旧会话不应仅因缺少新字段被清空或要求用户重新登录。不能用 UserInfo 的 name、email
或 DRIVE 文件的 owner/user_id 代替当前账号 sub。

## token 缓存与错误恢复

当前官网 SDK 缓存不是源码名字暗示的严格“每 action 一份”：

- local storage key 为 `captcha_<clientId>`，只存一个当前记录。
- 记录含 action、meta、captcha_token、expires_in、expires_at；getter 不按 action/meta
  匹配，`genCaptchaTokenStorageId` 返回固定空输入的散列，single-flight 对同一 client 全局共享。
- expires_at 按 `now + (expires_in - 30) 秒` 保存，过期则 init。
- 非过期 token 可在不同路径请求间复用；服务器拒绝时再更新。

NekoPak 可以采用更严格的内存隔离，但必须区分本项目设计和上游观察。优先按当前账号、
device_id、client profile、session generation 隔离并在退出/切换时清除；不要复制官网的
全局浏览器存储实现。若按每个 stat ID 强制重新 init，可能带来不必要的请求或人工验证。

SDK 以 `error == captcha_required || error == captcha_invalid` 判断需要更新，不依赖
具体 HTTP 400 或 403。收到这类错误会更新 token 再尝试原请求；源码限制
`MAX_INIT_CAPTCHA_TOKEN_TIMES = 3`。这是客户端的有界恢复策略，不是无限重试许可。
NekoPak 应保留厂商错误的安全枚举含义，将普通 PermissionDenied 与需要验证区分。
对文件列表/stat 可在验证完成后有界重放同一个 GET（保持原始分页/筛选参数）；未来写操作
仍必须遵守持久意图/幂等约束，不能照搬任意重试。

## 人工验证 URL 与 callback

init 响应带 url 时，即使同时含 captcha_token，也进入人工验证。官网先保存初始 token，
打开服务返回 URL，追加 `redirect_uri` 与 `state`；验证结束后从最终回调 query 读取新的
captcha_token 与 expires_in，再继续原请求。人工取消会作为 cancelled 返回，不是登出。
服务器不需要人工时，init 直接返回 token，之后继续 GET。

SDK 没有硬编码完整的验证 URL 域名列表；URL 由响应提供，本次未发账户请求，因此不能
声称已核验用户此刻收到的域名。应保留 HTTPS/host 校验与安全 opener，未知域名明确报错，
不能静默改为任意 URL、iframe 注入或执行脚本。

NekoPak 现有已登录会话应保持 signed_in。推荐把 DRIVE 验证做成独立、可查询/完成/取消的
verification 流程，token 与回调继续只在 Rust 内保存；前端得到可操作的验证入口和公开状态。
用户完成人工验证后继续原来的只读请求，不需要再次提交账号密码，也不把验证码错误伪装成
永久权限不足。此段是 NekoPak 的实现建议，具体 IPC 仍须按共享契约协调。

## 验证边界

本轮验证的是公开源码/配置和两个纯离线签名向量。没有调用用户账户接口，没有取用户 token，
没有尝试解决 CAPTCHA，没有复用 proprietary/AGPL 实现。真实目录恢复和人工验证流程需要
由用户在更新后的应用里验收；测试之前不能宣称这条流程已对真实账号通过。
