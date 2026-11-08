# UI 静态资源体积

生成：2026-11-08T03:21:14.701Z（node v24.18.0，vite build）

| 文件 | 原始 KiB | gzip KiB | brotli KiB | 加载时机 |
|---|---:|---:|---:|---|
| assets/index-CDemXr_F.js | 286.32 | 88.84 | 72.69 | startup |
| assets/style-BPqYzixW.css | 84.76 | 15.32 | 13.24 | startup |
| assets/backend-BYKs2Ppe.js | 52.01 | 17.84 | 15.18 | mock only (dynamic import) |
| assets/window-BMyBQEXS.js | 13.52 | 3.36 | 2.98 | tauri startup (dynamic import) |
| assets/Sync-t7Sdob69.js | 12.91 | 5.22 | 4.38 | on page/action (dynamic import) |
| assets/Media-BpBu-GCH.js | 11.16 | 4.76 | 4.08 | on page/action (dynamic import) |
| assets/BatchRename-DPELz2ic.js | 8.50 | 3.57 | 3.07 | on page/action (dynamic import) |
| assets/Palette-C8mh7Cyw.js | 7.28 | 3.31 | 2.80 | on page/action (dynamic import) |
| assets/Duplicates-BAngie5w.js | 6.93 | 3.20 | 2.74 | on page/action (dynamic import) |
| assets/Space-Bfjrt4CY.js | 5.97 | 2.80 | 2.43 | on page/action (dynamic import) |
| assets/Shares-DjKSOqO1.js | 5.29 | 2.36 | 1.96 | on page/action (dynamic import) |
| assets/Archive-Bnjz_NoV.js | 5.12 | 2.36 | 1.99 | on page/action (dynamic import) |
| assets/Mount-B9IXNefn.js | 2.81 | 1.50 | 1.20 | on page/action (dynamic import) |
| assets/DevPanel-Wz_2VBfZ.js | 2.62 | 1.51 | 1.21 | mock only (dynamic import) |
| assets/core-CKq3Z5BB.js | 2.39 | 0.95 | 0.82 | tauri startup (dynamic import) |
| assets/event-iqD-r1Mo.js | 1.30 | 0.65 | 0.54 | tauri startup (dynamic import) |
| index.html | 0.82 | 0.53 | 0.37 | startup |
| theme-init.js | 0.38 | 0.24 | 0.21 | startup |
| **合计** | **510.10** | **158.32** | **131.90** | |

桌面启动资源 gzip：109.90 KiB。全部桌面可加载资源（含按需页面，不含 Mock/DevPanel）gzip：138.98 KiB。
这是压缩静态资源口径，不是运行内存或整包体积；Mock 块仍在构建目录中，桌面不会加载。
