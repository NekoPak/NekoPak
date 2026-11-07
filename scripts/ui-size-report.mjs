#!/usr/bin/env node
// Static asset size report for the built UI (dist/). Raw, gzip -9 and brotli -11.
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
const rows = files.map((p) => {
  const buf = readFileSync(p);
  return {
    file: relative(dist, p),
    raw: buf.length,
    gzip: gzipSync(buf, { level: 9 }).length,
    brotli: brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
    loaded: /backend-.*\.js$/.test(p) ? "mock only (dynamic import)" : /(core|event|window)-.*\.js$/.test(p) ? "tauri only (dynamic import)" : "always",
  };
}).sort((a, b) => b.raw - a.raw);
const total = rows.reduce((t, r) => ({ raw: t.raw + r.raw, gzip: t.gzip + r.gzip, brotli: t.brotli + r.brotli }), { raw: 0, gzip: 0, brotli: 0 });
const tauriPath = rows.filter((r) => r.loaded !== "mock only (dynamic import)").reduce((t, r) => t + r.gzip, 0);

let md = "| 文件 | 原始 KB | gzip KB | brotli KB | 加载时机 |\n|---|---:|---:|---:|---|\n";
for (const r of rows) md += `| ${r.file} | ${kb(r.raw)} | ${kb(r.gzip)} | ${kb(r.brotli)} | ${r.loaded} |\n`;
md += `| **合计** | **${kb(total.raw)}** | **${kb(total.gzip)}** | **${kb(total.brotli)}** | |\n`;
console.log(md);
console.log(`Tauri 运行路径（不含 mock）gzip 合计：${kb(tauriPath)} KB`);
writeFileSync(join(root, "docs", "ui-size-report.md"), `# UI 静态资源体积\n\n生成：${new Date().toISOString()}（node ${process.version}，vite build）\n\n${md}\nTauri 运行路径（不含 mock 块）gzip 合计：${kb(tauriPath)} KB\n`);
