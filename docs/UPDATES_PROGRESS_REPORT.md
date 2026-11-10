# 0.1.3 应用更新与实时下载进度

2026-11-10。本版实现Windows x64／Android ARM64应用更新，并修复用户反馈的“手机有速度、
进度条不动”。0.1.2的吞吐、种子选择和软键盘修复保留。仍为Pre-alpha预览，真实账号与
物理手机验收尚未完成，不代表24组上游功能已全部通过。

## 用户可以执行的流程

在**关于 → 应用更新**检查公开预览版、阅读说明、下载或取消、确认安装。启动检查可选，
默认关闭。网络／签名失败显示可重试错误，不显示“已经最新”；安装失败不会被状态轮询抹除。
更新与PikPak登录无关；所有网络由Rust服务执行，前端只显示状态并提交update_id。

Windows暂停本地传输、打开正常NSIS向导后退出。便携版原目录保留，向导安装当前用户副本；
应用数据仍在既有目录。Android先核对签名和包身份，必要时打开本应用的安装授权页，返回后
再次点击安装进入系统确认。不自动授予权限、不静默安装。0.1.2及以前没有更新器，首次升到
0.1.3需手动安装；之后的较新公开预览版可由此入口取得。

## 进度问题的复现

旧版本Rust已经收到65,536字节但transferred_bytes仍为0，快照没有块内位置，前端进度保持0；
速度采样却直接来自网络接收。下载请求最大4MiB，在慢网络上要等完整块才跳一次。
新增`received_bytes`只在当前revision的running普通下载快照出现；手机及桌面据此显示
字节、百分比和ETA，手机百分比与速度同时可见。字段未持久化，transferred_bytes继续代表
完成块验证并落盘的可靠位置。暂停、失败、重试与新revision不会误用旧块位置；任务完成仍
由核心校验／发布状态决定。暂停后显示回到已验证位置属于实际恢复语义。

该回归测试在修复前失败，修复后通过。Android原生套件还等待“持久进度不变、实时字节增加”
的实际HTTP区间并检查WebView进度条确实移动。没有按时间伪造进度，也未把服务端云任务／
视频重封装百分比改成网络字节语义。

## 更新完整性和发布

Rust固定使用NekoPak/NikoPak公开release列表；接收较新、平台匹配、Ed25519签名通过的
update-manifest.json。只允许固定GitHub HTTPS主机，保留TLS验证、超时、长度及重定向限制，
沿用应用代理。公钥来自release/release-signing.pub，私钥从未进入源码、参数或日志。
清单原始payload字节包含产品／schema／版本／通道／平台／文件名／长度／SHA256及更新说明。
下载验证通过才ready，安装前再读文件验一次。Windows固定文件和父目录至spawn；Android再
校验APK包名、显示版本、递增versionCode和相同证书集合，然后只读分享files/updates下的APK。

操作互斥锁覆盖检查、下载与安装核验。每次检查撤销旧候选，清理应用私有updates目录中本
更新器UUID命名的包；不递归、不跟随链接、不删除其他文件。重启不自动信任残留包。
失败／取消下载删除半包。APK证书轮换暂不支持，最大更新包256MiB；检查45秒有界，下载
有界且可取消。本协议与Tauri updater插件和Windows Authenticode不同，不能混称。

发布工作流先构建并验签两平台，干净Windows runner检查安装器，再签署更新封套、完整上传
draft，最后公开为prerelease。每份来源报告也包含于平台签名，两个报告必须为同一个干净
HEAD，发布工作区必须干净。已有版本／异commit同名tag不可覆盖。两处来源绑定问题由
独立安全复核发现，真实合成签名测试复现后修复；复核未留下可操作安全发现。

## 实际验证

- `cargo test --workspace --locked --offline -j2`：194通过、0失败；原19个环境相关测试明确
  ignored。额外公开源验收为ignored测试，发布后显式运行，不在日常离线CI偷偷联网。
- `cargo clippy --workspace --all-targets --locked --offline -j2 -- -D warnings`、
  `cargo fmt --all --check`、`npm run build`通过。
- `npm test`：118单元＋6 wire通过；`E2E_BROWSER_CHANNEL=msedge npm run e2e`：57/57。
- `node tests/e2e/mobile.mjs`：12/12，360/390/412宽度与明暗主题，含同revision实时进度、
  更新开关、检查／下载／安装确认／权限提示，以及安装错误在后续轮询仍可见。
- `python -m unittest discover -s tests -p 'test_android*.py'`：20/20。
- `python -m unittest discover -s tests -p test_update_manifest.py`：3/3。
  独立审查另验证干净准备成功、已签脏构建拒绝、脏发布工作区拒绝、签后报告篡改拒绝。
- `node tests/e2e/update-native.mjs`：Windows7/7；真正Rust服务＋HTTP合成release/key，
  测试无更新／网络失败／坏签名／取消／篡改包／成功下载／启动无副作用测试EXE并退出。
- `NEKOPAK_TEST_UPDATE_APK=target/update-candidate.apk node tests/e2e/update-native.mjs --android`：
  Android7/7，包括真实安装权限页和系统最终“更新NekoPak”确认。未点击安装；测试候选
  0.1.4／104仅存在隔离模拟器测试目录、沿用debug签名，绝不作为公开发行包。
- `node tests/e2e/android-native.mjs`：17/17，含当前块进度、密码键盘底栏、真实种子SAF、
  JNI Keystore、实际速度、断线恢复、后台暂停、重启恢复、相同SHA256的下载及导出。

原生测试使用专属Android15/API35 x86_64模拟器和本地合成账号HTTP。release ARM64另通过
相同签名升级、R8压缩后冷启动验收；最终产物与公开源测试结果在下节落地后记录。

## 来源、文件与限制

独立实现，无AGPL复制。ring0.17.14和semver1.0从既有间接依赖成为Core直接依赖；reqwest、
SHA256、base64、AndroidX/Tauri均为既有库。完整许可随包，见THIRD_PARTY_NOTICES.md。
参考平台机制：[Android FileProvider](https://developer.android.com/reference/androidx/core/content/FileProvider)、
[canRequestPackageInstalls](https://developer.android.com/reference/android/content/pm/PackageManager#canRequestPackageInstalls())、
[GitHub release列表](https://docs.github.com/en/rest/releases/releases#list-releases)、
[锁定Tauri PathPlugin](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/mobile/android/src/main/java/app/tauri/PathPlugin.kt)。
实际原生行为以以上本轮测试为证，不将文档引用当设备验收。

主要文件：`crates/nekopak-core/src/updates.rs`及downloads/rates/settings，domain增量字段；
`src-tauri/src/update_commands.rs`、`src-tauri/android/UpdateInstaller.kt`、窄范围FileProvider；
`src/components/AppUpdates.tsx`、Transfers、reconcile、typed IPC及多语言；
`scripts/publish_release.py`、`.github/workflows/release.yml`和对应测试。

尚未承诺物理手机／真实服务网速、系统任意定制安装器、Android签名轮换、APK自动安装。
Android后台持续下载／普通上传／共享目录同步仍未完成。下一项实际验收是用户在自己的
网络与手机中测试签入、种子任务和文件下载；不能把缺少用户实测当成开发配置待用户解决。
