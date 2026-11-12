# 交接包：原生桌面端（给下一个会话）

本文件保留原生桌面迁移的历史交接信息。当前分支、合并与发布进度以 `docs/PROGRESS.md` 为准。
仓库：`NekoPak/NekoPak`；原始实现分支：`claude/ui-performance-compatibility-al9i0o`。
结构与测试方法见 `docs/NATIVE_DESKTOP.md`，0.2.0 发布说明见 `release/notes/0.2.0.md`。

## 用户要求（必须遵守）

1. 桌面端不能是 WebView2 外壳 → 已换成 Slint 原生界面（`crates/nekopak-desktop`）。
2. “尚未做的也要做，要做成比较完整的”——下面的待办全部要完成。
3. 发布签名私钥已写入 Actions secrets（`NEKOPAK_RELEASE_PRIVATE_KEY`，见 `.github/workflows/release.yml`），可以用。
4. 原先仅推工作分支；2026-11-12 用户已明确要求合并并发布 Release、tag 和 README。

## 现状（第二轮会话结束时）

Windows CI（`Windows desktop checks`）已全绿：clippy、测试、发行版构建、便携包、NSIS 安装器。
上一版“待办”清单的处理结果：

| 项 | 结果 |
|---|---|
| Windows CI | 通过。补了 13 个依赖的许可证文本（`licenses/upstream`、`licenses/sources.json`） |
| 应用内图片预览、媒体库 | 完成（`preview.rs`、`gallery.rs`、`ui/viewer.slint`、`ui/pages/gallery.slint`） |
| 视频画面预览、选段标记 | 完成。通用引擎 `frames.rs`，Windows 解码器 `frames_win.rs`（Media Foundation，运行时加载）。CI 上有端到端解码测试 |
| 拖放 | 完成。列表内用 Slint `DragArea` / `DropArea`；系统拖入经 winit 事件（`window_events.rs`） |
| 命令面板、F1 | 完成（`palette.rs`、`ui/palette.slint`） |
| 剪贴板链接提醒 | 完成，默认关闭（`cloud::clipboard_check`） |
| 演示场景面板 | 完成（`demo.rs`）；演示后端改用 `MockProvider::demo`，可搜索 / 整理 / 预览 |
| “位置”列 | 完成（`files::resolve_locations`） |
| 小瑕疵四项 | 完成；英文核心消息译表在 `locales/core-messages.json` |
| 手机界面 | 做了一轮触控与视觉打磨（`src/styles/phone.css` 末尾） |
| 文档 | `NATIVE_DESKTOP.md`、`CHANGELOG.md`、`SCREENSHOTS.md`、`USER_GUIDE.md`、`RUNNING.md` 已更新，原生截图在 `docs/screenshots/native` |

## 仍然没做 / 没验证

1. **Windows 真机人工验收**：托盘、关闭到托盘、单实例、开机自启、安装 / 卸载、系统文件拖入、
   鼠标侧键、视频预览的实际观感。CI 只证明能编译、测试通过、发行版能启动。
2. 更新源迁移已由 0.1.4 完成，0.2.0 保留该改动，读取 `NekoPak/NekoPak`；旧版升级说明见 README。
3. **真实账号流程**未在这两轮验证。
4. 视频预览没有声音（有意为之，完整播放交给外部播放器）；HEIC / AVIF / SVG 不能应用内预览。
5. exe 静态导入 `cldapi.dll`，因此要求 Windows 10 1709+；若要支持更早版本需把
   `nekopak-mount` 的云文件接口改成运行时加载。
6. 性能：见 `docs/NATIVE_DESKTOP.md` 的“性能实测”；CI 的启动与内存数据已记录在 `NATIVE_DESKTOP.md`，未设阈值。体积优化（去掉 `accessibility` 或
   只留一种渲染器）评估后未做——两者都影响兼容性或可访问性。

## 调试界面的办法

`tests/native/smoke.txt` 之外，可以写临时脚本驱动 `automation` 构建：
`click / dclick / rclick / down / move / up / key / ctrl / type / size / shot`。
`NEKOPAK_FAKE_FRAMES=1` 让视频预览使用合成帧（仅 `automation` 构建），便于在 Linux 上
走查进度条、播放和选段标记。演示后端里“照片”文件夹的图片可真实解码。

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
- 保留原始提交作者；新提交按实际贡献者署名，不冒用历史会话的作者信息。
