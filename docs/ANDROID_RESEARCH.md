# Android 宿主与发布约束核查

核查日期：2026-11-09。本文是实现依据，不是 APK、真实账号或 Android 功能验收报告。
已读取本仓库 README 与来源许可证约束；未复制 Piko 或其他 AGPL 实现。

## 版本与构建

- 本项目使用 NPM `@tauri-apps/cli 2.12.4`、Rust `tauri 2.12.5`。CLI 的
  `tauri-cli-v2.12.4` 标签与 Rust crate 的 `.cargo_vcs_info.json` 均指向
  `993038c49185307dfc895ab7657d77ca0d5ed7c7`。
- 该提交的 **Android app 模板**是 `compileSdk = 37`、`targetSdk = 37`、
  Android Gradle Plugin `9.3.1`、Kotlin `2.2.10`、Gradle `9.6.1`。
  **Tauri Android library** 自身的配置是 `compileSdk = 36`、`minSdk = 21`。
  因此不能根据 library 的 SDK 版本推断 `android init` 生成的 app 版本。
  实际集成的 `tauri-plugin-dialog 2.8.4` 与 `tauri-plugin-opener 2.7.1` 的 Android
  library 要求 **minSdk 24**；本项目运行最低版本因此为 Android 7.0/API 24，
  高于下文 Keystore 自身 API 23 的最低要求。2026-11-09 实际 manifest merge 验证了该约束。
  若本机构建采用已安装 SDK 36 和另一组 AGP/Gradle，应明确记录该项目配置与实际编译结果；
  不能将它写成 CLI 上游模板的默认值。
- 官方前置项：JDK、Android SDK Platform、Platform Tools、Build Tools、Command-line Tools、
  NDK；命令进程需要 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`。无需永久改用户全局环境。
  AGP 8.x 至少需要 JDK 17；JDK/Gradle 的实际组合还要满足对应 Gradle 兼容表。
- 本次执行本地 CLI `android init --help` 与 `android build --help`，确认支持
  `android init --ci --skip-targets-install`、`android build --apk --target aarch64 --ci`，
  以及 `--debug`、`--aab`、`--split-per-abi`。`tauri.android.conf.json` 会自动合并。
- Android 宿主必须编译 Rust library，典型结构为 `lib.rs` 中
  `#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run()`，桌面 `main.rs` 调用它；
  Cargo library 包含 `cdylib`。托盘、窗口管理和桌面 picker 方法须按平台编译。

来源：
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)、
[Android app template](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri-cli/templates/mobile/android/app/build.gradle.kts)、
[root Gradle template](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri-cli/templates/mobile/android/build.gradle.kts)、
[Gradle wrapper](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri-cli/templates/mobile/android/gradle/wrapper/gradle-wrapper.properties)、
[Tauri library Gradle](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/mobile/android/build.gradle.kts)、
[Android JDK requirements](https://developer.android.google.cn/build/jdks?hl=en)。

## 系统凭据库

- Android Keystore 保存不可导出的密钥；应用把会话数据交给系统密钥加密，密文可放在应用私有目录。
  这不等于将 token 以明文保存到 SQLite、SharedPreferences 或前端 localStorage。
- `KeyGenParameterSpec` 从 API 23 提供，因此采用 Android Keystore AES/GCM 的 app 应至少
  `minSdk = 23`。AES/GCM 每次保存使用新随机 IV，解密必须验证认证标签。
- `Context.getNoBackupFilesDir()` 从 API 21 提供；其中的文件不参加自动远程备份，适合存放
  与本机 Keystore 密钥绑定的密文。建议同时明确 Android 备份策略，避免换机只恢复密文而
  没有密钥后被错误处理为“未登录”。
- 必须区分首次没有记录、密文损坏、密钥缺失／失效、写入失败；损坏时不删数据、不生成新
  密钥覆盖已有会话。错误只返回稳定错误码，不能输出密文、token、登录名或 Java 异常细节。
- 桥接可以采用应用自有 Kotlin 类 + JNI，也可使用 Tauri 的 mobile plugin。
  Tauri `PluginApi::register_android_plugin(package, class)` 与
  `PluginHandle::run_mobile_plugin<T>(command, payload)` 支持 Rust 内部同步调用；后者等待
  原生响应，不能从 Android UI 线程阻塞调用。Rust provider 的同步 `SessionVault` 可以由
  宿主注入适配器，不需要把凭据 API 暴露给 WebView。
- 当前 Tao `0.37.2` 的 `ndk_glue.rs` 用 `getApplicationContext()` 创建全局 JNI 引用并初始化
  `ndk_context`；这为直接 JNI 提供应用 Context。工作线程仍需通过 `JavaVM` attach，
  使用应用 ClassLoader 获取自有 Kotlin 类，而不是假定工作线程 `FindClass` 能找到它。

来源：
[Android Keystore](https://developer.android.google.cn/training/articles/keystore?hl=en)、
[KeyGenParameterSpec API](https://developer.android.google.cn/reference/android/security/keystore/KeyGenParameterSpec?hl=en)、
[Context / noBackupFilesDir](https://developer.android.google.cn/reference/android/content/Context?hl=en)、
[Tauri mobile plugin guide](https://v2.tauri.app/develop/plugins/develop-mobile/)、
[Tauri mobile bridge implementation](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/src/plugin/mobile.rs)、
[Tao Android JNI source](https://docs.rs/crate/tao/0.37.2/source/src/platform_impl/android/ndk_glue.rs)。

## 文件路径、下载与导出

- Tauri Android `app_local_data_dir` 使用应用私有 data 目录；`download_dir()` 调用
  `getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)`，是**应用专属外部目录**，
  不是系统共享 Downloads。应用专属文件在卸载时删除，必须向用户说明，并提供明确导出。
- `tauri-plugin-dialog 2.8.4` 的 `set_parent`、`pick_folder`、`pick_folders` 都带
  `#[cfg(desktop)]`。手机不能直接编译使用这些方法的桌面处理器。
- `save_file` 支持 mobile。Android 实现使用 `ACTION_CREATE_DOCUMENT` 和 `CATEGORY_OPENABLE`，
  `saveFileDialogResult` 返回用户选择的 `content://` URI。不要把 URI 传给
  `FilePath::into_path` 或 Rust `std::fs` 作为普通磁盘路径。
- 可行的首版闭环：Rust 在应用专属目录下载、校验并持久化；完成后用户通过 `save_file`
  选择导出位置；Kotlin `ContentResolver.openOutputStream(uri, ...)` 或
  `openFileDescriptor(uri, ...)` 复制已验证的成品。仅访问 picker 授予的 URI，不申请
  `MANAGE_EXTERNAL_STORAGE`。写出失败时保留内部成品，不能显示导出完成。
- `ACTION_CREATE_DOCUMENT` 不覆盖同名文件；系统为同名结果调整名称。SAF 授权与普通路径的
  生命周期不同，未来目录上传／双向同步需要独立的 URI 授权和恢复设计；不能把可选本地
  根路径字段复用为任意 `content://` 字符串。

来源：
[Tauri PathPlugin](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/mobile/android/src/main/java/app/tauri/PathPlugin.kt)、
[dialog 2.8.4 Android source](https://github.com/tauri-apps/plugins-workspace/blob/128190689dbe59b59d344e01bf07b00862aa3791/plugins/dialog/android/src/main/java/DialogPlugin.kt)、
[dialog Rust platform gates](https://github.com/tauri-apps/plugins-workspace/blob/128190689dbe59b59d344e01bf07b00862aa3791/plugins/dialog/src/lib.rs)、
[Android app-specific storage](https://developer.android.google.cn/training/data-storage/app-specific?hl=en)、
[Storage Access Framework](https://developer.android.google.cn/training/data-storage/shared/documents-files?hl=en)。

## 生命周期和返回操作

- Rust Tauri `WindowEvent::Suspended` 对应 Android Activity `onPause`；
  `WindowEvent::Resumed` 对应 `onResume`，第一次创建 Activity 的 resume 被忽略。
  两个 variant 均为 `#[cfg(mobile)]`。`RunEvent` 有 `Resumed`，没有对应的 `Suspended`；
  按手机生命周期处理时应使用 WindowEvent 成对事件。
- `Focused(false)` 也可由普通焦点变化产生，不等同后台生命周期。
- 若首版没有 Android foreground service，建议进入暂停态时同步关闭任务调度门，异步暂停
  活跃本地传输并持久化，再由用户回到前台继续。必须防止“已暂停”期间调度器重启任务。
  打开验证浏览器或系统文件 picker 也会暂停 Activity；暂停不能清掉已有登录会话或副作用意图。
- `onPause`／`onStop` 回调不是进程结束保证；随时可能被系统终止，因此可靠恢复仍依赖
  已有 SQLite journal 和字节校验，不能等 `onDestroy` 才保存。
- Tauri 生成的 `TauriActivity` 设置 `handleBackNavigation = false`。Wry 的 true 路径只
  操作 WebView `canGoBack/goBack`；如果前端路由不是浏览器 history，必须明确接入返回键／手势，
  先关闭弹窗／回到上级，再决定是否离开应用。不能仅打开该布尔值就称返回导航已验收。
- 后续真正后台传输需遵守 Android foreground service 类型、通知、启动条件和超时限制。
  Android 的 `dataSync` 与 `mediaProcessing` 服务有每 24 小时累计 6 小时的后台限制及
  `Service.onTimeout` 收尾要求，不能把普通 Tokio 循环宣传为永久后台下载。

来源：
[Tauri WindowEvent implementation](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/src/app.rs)、
[TauriActivity template](https://github.com/tauri-apps/tauri/blob/993038c49185307dfc895ab7657d77ca0d5ed7c7/crates/tauri/mobile/android-codegen/TauriActivity.kt)、
[WryActivity 0.57.1 source](https://docs.rs/crate/wry/0.57.1/source/src/android/kotlin/WryActivity.kt)、
[Android foreground-service timeout](https://developer.android.google.cn/develop/background-work/services/fgs/timeout?hl=en)。

## APK 发布与验证

- Tauri 官方签名流程在 Android `app/build.gradle.kts` 的 `signingConfigs` 配置 keystore，
  `buildTypes.release.signingConfig` 指向它。`keystore.properties` 含密码，不能提交；
  CI 只从 Secrets 生成临时文件。密钥身份必须长期保留，同一应用更新不能随构建换密钥。
  APK 签名与仓库已有 Ed25519 发布清单签名是不同机制；应分别验证。
- 不含发布密钥的 PR／日常检查可构建明确标注的 debug APK；正式 APK 构建缺密钥必须失败，
  不能静默用 debug key 冒充 release。
- NDK `r28+` 默认生成 16 KiB 对齐 ELF。已有 `27.2.12479018` 属于 r27，需为 Rust native
  library 添加 `-Wl,-z,max-page-size=16384`、`-Wl,-z,common-page-size=16384`，并检查
  APK 中**每一个** `.so` 的 LOAD 对齐至少 `2**14`。AGP `8.5.1+` 支持相应 ZIP 对齐。
- 可执行检查包括 `apksigner verify --verbose --print-certs`、
  `zipalign -c -P 16 -v 4 app.apk`、用 NDK `llvm-readelf -l` 检查 ELF，以及
  emulator `adb install`／启动／退出重启。命令通过只证明对应构建、签名、安装或运行项，
  不证明真实账号或全格式媒体功能。
- Android 自动化至少覆盖：冷启动无崩溃；签入字段进入 Rust；合成凭据保存／覆盖／重启／删除；
  损坏密文拒绝恢复且不改写；下载取消／后台暂停／强制结束恢复；SAF 导出字节相同；
  返回导航、键盘、安全区和触摸布局。Windows 回归仍需继续执行。

来源：
[Tauri Android signing](https://v2.tauri.app/distribute/sign/android/)、
[Android 16 KiB page-size guidance](https://developer.android.google.cn/guide/practices/page-sizes?hl=en)。

## 本次研究证据边界

已实际读取上述 Tauri 官方文档、锁定提交模板、本机 Cargo registry 中的 Tauri/dialog/Tao/Wry
源码，并执行 CLI help 核对参数。`developer.android.com` 请求超时，使用 Google 自己的
`developer.android.google.cn` 对应文档核对 Android 行为。以上不包含 APK 编译结果、
模拟器验证结果或用户真实账号测试；这些由实现交付的验收记录单独报告。
