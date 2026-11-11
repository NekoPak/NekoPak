# 交接包：原生桌面端（给下一个会话）

分支：`claude/ui-performance-compatibility-al9i0o`（仓库 `NekoPak/NekoPak`，`main` 是从 NikoPak 原样搬来的旧历史）。
最新提交：`ea1e9f7` 之后加本文件。先读 `docs/NATIVE_DESKTOP.md`（结构、运行、测试）和 `CHANGELOG.md` 的“未发布”。

## 用户要求（必须遵守）

1. 桌面端不能是 WebView2 外壳 → 已换成 Slint 原生界面（`crates/nekopak-desktop`）。
2. “尚未做的也要做，要做成比较完整的”——下面的待办全部要完成。
3. 发布签名私钥已写入 Actions secrets（`NEKOPAK_RELEASE_PRIVATE_KEY`，见 `.github/workflows/release.yml`），可以用。
4. 不要创建 PR，除非用户明确要求；只推到上面的分支。

## 第一件事：看 Windows CI

提交 `ea1e9f7` 触发了 `Windows desktop checks`（run 42962162641）和 `Portable Rust core`（含新增的 `native-desktop` 作业，run 42962162705）。
此前两次 Windows 失败只是 `cargo fmt --check`，已修。本次是原生端第一次在真实 Windows 上编译/测试/打包，
很可能暴露问题，重点怀疑：

- `cargo clippy --workspace` 在 MSVC 下对 `desktop.rs`（tray-icon 0.25 API、windows-sys 调用）的报错；
- `scripts/package_installer.py`：用 Tauri NSIS 打包器 + `mainBinaryName: "nekopak"` 打包原生 exe，从未运行过；
- `scripts/test_installer.ps1`、`installer-smoke.yml`、`tests/e2e/native-*.mjs`（已改指向 `nekopak-webview.exe`，但 CI 里 debug 版 webview 宿主是否还会被构建需确认）；
- `build.rs` 的 `winresource` 图标嵌入；
- `npm run e2e` 等旧 web 测试是否仍依赖被改名的二进制。

## 待办（按优先级）

1. **让 Windows CI 全绿**，然后在真实 Windows 上验收托盘、关闭到托盘、单实例、开机自启、安装/卸载。
2. **应用内媒体预览**：视频/图片预览与选段画面预览。建议 Windows 用 Media Foundation（`IMFPMediaPlayer` 或 MediaEngine）渲染到子窗口，
   图片用 `image` 解码成 `slint::Image`；媒体会话地址来自 `service.media_session_open`（仅回环）。当前 `media.rs` 只做外部播放器。
3. **媒体库 / 图库视图**（web 的 `Gallery.tsx`）：现在只是“只看视频”筛选（`actions::open_media_library`）。
4. **拖放**：列表内拖动移动/复制、从系统拖入上传（Slint 1.18 的拖放能力先查文档）。
5. **命令面板**（Ctrl+K，web 的 `Palette.tsx`）和 **F1 快捷键入口**（`settings_page::shortcuts` 已有内容，未绑 F1）。
6. **剪贴板链接提醒**：功能和设置项已删，需重做（窗口获得焦点时读剪贴板；隐藏 TextInput 桥在 `ui/app.slint`）。
7. **演示场景切换面板**（web 的 `DevPanel.tsx`）；侧栏“演示”标签的 `Shell.open-demo` 回调未接。
8. **搜索/最近列表的“位置”列**（`Files.show-location` 恒为 false）。
9. **性能与兼容专项**（用户原始要求，尚未做）：核心热点、启动时间与内存实测（`scripts/measure_runtime.ps1`）、
   exe 体积（现 Windows gnu 约 19 MiB / 压缩 9 MiB，可评估去掉 `accessibility` 或只留一种渲染器、MSVC 实测）、
   手机 web 界面的性能与 UI 问题。
10. **更新源迁移**（待用户决定）：`crates/nekopak-core/src/updates.rs` 和 README 仍指向 `NekoPak/NikoPak` 发行页；
    迁到 `NekoPak/NekoPak` 需先在新仓库发布带签名清单的版本。`scripts/publish_release.py` 也写死了 NikoPak。
11. 文档：`docs/RUNNING.md`、`USER_GUIDE.md`、`SCREENSHOTS.md` 仍是 WebView 时代内容，需更新并补原生截图。
12. 小瑕疵：面板行右侧数值缺右内边距；窄窗口 rail 模式侧栏出现滚动条且无设置入口；
    “已选 1 项”英文单复数；核心部分英文错误消息在中文界面未翻译。

## 已验证 / 未验证

- 已验证（Linux）：`cargo clippy -p nekopak-desktop --all-targets --features automation -- -D warnings`、
  `cargo test -p nekopak-desktop`（15 项，含译文覆盖测试）、`python3 tests/native/run.py`（两种渲染器各 14 张截图）、
  核心测试、`npm run build`、`npm test`、`node scripts/gen-native-assets.mjs --check`、`cargo fmt --all --check`。
- 未验证：真实 Windows 运行与安装；真实账号流程；离线演示后端不支持重命名/分享/搜索，这些对话框无截图级验证。

## 开发速查

```bash
# Linux 依赖：libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-0 xvfb + CJK 字体；Rust ≥ 1.98
NEKOPAK_BACKEND=mock cargo run -p nekopak-desktop          # 离线演示
cargo build -p nekopak-desktop --features automation && python3 tests/native/run.py --keep out/
cargo check -p nekopak-desktop --target x86_64-pc-windows-gnu   # 需 gcc-mingw-w64-x86-64
node scripts/gen-native-assets.mjs                          # 改了图标/配色/web 文案后重新生成
```

- 新增界面文案：中文原文直接写在 `tr("…")` / `Tr.t("…")`，四种译文加到 `crates/nekopak-desktop/locales/native.json`，否则测试失败。
- 新页面优先用通用面板（`panel.rs` + `ui/pages/panel.slint`）和表单对话框（`overlay.rs` 的 `ctx.form`）。
- 界面线程不得跨 `.await` 持有 `RefCell` 借用；后端调用统一走 `ctx.call(|service| async move { … })`。
- 生成文件（`ui/generated/*`、`src/generated/*`、`locales/{en,ja,ko,zh-TW}.json`、`ui/icons/*`）勿手改。
- 提交信息结尾按会话要求附 Co-Authored-By / Claude-Session 行。
