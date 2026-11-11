# 工作进度（随时更新，中断后从这里接着做）

分支：`claude/ui-performance-compatibility-al9i0o`（只推这个分支，不建 PR，除非用户明确要求）。
先读本文件，再读 `docs/HANDOFF_NATIVE_DESKTOP.md`（模块位置、调试办法）和 `docs/NATIVE_DESKTOP.md`。

## 用户的长期要求

1. 桌面端是原生界面（Slint），不能是 WebView2 外壳。
2. 交接文档里的待办要全部做完，做成完整可用的；手机版（`src/`，Android WebView）也要完善。
3. 界面要好用、有点现代感、适合长期使用；Windows 界面已基本成型，**没有把握不要大改**。
4. **进度和要求要时时写入本文件**：每完成一步、每收到新要求就更新并提交推送，保证随时可以新开会话继续。
5. 提交信息结尾附 Co-Authored-By / Claude-Session 行；不在提交里写模型标识。
6. 如实汇报：没验证的要写明没验证。

## 当前状态（2026-11-11）

已完成并推送（详见 `docs/HANDOFF_NATIVE_DESKTOP.md` 的表格）：
Windows CI 全绿；图片查看器、媒体库、视频画面预览与选段标记；拖放与系统拖入上传；
命令面板、F1、位置列、演示面板、剪贴板链接提醒；小瑕疵四项；手机界面触控与视觉打磨；
文档与原生截图；Linux 内存 / 空闲 CPU 实测。

## 进行中

- [ ] 回查 Windows CI run 43057240688（提交 6f05a55）：是否全绿；把步骤
      “Launch the release build and measure start-up and memory” 的出窗时间、内存和
      `size-report.json` 的 exe / 压缩包体积补进 `docs/NATIVE_DESKTOP.md`“性能实测”。
- [ ] 用户反馈：“下载文件这里显示这么大，不可以选择小一点的吗”——**含义待确认**
      （可能指桌面侧栏“下载”入口 / 传输页，或手机传输页的大号速度与卡片）。确认后调整，
      倾向于提供“紧凑”选项而不是只改默认值。

## 等用户决定 / 需要用户做

- 更新源迁移：更新检查与 README 仍指向 `NekoPak/NikoPak`；迁到 `NekoPak/NekoPak` 要先在新仓库
  发布带签名清单的版本。
- Windows 真机人工验收：托盘、关闭到托盘、单实例、开机自启、安装 / 卸载、系统拖入、视频预览观感。
- 真实账号流程、Android 真机上的手机界面，本轮都没有验证。

## 可以继续做的改进（未开始）

- exe 静态导入 `cldapi.dll` → 要求 Windows 10 1709+；改成运行时加载可放宽。
- 手机传输卡片底部仍有一点多余留白（`src/styles/phone.css` `.phone-task-actions`）。
- 网页主包 663 kB（gzip 214 kB），可按页面再拆分以加快手机首屏。

## 更新记录

- 11-11：建立本文件。
