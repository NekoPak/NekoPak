# 种子文件导入依据

2026-11-10。用户要求云下载支持直接选择`.torrent`；本轮实现只解析元数据和提交既有
云下载协议，不建立BT peer连接，也不把普通文件上传完成冒充云下载完成。

## 已核查行为

此前实际获取的PikPak公开Web固定分块`6465.837e9f1a.js`包含以下调用链：筛选`.torrent`
文件、限制每个文件不超过10MiB、FileReader读ArrayBuffer、解析种子并生成magnet字符串、
调用createTask/createTasks并标记来源`upload_torrent`。因此文件选择入口不要求先把种子
作为普通网盘文件上传。本次按该行为独立实现Rust解析及原生选择器。

- 原URL：https://mypikpak.com/drive/6465.837e9f1a.js
- 固定缓存SHA256：`59996cb6a1fb83d52de28405323862d2e8b118247ba6c388126ed4c72b18f002`
- 复查路径：`target/file-protocol-audit/official-6465.837e9f1a.js`（忽略的研究缓存）。
- 当前URL重新请求未取得与固定缓存相同的字节，因此不声称它仍是当前站点的相同构建。
- Web源码许可证未作为可复用许可建立；未复制其JS实现，只记录行为与协议事实。

同时实际读取[BEP3](https://www.bittorrent.org/beps/bep_0003.html)，页面SHA256
`b24556b2a6881a407908013437bbc8cb3715885ba25fc326b9e7b1b502c24ced`。
该规范明确info_hash必须对种子文件中原始bencoded info字节取SHA1，而不是任意重编码后
计算；pieces长度为20字节的倍数；length与files两种布局互斥。本项目按此独立实现。
SHA1只用于BT协议身份，普通下载字节校验仍用SHA256。

## 实现边界

支持v1及包含v1 pieces信息的混合种子；纯v2、超过10MiB、重复字典字段、损坏长度、
过深嵌套、不安全路径、不一致pieces计数或非UTF-8文件名明确失败。文件上限10,000，
预览最多显示200条但提交整个种子；不宣称服务已支持按文件子集创建。

Android通过系统文件选择器与ContentResolver读取所选文件，content URI只在原生层。
Windows读取同样由系统选择器返回的文件。选取和本地预览不发云端创建请求；确认后才
使用既有POST /drive/v1/files UPLOAD_TYPE_URL云下载流程，并保留账号范围幂等意图。
tracker地址保留在Rust内存，预览不返回tracker授权或本地绝对路径。引用5分钟过期，
取消／关闭释放，账号改变禁止提交。没有新增第三方解析库或复用AGPL实现。

默认测试验证info原始字节、单／多文件、tracker不泄漏、损坏输入、引用过期与账号隔离、
同键只创建一次。实际Android选择器与本地HTTP端到端记录见本轮验收报告。
这不证明服务可获取任意私有tracker、死种、纯v2或真实账号资源。
