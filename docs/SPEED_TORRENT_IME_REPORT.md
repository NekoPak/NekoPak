# 0.1.2 下载速度、种子导入与软键盘

2026-11-10。针对用户反馈的异常下载速度、缺少种子文件提交、Android输入密码时底栏上浮。
本版仍为预览，以下受控测试不代表真实账号／物理设备或全部上游功能验收。

## 下载慢的复现与修复

`node tests/e2e/download-throughput.mjs`运行真正的Windows WebView2、Rust provider、SQLite、
下载器和本地HTTP服务；文件16MiB，每次metadata／CDN请求增加40ms延迟，无用户凭据。
旧版本9,263ms，1.73MiB/s；直接读取同样字节89ms。16个Range分别新建连接，块之间约500ms。
这证明下载被宿主轮询时钟和重复连接拖慢，之前的功能检查没有覆盖吞吐上限。

移除每块500ms等待后2,452ms。连续worker、最多4MiB请求、强ETag来源短期复用及有界连接池
全部接入后为1,085ms（14.74MiB/s），约为旧链路8.5倍。metadata请求17→2，CDN连接16→1，
Range16→4，最终SHA256一致。机器、系统TLS与本地测试服务决定此数字，不能据此保证真实网速。

云任务轮询仍独立限频。缓存仅在强ETag实际收到后启用，后续Range带If-Match并核对响应ETag、
范围和长度；无／弱ETag继续更新元数据。来源按账号、表示、会话代次和网络代次隔离，最多
8项、30秒；连接池固定已验证公共DNS解析并保留TLS校验，重连／代理变更淘汰旧代次。
网络每次可读4MiB，磁盘回执仍分为最多1MiB的SHA256块，保持旧持久格式与重启验证。
限速等待减去已经花在传输上的时间，并可被暂停／取消打断。未添加无限并发或绕过服务端限流。

## 种子导入

云下载弹窗增加“链接／种子文件”入口。选择.torrent后显示名称、大小和文件清单，用户确认
才创建云任务。Rust独立解析原始bencode并按BEP3算info_hash，保留tracker、使用已有云下载
协议；同键重复确认复用原任务，切账号／预览过期／损坏文件在写请求前拒绝。
Android content URI和本地路径不进WebView，预览不包含tracker授权。v1／混合v1支持，
纯v2和未知文件名编码明确拒绝。具体研究、限制、来源许可见TORRENT_IMPORT_RESEARCH.md。

## 输入法与底栏

在0.1.1实际Android WebView中复现：系统缩小WebView以避让键盘，但底部主导航继续显示，
因此“文件／传输／我的”被顶到键盘上方。0.1.2由原生WindowInsets报告IME可见性，前端在
软键盘打开时隐藏该导航、收起时恢复；原生返回键先隐藏键盘，页面不后退。仍保留WebView
对键盘和系统栏避让，输入框与提交按钮可以滚动到可见区域。

## 验证与交付

具体命令、检查计数、原生截图和最终产物哈希在`evidence/speed-torrent-ime-checks.json`。
性能脚本对旧版本实际失败后才修改；输入法测试同样先在旧APK捕获底栏错误。
Rust增加强／弱ETag缓存与网络代次测试、4MiB读取保持旧回执及重启续传、种子解析和
确认幂等测试；手机浏览器测试增加种子选择取消、预览和提交。Android原生套件覆盖真实
键盘、SAF种子选择、解析到Rust、确认后仅一次云请求，以及原有下载恢复／导出流程。

主要文件：Core的downloads.rs／torrent.rs／cloud.rs，Native provider的download.rs，
宿主lib.rs／catalog_commands.rs／Android MainActivity.kt，前端mobile.ts／MobileShell.tsx／
dialogs.tsx，以及对应Rust与端到端测试。MIT主仓许可保持，没有新运行时或AGPL代码复用。

真实用户网络速度、真实种子服务结果及物理手机输入法仍需用户在应用内验收。Android后台
持续下载、普通上传／目录同步和整体功能对齐仍未完成；本次修复不降低既有门槛。

## 最终构建与运行证据

Rust工作区188通过、19明确ignored；Clippy与格式检查通过。前端117单元＋6wire，桌面
浏览器57/57，手机布局与种子交互12/12，Windows原生功能3/3，Android原生16/16。
后者包括真正的系统键盘和SAF种子选择器，确认前无云请求、重复确认只创建一次。
失败测试原始记录与最终输出保存在本机target下；结构化证据纳入docs/evidence。

源码`919c4907b79cbea947469b758bdfdb2a0394eb65`的
[Windows／Android签名发布](https://github.com/NekoPak/NikoPak/actions/runs/42799710084)和
[Windows安装卸载](https://github.com/NekoPak/NikoPak/actions/runs/42802008148)均成功。
Android预览及Linux/macOS核心检查也成功。下载实际产物后再次验证Ed25519清单、全部哈希、
APK原证书、原生库16KiB与图标。该远端APK实际从0.1.1覆盖升级并通过两次冷启动，
保持中文与登录入口；运行在隔离模拟器ARM翻译桥上，未使用真实账号。

当前dist产物：Android ARM64 APK17,445,137字节（16.64MiB），Windows ZIP7,209,938字节
（6.88MiB），NSIS安装器4,163,593字节（3.97MiB）。APK SHA256：
`6ccd851bf88d5e56d08394373fda9afacccce73303cd4310d41d84ee265d3553`。
版本号0.1.2／Android versionCode102，证书身份沿用旧版。Windows清单签名不等于Authenticode。
