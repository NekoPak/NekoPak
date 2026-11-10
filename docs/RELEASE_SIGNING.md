# 发布仓库、签名与多平台顺序

用户确认的仓库是 https://github.com/NekoPak/NikoPak 。2026-11-08 的顺序为：先稳定Windows
桌面端，再适配Linux/macOS和Android。Android是后续明确目标，不因桌面预览而删掉。

## 密钥保管

`NEKOPAK_RELEASE_PRIVATE_KEY` 是Ed25519发布清单签名密钥，存GitHub Actions Secret。
公钥为 `release/release-signing.pub`，可公开。私钥备份在用户的本机受保护发布目录，
不在Git工作区，不输出到日志，不写入README。签名覆盖版本、文件名、大小与SHA-256，
`scripts/release_signing.py verify` 同时验证签名和对应产物内容。

这不是Windows Authenticode证书，不能据此声称EXE已获得微软信任或SmartScreen信誉。
Authenticode需要单独的受信任代码签名证书/服务。Tauri updater自己的签名、公钥和更新
manifest也属于独立协议，在实际接入更新器时单独生成并配置，不能拿本清单冒充可自动更新。

Android已生成独立keystore，后续同一应用更新必须保留原签名身份。已配置Secrets名称：
`ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、
`ANDROID_KEY_PASSWORD`。2026-11-09 使用 `scripts/android_signing.py` 生成并配置；
受保护的本机备份含PKCS12密钥与密码记录，不在仓库、不输出秘密。不可重新生成身份代替升级。

## Actions

- `windows.yml`：日常Windows检查与预览产物，不使用签名秘密。
- `release.yml`：手动或v*/parity-*标签分别触发Windows和Android构建、测试、许可清单、签名及验证，
  上传Actions artifact。不会自动创建公开Release。parity-*仍受严格功能门禁约束。
- `portable-core.yml`：Linux/macOS核心测试和native provider编译，不能替代相应桌面真实验收。
- `android.yml`：PR/main 日常 ARM64 debug APK，不使用发布私钥。debug 产物不可冒充正式签名。

### 实际产物约定（2026-11-09）

- `NekoPak-<version>-windows-x64-portable.zip`：解压为 `NekoPak/` 多文件目录，包含
  `nekopak.exe`、使用说明、完整许可证与依赖清单；可直接运行目录中的程序。
- `NekoPak-<version>-windows-x64-setup.exe`：真实NSIS安装器，当前用户安装，提供
  开始菜单快捷方式、卸载入口和中繁英日韩安装语言。缺WebView2时下载安装微软引导器，
  不捆绑Chromium或完整WebView2运行时。安装器内容沿用便携目录的许可证资源。
- `release-manifest.json`：对这两个产物的名称、版本、长度和SHA256签名；公钥单独附带。
- Android APK由独立宿主和Rust library生成，使用独立keystore。设备验收状态见
  `ANDROID.md`；缺签名输入直接失败。检查包签名、SDK/ABI、ZIP和ELF16KiB对齐，
  发布清单另以Ed25519签名，不能将二者混为同一种签名。

Tauri前端资源目前嵌入原生EXE，不属于启动时自解压的打包器。发行目录包含多个文件，
不代表必须人为拆分Rust/TLS/SQLite库。两种布局的性能要量测启动时间、WebView2整体内存、
I/O和安全扫描，不能仅按文件数判断。更新前端或媒体组件时可继续按实际需要分包。

可复现命令：`npm ci` → `npm run tauri -- build --no-bundle` →
`python scripts/package_release.py` → `python scripts/package_installer.py`。
NPM Tauri API/CLI固定为2.12.4，与Rust Tauri 2.12.x使用相同minor版本，避免CLI版本门禁失败。

未来Android需要Tauri mobile宿主、Android Keystore实现、目录选择/存储权限/后台下载/
网络代理/播放器/触屏布局以及设备测试。Windows Credential Manager、CfAPI、WinFsp及
EXE外部播放器不是Android可用功能，须按平台能力分别实现。

生成签名仅运行一次，脚本拒绝覆盖已有密钥。使用GitHub CLI通过标准输入上传Secret，
不得把私钥拼进命令行或提交git。Workflow校验Secret缺失时失败，不降级成“已签名”。

## 本机与仓库已配置

2026-11-08 已生成发布签名，公钥入库，`NEKOPAK_RELEASE_PRIVATE_KEY` 已通过标准输入
写入NekoPak/NikoPak Actions Secret。本机备份位于用户主目录 `.nekopak-release/`，
Windows ACL仅当前用户和SYSTEM可读；保留备份，不要删掉或公开。使用已有ZIP实际签名和验签
通过，测试证明修改包字节或签名覆盖的版本会拒绝。没有创建Windows Authenticode证书。

## 自动构建实测（2026-11-09）

[签名发布工作流](https://github.com/NekoPak/NikoPak/actions/runs/42569659660) 已成功生成便携ZIP和NSIS安装器，
对两者签名并上传artifact；下载后再次用仓库公钥验签、核对ZIP与内嵌EXE哈希通过。
[独立安装/卸载检查](https://github.com/NekoPak/NikoPak/actions/runs/42571623607) 在干净Windows runner
验证实际落盘文件与许可证、EXE哈希、开始菜单快捷方式和卸载清理，全部通过；没有在用户本机安装。
`installer-smoke.yml` 可在签名发布成功后自动执行，也可指定历史run手动验证，使用独立临时目录。

## Android签名与设备检查（2026-11-09）

最终源提交 `df2141c` 的 [Android检查](https://github.com/NekoPak/NikoPak/actions/runs/42617477353) 和
[签名发布Android job](https://github.com/NekoPak/NikoPak/actions/runs/42617478011) 已通过。
从Actions下载实际APK后，使用仓库公钥验清单、Android证书、全部哈希／16KiB对齐／许可证。
该远端APK以相同签名身份安装到隔离Android15模拟器，并通过冷启动与登录入口显示检查；
没有输入真实凭据。完整证据 `evidence/android-actions-package.json`。
Android证书SHA256为 `3c7f59b7dd1ae693aa5dd5309966a2eee83bc9055a650bb32d9c035780b588ba`，
用于核对升级身份，公示证书摘要不包含私钥。公开GitHub Release仍是独立发布步骤。

最终 `df2141c` 的 [Windows+Android签名发布](https://github.com/NekoPak/NikoPak/actions/runs/42617478011)
两个job均成功，ZIP、安装器与APK已上传；随后[最终安装卸载测试](https://github.com/NekoPak/NikoPak/actions/runs/42619426755)通过。
这些是预览构建与打包验收，没有解除功能对齐门禁或替代真实账号测试。

## 0.1.1发布验证（2026-11-10）

源码`2a99844334c276c1e0a36e0ce4060f065fb54fbe`的
[Windows＋Android签名发布](https://github.com/NekoPak/NikoPak/actions/runs/42770550171)与
[安装卸载检查](https://github.com/NekoPak/NikoPak/actions/runs/42772343047)均成功。
便携ZIP为7,152,739字节，NSIS安装器4,125,655字节，ARM64 APK17,322,049字节。
三个最终用户包均已下载到本机dist，并以仓库公钥重新验证清单及全部哈希。

Android沿用原证书，versionCode由100升为101。实际从0.1.0覆盖安装后，两次冷启动通过，
原生中文语言保持，应用信息页显示猫箱图标。APK SHA256为
`391c1f723c4d0e8c32b7f387a4a8941dc80a87439ae35619febf105cf47cdb7c`。
Maven POM许可证元数据的固定URL和哈希随包附带；原始文件与Git中一致，无新增运行时。
证据为`evidence/mobile-recovery-actions-android.json`、`mobile-recovery-actions-windows.json`
及`mobile-recovery-release-runtime.json`。Windows仍为清单签名，未宣称Authenticode。
