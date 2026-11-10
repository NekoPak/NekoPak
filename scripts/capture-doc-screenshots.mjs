// Capture the built UI with its existing offline demo data. No account or live API.
// npm run build && node scripts/capture-doc-screenshots.mjs
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "..");
const output = join(root, "docs/screenshots/readme");
const port = 4191, base = `http://127.0.0.1:${port}`;
mkdirSync(output, { recursive: true });
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "preview", "--host", "127.0.0.1", "--port", String(port), "--strictPort"], {
  cwd: root, windowsHide: true, stdio: "pipe",
});
let browser;
const errors = [], screenshots = [];
const viewport = { width: 1440, height: 900 };
async function capture(page, name, description) {
  await page.mouse.move(4, 4);
  await page.locator(".toast").waitFor({ state: "detached" });
  await page.evaluate(() => document.fonts.ready);
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), "Screenshot page must fit its viewport");
  const path = join(output, name);
  await page.screenshot({ path, animations: "disabled" });
  screenshots.push({ file: name, description, viewport, bytes: readFileSync(path).length,
    sha256: createHash("sha256").update(readFileSync(path)).digest("hex") });
}
try {
  await new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error("Local screenshot server did not start")), 20000);
    server.stdout.on("data", data => { if (String(data).includes(String(port))) { clearTimeout(timer); resolveReady(); } });
    server.on("exit", code => { clearTimeout(timer); reject(new Error(`Local server exited: ${code}`)); });
  });
  browser = await chromium.launch({ channel: process.env.E2E_BROWSER_CHANNEL ?? (process.platform === "win32" ? "msedge" : undefined) });
  for (const theme of ["light", "dark"]) {
    const context = await browser.newContext({ viewport, locale: "zh-CN", colorScheme: theme, reducedMotion: "reduce" });
    await context.addInitScript(() => {
      localStorage.setItem("nekopak.disclaimer.v1", "1");
      localStorage.setItem("nekopak.language", "zh-CN");
      localStorage.setItem("nekopak.mock.faults", JSON.stringify({ latency_ms: 30 }));
    });
    await context.route("**/*", route => {
      const url = route.request().url();
      if (url.startsWith(`${base}/`) || /^(data|blob):/.test(url)) return route.continue();
      errors.push(`Unexpected external request: ${url}`);
      return route.abort();
    });
    const page = await context.newPage();
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`${base}/?scenario=free#/files/d-docs`);
    await page.locator(".ft-row", { hasText: "线性代数笔记.pdf" }).waitFor();
    await capture(page, `desktop-files-${theme}.png`, `Desktop files, ${theme}; existing offline demo folder`);
    if (theme === "light") {
      await page.locator(".dl-btn").click();
      await page.locator('[data-task="task-1"]').waitFor();
      await capture(page, "desktop-transfers.png", "Desktop transfer queue; simulated demo bytes and speeds");
      await page.goto(`${base}/?scenario=free#/files/d-movies`);
      await page.locator(".ft-row", { hasText: "纪录片 S01E01.mp4" }).click({ button: "right" });
      await page.getByRole("menuitem", { name: "下载指定段落…", exact: true }).click();
      const panel = page.locator(".side-island");
      await panel.locator(".clip-preview img").waitFor();
      await panel.getByRole("radio", { name: "终点", exact: true }).click();
      await panel.getByLabel("终点时间", { exact: true }).fill("03:00");
      await panel.getByLabel("终点时间", { exact: true }).press("Enter");
      await panel.getByRole("radio", { name: "起点", exact: true }).click();
      await panel.getByLabel("起点时间", { exact: true }).fill("01:00");
      await panel.getByLabel("起点时间", { exact: true }).press("Enter");
      await page.waitForFunction(() => document.querySelector(".clip-scale")?.textContent.includes("01:00 至 03:00"));
      await capture(page, "desktop-video-segment.png", "Video time selection; built-in demo preview, not decoded account media");
    }
    await context.close();
  }
  assert.deepEqual(errors, []);
  const report = {
    version: JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version,
    source_commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8", windowsHide: true }).trim(),
    captured_at: new Date().toISOString(), command: "npm run build && node scripts/capture-doc-screenshots.mjs",
    scope: "Built application UI in Edge/Chromium with existing offline demo data. No real accounts, network throughput claim, DOM retouching or image compositing.",
    screenshots, errors,
  };
  writeFileSync(join(output, "capture.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(`Captured ${screenshots.length} documentation screenshots; no external requests or page errors.`);
} finally {
  await browser?.close();
  server.kill();
}
