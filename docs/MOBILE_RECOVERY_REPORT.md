# 0.1.1 手机界面与连接恢复

2026-11-10。接续上一窗口中断的手机重做、图标、登录与下载恢复工作。
本版为预览；离线协议、实际字节和模拟器验证不等于真实账号或上游全部功能验收。

## 行为变化

- 手机采用文件／传输／我的三个底部入口、触屏文件行、长按多选、底部操作单、传输卡片和
  独立设置子页。桌面侧栏、多标签和多段工具栏不进入手机组件树。桌面保留原有交互。
- Android 工程准备会同步猫箱图标，移除旧启动图标模板；打包检查编译后的像素与
  adaptive／round／monochrome 引用。跟随系统语言读取 Android 原生应用配置。
- 登录页提供记住账号与保持登录。用户名、偏好与会话只存系统凭据库，密码不持久化。
  刷新临时失败保留会话；代理恢复后重新连接／再次登录会取得新连接配置。
- 四个固定 API 根域名支持显式选择与匿名测速。自动选择只在用户测速后采用可用结果，
  不改写 CDN 签名 URL，不自动重放远端写操作，不改系统 Hosts／代理。
- 原文件与媒体下载网络故障进入有限自动重试，基础退避1/2/4/8/16/30秒，最多连续6次；
  服务端 Retry-After 可以延长等待，重新连接不会跳过该期限。暂停、取消及后台暂停优先。
  下载速度来自实际收到的字节，缓存命中与恢复前已下载字节不冒充新传输。
- 完成／失败／取消记录可单项移除或批量清理；文件、回执与幂等记录保留。跨页账号切换
  停止清理，损坏隐藏标记报错并保留。活动或手动暂停的任务不能直接移除。
- 视频选段预览通过 Rust 可撤销的本机流交给系统 WebView 解码；移动起终点会 seek，
  播放到选定终点暂停，切画质／关闭／账号重置释放会话。编码支持取决于系统。

## 本轮发现并修复的回归

完整检查捕获了清理任务后的旧 IPC 断言及版本升级后的旧 User-Agent 断言。
新增定向失败测试还复现了：CDN503无ETag被误报文件变化、重连跳过服务端等待下限、
清理翻页期间可能移除新账号历史、移除操作覆盖损坏隐藏标记。均修复后通过工作区测试。
原始失败与修复后的完整执行日志保存在本机 target/continuation-*.log。

首次 Rust 编译因构建盘耗尽失败。使用 Cargo 对本仓库生成缓存进行清理后重跑，
下列通过结果来自完整重跑；磁盘故障没有被记成测试通过。

## 实际执行的验证

- `cargo test --workspace --locked --offline -j 2`：182通过、0失败、19显式ignored。
- `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` 与格式检查通过。
- 显式提供已核验的本机 FFmpeg oracle 后，`cargo test -p nekopak-core --test media_tasks
  --locked --offline -- --ignored`：1通过，覆盖持久媒体计划、暂停、重启、恢复和解码。
- `npm run check`、`npm test`、`npm run build`：117个Vitest、6个wire测试通过，构建通过。
- `E2E_BROWSER_CHANNEL=msedge npm run e2e`：桌面57/57、外部请求0。
- `node tests/e2e/mobile.mjs`：360/390/412px、明暗主题共12/12，含登录、文件选择／搜索／
  操作单、传输速度／清理／导出、我的与网络设置。使用明确标注的浏览器IPC夹具。
- `node tests/e2e/clip-preview.mjs`：5/5，真实H.264红／蓝视频帧随选区变化，结束点停止，
  切画质及账号重置释放流；本地HTTP Range与合成IPC，不含真实账号或捆绑解码器。
- `node tests/e2e/native-startup.mjs`：4/4；`node tests/e2e/native-functional.mjs`：3/3。
  使用实际Windows WebView2、Rust、SQLite和本地HTTP，未操作真实账号。
- 配置现有SDK后，`python -m unittest discover -s tests -p 'test_android*.py'`：18/18，
  包括实际AAPT2编译图标、错误像素／资源图拒绝、签名和16KiB校验逻辑。
- `node tests/e2e/android-native.mjs`：隔离Android15/API35 x86_64模拟器14/14。
  实际安装debug APK，检查原生中文语言、手机组件树、真实速度、HTTP503自动恢复、后台暂停、
  重启续传、SAF取消／保存与SHA256、清理记录持久化且字节保留。登录provider使用内存
  合成会话；实际Rust/JNI AndroidKeyStore的独立读写／篡改测试另有明确检查。
- `test_parity.py`：4/4；`test_release_signing.py`：1/1。功能清单仍为0/24完整组已验收。

本轮摘要、原生截图和结构化结果在 `evidence/mobile-recovery-checks.json`、
`evidence/mobile-recovery-android-native.json`、`evidence/mobile-recovery-ui.json`、
`evidence/mobile-recovery-clip-preview.json` 与 `screenshots/android/phone-*.png`。
发行包的准确大小、源码哈希、签名和实际编译条件由各包的发布报告及清单记录。

## 许可与剩余边界

独立MIT实现，没有复制AGPL客户端代码，没有新增运行时或打包FFmpeg。固定域名协议来源、
提交及路径见NETWORK_RECOVERY_RESEARCH.md，Android自有图标及已有依赖见THIRD_PARTY_NOTICES.md。

真实登录／CAPTCHA／四域名服务能力、Free/Premium权限及实际网络由用户在应用内验收。
物理ARM64手机、Android上传／共享目录同步及前台持续下载服务尚未完成；切后台仍暂停。
Windows／Android预览包与发布链路检查不解除现有上游功能对齐门禁。

下一可执行事项：用户用同签名0.1.1 APK验证原手机上的图标、中文、代理先断后恢复、
账号自动恢复及下载重连；提供脱敏错误后继续修正真实服务差异，不交接密码或token。
