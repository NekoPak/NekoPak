# 桌面客户端整体集成记录

2026-11-08，Windows x64。承接原生直连接入与桌面体验工作，将原先只有前端提案/Mock的
主要功能接入Rust Core。源码工作区包含此前全部修改；不把离线HTTP、系统测试或界面
夹具当作真实账号全功能验收。用户已澄清本机仍需代理登录/使用；未设置Hosts规则能用，
不构成无代理直连成功证据。

## 已实现并接通的范围

- 多账号原生登录、切换、移除、系统凭据库迁移与账号范围偏好。添加账号失败/CAPTCHA
  等待不销毁当前账号；跨账号迟到响应和副作用被epoch/scope约束。
- 文件操作、完整分页、最近/星标/回收站，永久删除只允许已在回收站对象并核验消失。
  目录递归下载、批量重命名/撤销、内容重复扫描、空间统计。危险名称和循环树失败明确。
- 云下载预览、创建、真实任务查询、取消和原任务重试；分享创建/取消、分享目录浏览/
  选择转存与未转存原文件读取。分享令牌/签名URL只在Rust，持久任务来源进OS凭据库。
- 原文件下载与目录上传/分片上传：真实字节、长度/身份、持久计划、暂停/恢复、重启恢复、
  本机速度、限速、不覆盖既有目标。上传finish确认云端GCID/size/phase后释放临时授权。
- 纯Rust媒体探测、真实关键帧选段和MP4/MKV/TS转封装；H.264/HEVC、多AAC音轨、B帧/CTTS
  与音轨延迟有真实生成样本和解码/帧hash测试。支持TS/MKV→fast-start MP4，不捆绑FFmpeg。
  Rust媒体队列保存无秘密计划，暂停重启重新封装并复用版本化块缓存。
- 本机循环回环媒体流、应用内video/image、SRT/VTT字幕、倍速、位置恢复、图库/信息流、
  外部播放器选择。流验证Host/Origin/账号/Range/版本；图片会话限并发并及时释放。
- 本地目录只读映射，取消映射不删文件。来源门控覆盖工具栏、菜单、键盘、拖放和IPC，
  不把本地/分享句柄发给普通云端修改。
- 压缩包浏览、解锁、选择解压与持久任务；密码为短期Rust引用，页面不持久保存。
- 安全单向/双向同步：扫描与三方基线、差异预览、冲突拒绝、更新先备份、显式删除确认，
  完成子任务后才更新基线。自动同步默认关闭，间隔/退避持久化，删除/冲突等待确认。
- Windows Cloud Files真实同步根/占位/水合/断开重连；WinFsp外部运行时只读盘符适配与
  能力检测。启动执行真实尾/头/中非连续读取，账号切换与挂载生命周期串行。
- 引用归档：完整清单→真实云清单上传→本地SHA256/云GCID/长度/读回字节核验→显式逐文件
  移回收站；优先恢复原对象，GCID重建需另行授权，pending/未知创建保留回执且不重发。
- 五语词典、多标签、独立历史/选择/滚动、托盘/图标、原生目录选择、自启动、应用内代理/
  TLS兼容；用户要求的access域名屏蔽独立默认关闭，不改系统Hosts，精确匹配四域名。

## 真实执行的检查

- `cargo test --workspace --locked --offline`：结果汇总在
  `evidence/full-client-rust-tests.json`（当时157通过、0失败、18显式ignored）。之后引用归档
  独立复核修正增加1个Core崩溃回执和1个native phase测试，均由定向测试实际执行通过；
  最终整体计数以最终更新的该文件为准。
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`、Rust格式检查通过。
- 显式配置已核验本机FFmpeg仅作为oracle，`nekopak-media --test native_integration
  -- --include-ignored` 8/8通过：稀疏读取选段、全部帧hash、多轨解码、整片TS→MP4。
- `nekopak-core --test media_tasks -- --ignored` 1/1通过：本地映射探测→建持久媒体任务→
  暂停→重启→恢复→真实输出可解码→原目标保护。同一测试捕获并修正Windows发布名乱码。
- `nekopak-mount native_register_populate_hydrate_reconnect -- --ignored --nocapture` 真实
  Windows 1项通过：独立子进程字节核验、重连、新增项、拒绝版本替换，测试根注销清理成功。
- `npm test` 114 Vitest + 6 wire通过。Edge完整`npm run e2e` 57/57通过、外部请求0；
  其中49个Mock场景、8个明确IPC夹具，不能用它们证明真实OS/服务动作。
- `node tests/e2e/native-functional.mjs` 真实WebView2+Rust HTTP+SQLite/文件系统三组通过，
  扩展到云任务、分享、压缩包、媒体导出/解码、loopback bytes、本地映射、语言、网络与
  access开关；合成账号与本机HTTP服务器，未使用用户凭据。
- `node tests/e2e/native-startup.mjs` 4/4通过；fixture exporters通过。
- `python -m unittest discover -s tests -p test_parity.py` 4/4通过；签名测试1项验证拒绝
  文件改动和版本篡改；实际压缩包签名/验签通过。

## 发布、许可与多平台

代码按MIT主仓独立实现，未复制AGPL Piko源码。新协议与固定来源分别记录于
`NATIVE_EXTENDED_PROTOCOLS.md`、`SHARED_MEDIA_PROTOCOL_RESEARCH.md`、
`REFERENCE_ARCHIVE_PROTOCOL_RESEARCH.md`。实际依赖许可进入发行包，不捆绑媒体/驱动/CLI。
WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos，
https://github.com/winfsp/winfsp 。依FLOSS例外使用用户安装运行时，未打包其DLL。

发布仓库为NekoPak/NikoPak，origin已设置，签名公钥入库、私钥为GitHub Actions Secret及
受保护本机备份。签名是Ed25519发布清单验证，非Windows Authenticode证书。Actions准备
Windows检查/签名构建和Linux/macOS核心检查，Android在桌面稳定后适配，未假造手机构建。

## 尚须保留的验收边界

真实用户账号/实际地区代理、Free/Premium配额与弱网完整验收属于用户在应用内操作。
本机无WinFsp运行时，因此只有其ABI/回调离线测试，未声称真实驱动挂载成功。Cloud Files
只允许新增条目刷新；旧内容改变保留旧根并拒绝静默重写。Linux/macOS/Android未做设备验收。

媒体不能支持任意编码/容器：fragmented MP4、复杂edit、无Cues或压缩/加密MKV、部分VFR/
多program TS等明确拒绝；WebView解码能力取决于系统。视频帧截图没有解码器时保持不可用。
分享子集磁力下载、分享转码/压缩包内读取、跨设备归档清单导入、缺失父目录的自动重建、
自动更新安装、完整索引增量仍不能标成全上游验收。要求继续保留在parity-manifest；
当前构建是广泛集成版，不能仅凭界面有入口宣称所有24组已验证。

完整构建/体积/签名哈希以 `evidence/integration-size-report.json` 和
`evidence/integration-checks.json` 为准。不会用体积目标删除TLS、校验或持久化。
