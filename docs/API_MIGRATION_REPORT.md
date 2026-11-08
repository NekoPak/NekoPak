# CLI 移除与原生 API 迁移报告

> 后续修正：本报告描述最初的迁移包，其“默认登录禁用”已在登录修复版中取消。当前结果见 LOGIN_FIX_REPORT.md。

日期：2026-11-08；Windows x64；基线 commit `b2f2923e0fb22cf82a60d74574d6e8e8d0c1467b`。
本轮是接入路线迁移与 v3 集成，不是完整 M1 或上游功能对齐完成。

## 完成的改动

- 删除 nekopak-provider-cli 全部源码、Cargo 依赖、probe handler、CLI 环境配置读取、
  安装/选择界面。仅保留旧 BackendId 字符串的历史反序列化兼容及拒绝路径测试。
- 新增 nekopak-provider-native：独立 HTTP 会话、人工 CAPTCHA 衔接、串行刷新、
  Windows Credential Manager、退出、只读分页/stat、错误与数据白名单、请求限流/超时。
- 默认桌面 rust_compat，显式 Mock 单独运行、单独数据库；无浏览器 Mock 回退。
- 合入用户 v3 前端和交互工具，保持 Rust 为业务真源；不能执行的操作与选段/画质参数明确拒绝。
- 契约 v0.2，同步 Rust/TS/fixtures；保留十进制 u64 版本与可靠快照。更新 README、
  AGENTS、架构/安全/路线图/协作说明与打包；旧 CLI 报告移至 docs/history。
- 用户既有最终功能不回退要求保持不变，附件自行标“不做”的上游功能恢复为待实现。

主要路径：crates/nekopak-provider-native、crates/nekopak-domain、crates/nekopak-provider、
crates/nekopak-core、src-tauri/src/main.rs、src/lib、src/state、src/pages、src/components、
测试/fixtures、scripts/package_release.py 和 docs。完整文件差异保留在工作区；路径清单见 `evidence/migration-files.txt`。

## 已实际执行的检查

- `cargo test --workspace --locked --offline`：49 项通过，0 失败。另有 6 项外部 FFmpeg
  实验和 1 项 Windows 凭据库测试默认 ignored，不计通过；凭据库测试另行执行如下。
- `cargo test -p nekopak-provider-native --locked --offline windows_vault_roundtrip_and_delete -- --ignored --exact vault::tests::windows_vault_roundtrip_and_delete`：1 项通过；仅临时测试凭据，结束删除，未读取用户账号。
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`：通过。
- `npm run check`：通过；`npm test`：78 项 Vitest + 6 项 wire 测试通过。
- `npm run build`：通过（ui-dist）。
- `cargo run -p nekopak-core --example export_fixtures --locked --offline` 和
  `cargo run -p nekopak-provider-native --example export_fixture --locked --offline`：通过。
- `E2E_BROWSER_CHANNEL=msedge npm run e2e`：47/47 通过、外部请求 0，见 `evidence/ui-v3-e2e.json`；覆盖 12,000 项目录与四档缩放/浅深主题。
- `cargo fmt --all --check`、`git diff --check`：通过。
- `python -m unittest discover -s tests -p test_parity.py`：4 项通过；`python scripts/check_parity.py` 清单有效，0/24 必需功能组完成，正式对齐版仍不允许发布。`python scripts/check_parity.py --release` 已实际执行并按预期失败（缺少候选复核与全部必需能力）。
- `cargo build -p nekopak-app --release --locked --offline`：Windows x64 Release 通过。
- `python scripts/package_release.py`：通过；ZIP CRC、内嵌 EXE SHA-256 与无附带外部 EXE 检查通过。
- 真实 WebView2 点击套件本轮未运行；已更新独立调试入口 `tests/e2e/desktop.mjs`。Tauri 命令的实际 Rust mock-runtime 往返测试已通过，不能代替 WebView2 点击验证。

Playwright 预期的 Chromium revision 未安装；采用已安装的 Microsoft Edge headless，
通过 `E2E_BROWSER_CHANNEL=msedge` 运行。本轮没有下载浏览器，也没有接入真实 PikPak 账号。
拖放初次失败已定位为测试坐标在虚拟列表自动滚动后失效，实际拖了另一行；测试现在先保证两行可见并校验 dragstart/drop 的真实 ID，完整复跑通过。

## 复核与修正

code-review 技能的两个只读复核方向分别检查 Standards/security 与 Spec。
发现并修复：保存会话刷新失败时缺少恢复入口；凭据库失败时登录门控不足；退出后残留
账号 UI 数据；加载全部传入 1000 超出冻结 500 上限（两方向重复发现）。
增加对应回归，两位复核者聚焦复查后确认原发现全部关闭。复核不代替实际测试。

## 未实现与证据边界

没有内置可用服务客户端身份；默认登录禁用。真实 Free/Premium、人工验证码回流、
服务配额、用户信息/会员查询仍需实测，不能宣传开箱可登录。Native 登录成功测试来自
本地 HTTP fixture，Windows 凭据库成功不等于 PikPak 登录成功。
真实云端写操作、上传、完整下载与恢复、远端片段、Range、媒体播放、分享、解压、
同步、挂载均未完成；新界面在浏览器演示可交互，桌面按实际能力禁用。
固定目录当前仅保留当前桌面会话，持久账号范围的偏好需要后续契约；外观偏好保留。
未测试 Linux/macOS、干净 Windows VM、安装器/签名、吞吐或本轮内存基线。

## 许可证与下一步

主仓继续 MIT。没有复制 AGPL/GPL 云盘源码、PikPak 客户端秘密或验证码绕过逻辑。
用户提供的 UI 已保留来源说明和 Material Symbols Apache-2.0 标头；新增 reqwest/rustls、
ring、zeroize、windows-sys 进入锁文件与发布依赖许可清单。CLI 不进入任何新发行包。
重新读取 pinned pikpaktui 源码时网络连接重置，未关闭 TLS 或把失败声称为复验成功。

下一 Issue：开发侧补齐可用的客户端协议配置和可测试版本；真实账号登录、验证码及
登录→刷新→只读分页→退出验收由用户在应用内亲自完成。当前配置缺口属于开发工作，
不能让用户承担协议接入。开发侧根据脱敏结果修复问题，继续补持久远端意图/幂等与真实下载。

## 最终包体与追溯

- 便携开发预览：`dist/NekoPak-0.1.0-windows-x64-native-preview.zip`。
- ZIP：5,361,778 字节（5.11 MiB）；EXE：9,634,816 字节（9.19 MiB）。
- 超过 5 MiB 挑战目标约 116.1 KiB；保留 TLS、SQLite 与可靠性检查，未为体积删减安全能力。
- 桌面启动静态资源 gzip 109.90 KiB；全部可加载桌面资源含按需页面为 138.98 KiB。此数字不包括整程序或 WebView2 内存。
- Windows 构建依赖许可证文本缺口 0；完整多平台依赖图仍保留原先 6 项未使用平台的文本缺口，见 `evidence/native-preview-size-report.json`。
- ZIP SHA-256：`aa785c620ec6a154957dda095fc8b9746eca4fab4afe70795cb1d3a88e3b14d5`。
- EXE SHA-256：`03eb550d4bc27df1fdc2da45ba325c6232659db6becaf2e9aa92fa1f08de8d83`。
- 代码基线 commit 为上述 b2f2923；产物含本轮未提交工作区修改，实际 source SHA-256：`dbfeff997f982573bc0fd5913cc1c446f6687f40dc509ba94a3354f6fb3928c9`。没有把基线 commit 冒充本轮新提交。

机器可读验证汇总：`evidence/api-migration-checks.json`。新增来源、权限边界与未实现项见 API_ADAPTER.md。
