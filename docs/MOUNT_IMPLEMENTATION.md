# NekoPak Mount Implementation

状态：Windows x64 可选挂载层，2026-11-08。此 crate 不包含驱动、安装程序或外部命令行工具。挂载成功只在 Windows API/已安装 WinFsp API 实际返回成功后报告。

## 共用读取模型

`MountSnapshot` 是一次完整、不可变的远端目录快照。路径使用 `/`，不允许绝对路径、`..`、ADS、保留设备名、尾随空格/句点、符号链接穿越或大小写冲突。每个文件有 `remote_id`、账户/版本绑定的 `identity` 和大小。`ReadSource` 是 Core 提供的同步读取适配器：每次读取必须重新验证快照条目的 identity/size，不能把签名 URL 或令牌传给挂载层。

启动前 `ReadSession::verified` 实际读取最大文件的尾部、开头和中点（每段最多 4 KiB），短读、越界、空树或取消均失败。Core 应在 `spawn_blocking` 中执行启动，并在适配器内用专用 runtime/`Handle::block_on`；不要在 Tokio worker 内直接阻塞。

## Cloud Files API

`CloudFilesMount::start(root, snapshot, source)` 只接受专用目录：新目录由 NekoPak 创建并写入隐藏 marker；已存在目录仅在 marker、OS sync-root identity、目录 file-id/volume 匹配且已存在条目版本不变时重连。任意其他目录（哪怕为空）均拒绝。目录内占位文件通过 `CfCreatePlaceholders` 生成，文件标识是 SHA-256 派生的 entry identity；`FETCH_DATA` 回调调用 Core `ReadSource`，按 4 KiB 对齐发布 `TRANSFER_DATA`，取消/失败发布失败状态。只读回调拒绝删除和重命名。

停止后重新开始会读取新目录快照，允许增加新节点。旧 inventory 的 fingerprint、全部原条目版本和占位文件状态先校验，通过后只创建新增占位文件，最后原子写入新清单。修改、移动、删除和本地已修改条目均保留原文件并返回冲突，不静默重置。不会为了刷新而重新注册或注销根。当前没有自动刷新轮询；用户停止后重新开始以刷新新增条目。目录新增不会使原有目录 identity 因 mtime 变化而失效。

Cloud Files 的只读是提供程序不做远端写入、占位文件 READONLY 属性和删除/重命名回调约束，不等于独立只读卷。用户仍可在该本地目录中新增自己的文件，或改变文件属性；这些文件不会上传，已改占位文件不会被覆盖。真正由文件系统驱动强制的只读卷是 WinFsp 模式。

`stop` 取消请求并调用 `CfDisconnectSyncRoot`，保留 marker、注册和占位文件。没有隐式调用 `CfUnregisterSyncRoot`：微软文档说明注销会删除未水合占位文件。只有显式的、经过用户确认的删除/卸载流程才能另行注销。

## WinFsp

`WinFspMount::start("P:", snapshot, source)` 只读挂载。它从 HKLM WinFsp 安装目录加载固定 `winfsp-x64.dll`，校验文件不是 reparse point，检查 `FspVersion` 和所需导出后创建卷、设置盘符、启动 dispatcher。不会自动安装或提权；用户显式挂载时 WinFsp 自身可启动已安装的驱动服务。实现使用 WinFsp 2.x 公共 C ABI，挂载层仅实现 volume/security/open/read/flush/get-info/readdir/dispatcher-stopped；所有写、删除、重命名、reparse、stream 操作保持未实现，驱动返回相应失败。盘符必须为空闲的 `A:`–`Z:`。ACL 仅向当前用户和 SYSTEM 提供读/执行，文件上下文使用 `UmFileContextIsUserContext2` 描述符语义。

WinFsp 上游为 GPLv3 + FLOSS 动态链接例外。NekoPak 没有复制 `winfsp-rs`（GPL）实现，也不捆绑 DLL。若向用户展示或发布该可选功能，UI/文档必须包含：`WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos` 和 <https://github.com/winfsp/winfsp>。完整许可证见上游 `License.txt`。

## 能力与生命周期

`native_capabilities()` 只做平台/运行时发现，不代表远端 Range 已可读。`CloudFilesMount` 和 `WinFspMount` 均为 `Send`（回调 context 由实例拥有且 stop/drop 先断开回调），不是 `Sync`；因此可以放入 Core 的 `Mutex<HashMap<MountId, Mounted>>` 并在 `spawn_blocking` 中 start/stop。stop 必须先取消读取再断开；任何断开失败都不能释放回调 context。

Core 启动前逐文件取得实际 `RangeSource.identity`，每次回调与之比较；没有 hash/mtime 的同大小替换仍会拒绝。元数据、版本校验、读取和最终账号校验共用 40 秒总时限；取消每 25ms 检查。`Source` 仅持 `Weak<AppService>`，临时升级的服务引用在阻塞线程池释放，防止最后一个引用在 native callback 线程析构并等待自己。

Core 的 async mount gate 序列化开始/停止/配置变更；挂载 epoch 在最终插入所用 mutex 内更新，使账号切换前已经启动的操作失效。账号变化期间持有 gate，直到登录/退出/切换完成。重复目标和重复配置不会覆盖已存在实例，失败实例在阻塞线程池停止，状态读取原生连接状态。

当前 DLL 未安装时能力报告 unavailable；DLL 接口可加载仍不等于驱动可工作，最终由 `mount_start` 验证。本机已执行的 WinFsp ABI 编译和 callback 单元测试不等于真实盘符验收。

## 许可证/来源

Cloud Files API 是 Windows SDK API，按微软平台条款使用。本 crate 的 Cloud Files/WinFsp 胶水代码是 NekoPak 独立实现，未复制 AGPL Piko、GPL `winfsp-rs` 或任何第三方实现。WinFsp ABI 对照的公开版本 SHA 为 `4425d4807aea4626affb151e38c1cc774ccd5e75`，参考 `inc/winfsp/winfsp.h`、`inc/winfsp/fsctl.h` 的接口定义，以及 `src/dll/fsctl.c`、`src/dll/sxs.c` 的服务启动/SxS 行为；未移植实现。

核验来源：

- [CfRegisterSyncRoot](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot)
- [CfDisconnectSyncRoot](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfdisconnectsyncroot)：返回后不会再收到回调。
- [CfUnregisterSyncRoot](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfunregistersyncroot)：未水合文件可能删除，因此 stop 不调用它。
- [CF_OPERATION_PARAMETERS](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/ns-cfapi-cf_operation_parameters)：TRANSFER_DATA 的 4 KiB/EOF 对齐。
- [WinFsp API header](https://github.com/winfsp/winfsp/blob/4425d4807aea4626affb151e38c1cc774ccd5e75/inc/winfsp/winfsp.h)、[ABI structures](https://github.com/winfsp/winfsp/blob/4425d4807aea4626affb151e38c1cc774ccd5e75/inc/winfsp/fsctl.h)、[许可证与 FLOSS 例外](https://github.com/winfsp/winfsp/blob/4425d4807aea4626affb151e38c1cc774ccd5e75/License.txt)。

## 已执行验证（2026-11-08）

平台 Windows x64；`CfGetPlatformInfo` 返回 `26100.9278.1576`。WinFsp：未安装所需 DLL；未安装驱动、未测试用户盘符。

- `cargo check -p nekopak-mount`：通过。
- `cargo test -p nekopak-mount --locked`：12 个通过；2 个显式 native smoke/helper 默认忽略。
- `cargo clippy -p nekopak-mount --all-targets --locked -- -D warnings`：通过。
- `cargo test -p nekopak-mount --locked native_register_populate_hydrate_reconnect -- --ignored --nocapture`：真实 Windows 测试通过。新建隔离 temp 根→注册→连接→placeholder→显式水合→独立进程尾/中/头读取且逐字节校验→断开/重连→新增嵌套 placeholder→修改版本拒绝且旧内容保留。最后仅注销并清理测试自己创建的 canonical temp 根。
- `cargo test -p nekopak-core --locked mounts::tests --lib`：5 个通过；覆盖账户/版本、配置、并发提交、总取消、Weak 生命周期和 native dispatcher 状态映射。

尚未验收：真实账户大目录与断网后的 CfAPI 错误展示、Explorer 外壳 SyncRootManager 注册/状态图标、手动注销/安全移除 UI、修改/删除条目的非破坏更新策略、脱水和 pin 控件、真实 WinFsp 驱动/播放器随机读取、Linux FUSE。当前实现和上述 smoke 不代表完整挂载/上游功能对齐完成。
