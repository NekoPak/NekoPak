# 发布仓库、签名与多平台顺序

用户确认的仓库是 https://github.com/NekoPak/NikoPak 。2026-11-08 的顺序为：先稳定Windows
桌面端，再适配Linux/macOS和Android。Android是后续明确目标，不因桌面预览而删掉。

## 密钥保管

`NEKOPAK_RELEASE_PRIVATE_KEY` 是Ed25519发布清单签名密钥，存GitHub Actions Secret。
公钥为 `release/release-signing.pub`，可公开。私钥备份在用户的本机受保护发布目录，
不在Git工作区，不输出到日志，不写入README。签名覆盖版本、文件名、大小与SHA-256，
`scripts/release_signing.py verify` 同时验证签名和对应产物内容。

这不是Windows Authenticode证书，不能据此声称EXE已获得微软信任或SmartScreen信誉。
Authenticode需要单独的受信任代码签名证书/服务。0.1.3接入NekoPak自己的更新协议，明确
复用这个受保护Ed25519密钥签署另一种product/schema限定的更新封套；没有使用Tauri
updater插件、它的签名格式或公钥。旧release-manifest.json本身不充当更新源。

Android已生成独立keystore，后续同一应用更新必须保留原签名身份。已配置Secrets名称：
`ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、
`ANDROID_KEY_PASSWORD`。2026-11-09 使用 `scripts/android_signing.py` 生成并配置；
受保护的本机备份含PKCS12密钥与密码记录，不在仓库、不输出秘密。不可重新生成身份代替升级。

## Actions

- `windows.yml`：日常Windows检查与预览产物，不使用签名秘密。
- `release.yml`：手动或v*/parity-*标签分别触发Windows和Android构建、测试、许可清单、签名及验证，
  上传Actions artifact。0.1.3起两端成功后由publish job验证安装器并发布完整公开预览Release。
  parity-*仍受严格功能门禁约束；当前publisher只接受0.x.y预览，不宣称功能对齐完成。
- `portable-core.yml`：Linux/macOS核心测试和native provider编译，不能替代相应桌面真实验收。
- `android.yml`：PR/main 日常 ARM64 debug APK，不使用发布私钥。debug 产物不可冒充正式签名。

### 实际产物约定（2026-11-09）

- `NekoPak-<version>-windows-x64-portable.zip`：解压为 `NekoPak/` 多文件目录，包含
  `nekopak.exe`、使用说明、完整许可证与依赖清单；可直接运行目录中的程序。
- `NekoPak-<version>-windows-x64-setup.exe`：真实NSIS安装器，当前用户安装，提供
  开始菜单快捷方式、卸载入口和中繁英日韩安装语言。缺WebView2时下载安装微软引导器，
  不捆绑Chromium或完整WebView2运行时。安装器内容沿用便携目录的许可证资源。
- `release-manifest.json`：对这两个产物的名称、版本、长度和SHA256签名；0.1.3起同时签入
  size-report.json，以绑定源码commit及干净构建状态；公钥单独附带。
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

## 0.1.2发布验证（2026-11-10）

[签名发布](https://github.com/NekoPak/NikoPak/actions/runs/42799710084)在
`919c4907b79cbea947469b758bdfdb2a0394eb65`通过；
[安装卸载](https://github.com/NekoPak/NikoPak/actions/runs/42802008148)通过。
最终ZIP／NSIS／APK均已下载复验并置于dist。APK沿用原证书，versionCode102，实际从101
覆盖升级及两次冷启动通过。具体文件摘要、大小与证据见SPEED_TORRENT_IME_REPORT.md及
`evidence/0.1.2-android-package.json`、`0.1.2-windows-package.json`。

## 0.1.3公开更新协议

生产客户端固定读取`https://api.github.com/repos/NekoPak/NikoPak/releases?per_page=30`，
按语义版本查找当前平台的新版本。预览版接受preview/stable，未来stable仅接受stable；
不存在更高版本与网络／签名失败是不同状态。不会发送PikPak账号头或接受IPC任意URL。

公开`update-manifest.json`形状为`{payload_base64,signature}`。签名覆盖解码后的**原始字节**，
payload含schema_version=1、product=NekoPak、version、channel、published_at、notes及
platforms。平台为windows-x86_64／android-aarch64，每项严格限定name、bytes、sha256。
签名之外仍核对tag／文件名／通道／平台／版本；下载和安装前分别核对长度与SHA256。
TLS证书检查保留，固定GitHub/Release CDN HTTPS白名单和有界响应／跳转；使用应用代理设置。

`scripts/publish_release.py`必须验证两份平台签名，要求被发布文件完全属于签名集合、
两端报告git_worktree_dirty=false且commit等于当前HEAD，发布源码也必须干净。两端报告
均签入对应平台清单；已有版本不覆盖，已有tag若指向其他commit则拒绝。创建带齐所有包、
签名和报告的draft，最后才发布prerelease，避免客户端读到半套资产。
公开平台清单分别命名windows-release-manifest.json／android-release-manifest.json，
其内部文件名保持不变，可将对应文件下载到同目录后用原verify命令验签。

安装始终用户确认。Windows验证文件及父目录在启动期间持锁，暂停本地传输后启动正常NSIS
向导并退出应用。Android同时校验包名、versionName、递增versionCode及当前签名证书集合，
只读FileProvider范围为files/updates；缺权限时仅打开本应用的系统授权页。当前不支持APK
签名轮换，不包含提权／静默安装／自动点击。便携版经向导安装新副本，原目录保留。
0.1.2以前没有更新器，需手动安装0.1.3一次。实际验证和发布证据见UPDATES_PROGRESS_REPORT.md。
