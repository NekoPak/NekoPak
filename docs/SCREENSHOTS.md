# 界面截图

对应 **0.1.3 预览版**，整理于 2026-11-10。[返回首页](../README.md) · [使用指南](USER_GUIDE.md)

截图来自运行中的应用界面，没有重绘控件或拼接虚构功能。桌面截图使用内置离线演示；手机图片分别注明浏览器样例、原生调试版或已签名发行版。样例文件、速度与账号不能作为真实 PikPak 服务验收证据。

## 原生桌面客户端

以下截图由原生客户端在离线演示数据下自行输出（`automation` 构建的脚本化截图，Linux、
1280×760）。Windows 上的字体和窗口边框会不同。

![文件浏览，浅色](screenshots/native/files-light.png)

![文件浏览，深色](screenshots/native/files-dark.png)

![命令面板（Ctrl+K）](screenshots/native/palette.png)

![媒体库缩略图墙；图片为演示数据在运行时生成](screenshots/native/gallery.png)

![图片查看器](screenshots/native/viewer.png)

![传输页](screenshots/native/transfers.png)

![设置](screenshots/native/settings.png)

下面“桌面”一节的旧截图来自手机 / 网页界面开发时的 WebView 桌面布局，保留作对照。

## 桌面文件浏览

侧栏、路径导航、文件排序和操作工具栏。浅色与深色使用相同的演示文件夹。

![桌面文件浏览，浅色主题，离线演示](screenshots/readme/desktop-files-light.png)

![桌面文件浏览，深色主题，离线演示](screenshots/readme/desktop-files-dark.png)

## 桌面任务与视频片段

传输页集中显示需要处理、进行中和已完成的任务。下面的错误和进度均是内置演示场景，便于展示不同状态。

![桌面传输任务：暂停、继续、速度、进度和错误状态；离线演示](screenshots/readme/desktop-transfers.png)

视频指定片段面板可设置起止点并显示关键帧提示。图中是内置示意预览，不是已解码的真实视频；原生支持范围见[媒体说明](../crates/nekopak-media/README.md)。

![视频起止时间选择与画质面板；演示预览](screenshots/readme/desktop-video-segment.png)

## 手机文件、传输与种子

左图为浏览器 IPC 样例的触屏文件页；中图为 Android 原生调试包读取本地 HTTP 测试服务时的真实进度；右图为浏览器种子导入样例。中图的实际网络与磁盘链路通过了自动化检查，但来源不是用户云盘。

<p>
  <img src="screenshots/android/phone-files-fixture-390.png" alt="手机文件列表，浏览器测试样例" width="30%">
  <img src="screenshots/android/progress-native-0.1.3.png" alt="原生 Android 下载任务，当前块进度为 50.7%，本地服务测试" width="30%">
  <img src="screenshots/android/torrent-fixture-390.png" alt="手机种子文件预览与提交，浏览器测试样例" width="30%">
</p>

## Android 登录与更新

前两张来自已签名 **0.1.3 ARM64 发行包**，在 Android 15 / API 35 隔离模拟器上通过 ARM 翻译桥运行：登录入口，以及无需账号的公开更新检查。第三张是原生调试版与同签名测试候选之间的系统更新确认；候选仅用于测试，没有公开发布。

<p>
  <img src="screenshots/android/release-login-0.1.3.png" alt="已签名 Android 0.1.3 发行版登录页" width="30%">
  <img src="screenshots/android/release-public-update-0.1.3.png" alt="已签名 Android 0.1.3 发行版检查公开更新源" width="30%">
  <img src="screenshots/android/update-confirmation-0.1.3.png" alt="Android 系统显示更新 NekoPak 的最终确认，独立测试候选" width="30%">
</p>

这些是模拟器实拍，不替代物理手机、第三方输入法、真实账号或任意系统安装器的验收。

## 复现与来源

四张桌面文档截图可从仓库根目录重新生成：

```powershell
npm ci
npm run build
node scripts/capture-doc-screenshots.mjs
```

Windows 默认使用已安装的 Microsoft Edge；其他平台可使用 Playwright Chromium。脚本只允许访问本机预览，检查页面错误与横向溢出，将 PNG 和版本、源码提交、尺寸、SHA256 记录到 [capture.json](screenshots/readme/capture.json)。

手机截图复用已经验收的原始 PNG，没有二次处理：

- 文件与种子样例：[mobile.mjs](../tests/e2e/mobile.mjs)、[手机布局记录](evidence/updates-mobile-ui.json)。
- 原生进度：[android-native.mjs](../tests/e2e/android-native.mjs)、[17 项原生检查](evidence/updates-progress-android-native.json)。
- 系统安装确认：[update-native.mjs](../tests/e2e/update-native.mjs)、[原生更新检查](evidence/updates-native-android.json)。
- 发行版登录与公开更新：[android-release.mjs](../tests/e2e/android-release.mjs)、[最终 APK 运行记录](evidence/0.1.3-android-release-runtime.json)。

完整版本验收与文件哈希见 [UPDATES_PROGRESS_REPORT.md](UPDATES_PROGRESS_REPORT.md)。
