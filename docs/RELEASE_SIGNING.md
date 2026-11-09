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

Android适配时生成独立keystore，后续同一应用更新必须保留原签名身份。建议Secrets名称：
`ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、
`ANDROID_KEY_PASSWORD`。现在尚未初始化手机项目，因此不生成无用keystore或假Android构建。

## Actions

- `windows.yml`：日常Windows检查与预览产物，不使用签名秘密。
- `release.yml`：手动或v*/parity-*标签触发Windows构建、测试、许可清单、签名及验证，
  上传Actions artifact。不会自动创建公开Release。parity-*仍受严格功能门禁约束。
- `portable-core.yml`：Linux/macOS核心测试和native provider编译，不能替代相应桌面真实验收。

### 实际产物约定（2026-11-09）

- `NekoPak-<version>-windows-x64-portable.zip`：解压为 `NekoPak/` 多文件目录，包含
  `nekopak.exe`、使用说明、完整许可证与依赖清单；可直接运行目录中的程序。
- `NekoPak-<version>-windows-x64-setup.exe`：真实NSIS安装器，当前用户安装，提供
  开始菜单快捷方式、卸载入口和中繁英日韩安装语言。缺WebView2时下载安装微软引导器，
  不捆绑Chromium或完整WebView2运行时。安装器内容沿用便携目录的许可证资源。
- `release-manifest.json`：对这两个产物的名称、版本、长度和SHA256签名；公钥单独附带。
- Android APK在桌面稳定、Android宿主/凭据库/存储/后台任务实际适配后接入工作流，
  使用独立keystore，不能把Windows EXE改扩展名或把无手机代码的流程说成APK已可用。

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
