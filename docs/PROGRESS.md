# 工作进度（随时更新，中断后从这里接着做）

分支：当前会话推到 `ccr-888a9d4f-punvqw`（由 `claude/ui-performance-compatibility-al9i0o` 的最新提交 a6fc7fa 接着做；
每个新会话以系统指定的分支为准，开始前先把它对齐到上一个分支的最新提交）。不建 PR，除非用户明确要求。
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

- [x] Windows CI run 43057240688（提交 6f05a55）全绿：发行版 exe 在 Windows 上启动成功，
      Media Foundation 解码测试为强制项（`NEKOPAK_REQUIRE_MEDIA_FOUNDATION=1`）并通过。
      体积已写入 `NATIVE_DESKTOP.md`。出窗时间 / 内存的具体数字在该 run 的步骤日志
      “Launch the release build…” 里，**还没抄进文档**（日志很长，下次只取那一步）。
- [ ] 用户曾提到“下载文件这里显示这么大，不可以选择小一点的吗”，后来重发消息时去掉了这句，
      含义未确认；如再提起先问清指哪里（手机传输页 / 桌面侧栏“下载” / 桌面传输列表 / 安装包体积）。
- [x] 网页语言包按需加载（`src/lib/i18n.ts` 的 `ensureLocale`）：主包 663 → 366 kB
      （gzip 214 → 113 kB），简体中文用户不再下载任何译文。已在浏览器里验证英文 / 日文首屏
      和运行时切到韩语；单元测试通过 `tests/unit/setup-locales.ts` 预先注册全部语言。
      已确认：提交 3d97d96 的 Windows CI 全绿（含 `npm test`、`npm run e2e`），Linux 两个作业通过。
      同一轮的 `core (macos-latest)` 被取消——排队 15 分钟没分到运行器、一步都没执行，
      不是代码失败；之前各轮 macOS 都通过。下次推送时留意它是否恢复。
- [x] 原生桌面性能（11-12）：新增 `src/models.rs`（`sync_rows` / `update`，只通知变化的行，
      大部分不同时才整表重置）；文件列表 `push_rows`、网格选中（`patch_row` 按格更新）、
      传输页 350 ms 刷新都改为原地更新；“位置”列每批只扫相关行；Windows 名称排序改用
      `LCMapStringEx` 预生成排序键。Linux：clippy、单元测试、两种渲染器冒烟测试通过，
      网格视图单选 / 全选 / 取消截图核对正确。Windows 目标交叉 clippy 通过；`collate.rs` 新增仅 Windows 运行的测试，
      在 CI 上核对排序键顺序与 `CompareStringEx` 一致；
      **没有在 Windows 真机上对比排序结果**，也没有大目录的耗时对比数字。
- [ ] 下一步候选：`cldapi.dll` 改运行时加载（放宽到更早的 Windows 10）；把 CI 启动测量的
      数字抄进 `NATIVE_DESKTOP.md`；Android 真机上看手机界面。

## 等用户决定 / 需要用户做

- 更新源迁移：更新检查与 README 仍指向 `NekoPak/NikoPak`；迁到 `NekoPak/NekoPak` 要先在新仓库
  发布带签名清单的版本。
- Windows 真机人工验收：托盘、关闭到托盘、单实例、开机自启、安装 / 卸载、系统拖入、视频预览观感。
- 真实账号流程、Android 真机上的手机界面，本轮都没有验证。

## 可以继续做的改进（未开始）

- exe 静态导入 `cldapi.dll` → 要求 Windows 10 1709+；改成运行时加载可放宽。
- 网页主包现为 366 kB；图标表（约 100 kB 源码）仍在主包内。11-12 评估：`filled` 变体是运行时决定的
  （按钮按下态、导航激活态），无法静态裁剪；首屏就要用，拆分收益小，暂不做。
- 媒体库 `gallery.rs` 的行结构含嵌套模型，仍整表替换（缩略图到达已是按格更新）。

## 更新记录

- 11-11：建立本文件。
- 11-11：CI 全绿并记录体积；进度文件改为每步更新。
- 11-11：语言包按需加载，手机首屏包减小约 45%。
- 11-12：确认语言包改动的 CI；macOS 作业因没有运行器被取消（非代码问题）。
- 11-12：原生列表 / 网格 / 传输页改为增量更新模型；Windows 名称排序预生成系统排序键。
