#!/usr/bin/env node
// Static asset size report for ui-dist. KiB, gzip -9 and brotli -11.
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(root, "ui-dist");
const files = [];
(function walk(d) {
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else files.push(p);
  }
})(dist);

const kb = (n) => (n / 1024).toFixed(2);
function loading(path) {
  if (/(backend|DevPanel)-.*\.js$/.test(path)) return "mock only (dynamic import)";
  if (/(core|event|window)-.*\.js$/.test(path)) return "tauri startup (dynamic import)";
  if (/(Sync|Media|BatchRename|Palette|Duplicates|Space|Shares|Archive|Mount)-.*\.js$/.test(path)) return "on page/action (dynamic import)";
  return "startup";
}
const rows = files.map((p) => {
  const buf = readFileSync(p);
  return {
    file: relative(dist, p).replaceAll("\\", "/"),
    raw: buf.length,
    gzip: gzipSync(buf, { level: 9 }).length,
    brotli: brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
    loaded: loading(p),
  };
}).sort((a, b) => b.raw - a.raw);
const total = rows.reduce((t, r) => ({ raw: t.raw + r.raw, gzip: t.gzip + r.gzip, brotli: t.brotli + r.brotli }), { raw: 0, gzip: 0, brotli: 0 });
const tauriPath = rows.filter((r) => r.loaded !== "mock only (dynamic import)").reduce((t, r) => t + r.gzip, 0);
const startup = rows.filter(r => r.loaded.includes("startup")).reduce((t, r) => t + r.gzip, 0);

let md = "| 文件 | 原始 KiB | gzip KiB | brotli KiB | 加载时机 |\n|---|---:|---:|---:|---|\n";
for (const r of rows) md += `| ${r.file} | ${kb(r.raw)} | ${kb(r.gzip)} | ${kb(r.brotli)} | ${r.loaded} |\n`;
md += `| **合计** | **${kb(total.raw)}** | **${kb(total.gzip)}** | **${kb(total.brotli)}** | |\n`;
console.log(md);
const summary = `桌面启动资源 gzip：${kb(startup)} KiB。全部桌面可加载资源（含按需页面，不含 Mock/DevPanel）gzip：${kb(tauriPath)} KiB。\n这是压缩静态资源口径，不是运行内存或整包体积；Mock 块仍在构建目录中，桌面不会加载。`;
console.log(summary);
writeFileSync(join(root, "docs", "ui-size-report.md"), `# UI 静态资源体积\n\n生成：${new Date().toISOString()}（node ${process.version}，vite build）\n\n${md}\n${summary}\n`);
writeFileSync(join(root, "docs", "evidence", "ui-v3-size.json"), JSON.stringify({ unit: "bytes", compression: "gzip level 9 / brotli quality 11", total, startup_gzip_bytes: startup, desktop_all_gzip_bytes: tauriPath, rows }, null, 2));
