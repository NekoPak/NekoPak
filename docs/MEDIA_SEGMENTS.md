# 视频指定片段：当前实现与剩余验收

2026-11-07。用户要求最终不少于上游，第三方 API 可参考。主行为基线是 Piko v1.4.0，
固定 commit 和逐项证据见 UPSTREAM_PARITY_RESEARCH.md；本模块独立实现，没有复制 AGPL 代码。

## 已实现的本地实验核心

`nekopak-media::render_clip` 接收已核验的外部 FFmpeg、**本地**源、全新 MP4 输出路径、
毫秒时间范围。当前只放行有索引的 MP4/MKV 源，采用流复制、前关键帧 seek、视频+音轨映射、
fast-start MP4，并报告请求区间、实际输出时长、字节数及本地 SHA-256。
输出可能从请求点之前开始；不声称逐帧精确或所有轨道无损。字幕/数据/附件被排除，
不支持的音轨组合会失败，不暗中转码或丢弃音频。完整音轨语言/HEVC验证仍待补齐。

库强制绝对本地路径、拒绝穿越/Windows设备名/reparse点/现有目标，固定参数而非 shell；
仅允许 file 协议和特定容器，避免媒体文件引用远程/播放列表输入。子进程隐藏运行、
校验可执行文件哈希、限制诊断输出、超时杀进程。唯一临时文件位于目标目录，输出验证后
无覆盖提交，冲突时原文件不变。MP4校验只覆盖容器结构/时长，不能替代完整解码校验。
运行中取消/超时后等待子进程退出再清理本次临时文件；强杀整个应用后的清理/恢复尚未实现。

这是独立 library/example，不接入 Tauri 产品入口、持久任务或真实 Provider。
`segment_download` 当前能力始终为 false。该代码不能算远端片段下载已完成。

## 运行

FFmpeg 必须由用户配置外部可信版本与 SHA-256；Lite 不捆绑或自动下载媒体工具。
当前没有产品内依赖安装 UI；完整媒体组件体验仍是发布前必需工作。

```powershell
cargo run -p nekopak-media --example clip_local --locked -- `
  C:\Tools\ffmpeg.exe <已核验SHA256> C:\Videos\source.mkv C:\Videos\new-clip.mp4 5000 8500
```

```powershell
cargo test -p nekopak-media --locked
.\scripts\test-media.ps1 -FfmpegPath C:\Tools\ffmpeg.exe -Sha256 <已核验SHA256>
```

本机测试工具是已有 imageio_ffmpeg 附带的 FFmpeg 7.1 essentials（Gyan，GPLv3 build），
SHA-256 `2ce797a0f88d7f067180338fb227f7b1928ea727bd9a4d7a1d022f7c52af71a3`。
仅外部执行测试，没有链接其库、复制源码或随 NekoPak 分发该二进制。

## TS 失败证据与必须补齐的部分

初始真实集成测试：12秒 H.264/AAC、每2秒关键帧，请求5.0–8.5秒；MP4/MKV满足
前关键帧导出窗口。相同策略对 TS 输出仅3755ms，没有通过预期4000–5000ms的窗口。
因此当前代码拒绝 TS（含伪装后缀），TS 的测试验证拒绝行为，而不是声称 TS功能完成。
`segment_ts` 在 parity-manifest 中保持 blocked，正式对齐门禁不会通过。

后续必需：媒体探测与准确关键帧索引，TS非零时间戳/HEVC/hvc1，多音轨和语言/字幕提示，
授权远程随机读取+Range/版本验证+共享稀疏缓存，原画/转码表示选择，进度、持久计划、
暂停与进程重启恢复，来源改变/过期/配额，播放器选段入口和真实账号验证。
第三方社区协议适配器可提供上述源能力，不要求官方 CLI 单独具备全部能力。

## 许可/事实边界

参考 Piko 的用户行为与公开源码测试，未复用其实现。FFmpeg 参数遵循其公开接口，
容器检查是独立编写的最小 ISO-BMFF 读取。上游文档/源码被核查不代表运行上游成功。
本地生成样本的解码测试不代表任意真实媒体/远端链接均被支持。
当前没有新的产品发行包；前一 CLI ZIP 不包含这一本地实验核心。
