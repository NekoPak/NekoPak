#!/usr/bin/env node
/**
 * NekoPak UI end-to-end run against the built app in Mock mode.
 *
 *   npm run build && npm run e2e
 *
 * Starts `vite preview` (unless E2E_BASE is set), drives Chromium with
 * Playwright, asserts behaviour, records screenshots to docs/screenshots/,
 * and fails on any console error or any request that leaves localhost.
 * Requires Playwright with a Chromium build (`npm i -D playwright && npx playwright install chromium`).
 */
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
let chromium;
try {
  ({ chromium } = require("playwright"));
} catch {
  console.error("Playwright 未安装：npm i -D playwright && npx playwright install chromium");
  process.exit(2);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const shots = join(root, "docs", "screenshots", "ui-v3");
mkdirSync(shots, { recursive: true });
for (const f of readdirSync(shots)) if (f.startsWith("FAIL-")) rmSync(join(shots, f));
const PORT = 4179;
const BASE = process.env.E2E_BASE ?? `http://localhost:${PORT}`;

let server;
if (!process.env.E2E_BASE) {
  // Run vite's bin directly (not via npx) so kill() really stops the server.
  server = spawn(process.execPath, [join(root, "node_modules", "vite", "bin", "vite.js"), "preview", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", windowsHide: true });
  await new Promise((ok, fail) => {
    let errText = "";
    const t = setTimeout(() => fail(new Error(`preview did not start: ${errText}`)), 20000);
    server.stdout.on("data", (d) => {
      if (String(d).includes(String(PORT))) {
        clearTimeout(t);
        ok();
      }
    });
    server.stderr.on("data", (d) => (errText += String(d)));
    server.on("exit", (code) => {
      clearTimeout(t);
      fail(new Error(`preview exited (${code}): ${errText}`));
    });
  });
  process.on("exit", () => server?.kill());
}

const browser = await chromium.launch(process.env.E2E_BROWSER_CHANNEL ? { channel: process.env.E2E_BROWSER_CHANNEL } : {});
const results = [];
const external = new Set();

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

async function poll(fn, msg, timeout = 6000, step = 100) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await new Promise((r) => setTimeout(r, step));
  }
  throw new Error(`${msg}（超时；最后值：${last instanceof Error ? last.message : JSON.stringify(last)}）`);
}

/**
 * @param {string} name
 * @param {(p: import('playwright').Page) => Promise<void>} fn
 * @param {{ scenario?: string, faults?: object, viewport?: {width:number,height:number}, dpr?: number, accepted?: boolean, theme?: string, hash?: string, ipcFixture?: boolean }} opts
 */
async function test(name, fn, opts = {}) {
  if (process.env.E2E_FILTER && !name.includes(process.env.E2E_FILTER)) return;
  const ctx = await browser.newContext({
    viewport: opts.viewport ?? { width: 1280, height: 720 },
    deviceScaleFactor: opts.dpr ?? 1,
    locale: "zh-CN",
    colorScheme: opts.theme === "dark" ? "dark" : "light",
    permissions: ["clipboard-read", "clipboard-write"],
  });
  const faults = { latency_ms: 30, ...(opts.faults ?? {}) };
  await ctx.addInitScript(([accepted, faults, init]) => {
    try {
      if (!sessionStorage.getItem("e2e-init")) {
        sessionStorage.setItem("e2e-init", "1");
        if (accepted) localStorage.setItem("nekopak.disclaimer.v1", "1");
        localStorage.setItem("nekopak.mock.faults", JSON.stringify(faults));
        if (init) localStorage.setItem("nekopak.prefs.v2", JSON.stringify(init));
      }
    } catch {}
  }, [opts.accepted !== false, faults, opts.init ?? null]);
  // Browser-only IPC fixture exercises native-only UI and snake_case calls.
  // It is not evidence that a real OS mount or account request succeeded.
  if (opts.ipcFixture) await ctx.addInitScript(() => {
    let callbackId = 0, eventId = 0, active = "account-a";
    const callbacks = new Map(), listeners = new Map();
    const accountPrefs = { "account-a": { pins: [{ id: "folder-a", name: "Private A" }] }, "account-b": { pins: [{ id: "folder-b", name: "Private B" }] } };
    const configs = [], states = new Map();
    let startupEnabled = false;
    const references = [];
    const sharedEntries = [{ id: "raw-shared-video", name: "Shared movie.mp4", kind: "file", size_bytes: "100", mime_type: "video/mp4" }, { id: "raw-shared-doc", name: "Shared note.txt", kind: "file", size_bytes: "20", mime_type: "text/plain" }];
    const settings = { schema_version: 1, theme: "light", language: "zh-CN", download_dir: "D:\\Downloads", max_concurrent_transfers: 3, bandwidth_limit_kib_s: 0, cache: { dir: "D:\\Cache", max_bytes: "1000000000" }, experimental_network: { enabled: false, proxy_mode: "system", user_proxy_configured: false, custom_dns_mode: false, connection_diagnostics: false }, telemetry: false, download: { default_quality: "original", connections_per_task: 4 } };
    const auth = () => ({ backend: "rust_compat", authenticated: !!active, state: active ? "signed_in" : "signed_out", account_id: active || undefined, display_name: active === "account-a" ? "a***@example.test" : "b***@example.test", credential_owner: "os_vault", account_tier: "unknown", message: "" });
    const emit = (event, payload) => { for (const [id, item] of listeners) if (item.event === event) callbacks.get(item.handler)?.({ event, id, payload }); };
    window.__ipcFixture = { calls: [], failMount: false };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: callback => { callbacks.set(++callbackId, callback); return callbackId; },
      unregisterCallback: id => callbacks.delete(id),
      invoke: async (command, args = {}) => {
        window.__ipcFixture.calls.push({ command, args: structuredClone(args) });
        if (command === "plugin:event|listen") { listeners.set(++eventId, args); return eventId; }
        if (command === "plugin:event|unlisten") return;
        if (command.startsWith("plugin:window|")) return false;
        if (command === "app_status") return { version: "ipc-fixture", contract_version: "0.2", mode: "rust_compat_preview", storage: { status: "ready", schema_version: 1 }, features: { accounts: true, auth_sign_in: true, auth_logout: true, settings: true, ui_prefs: true, mount: true, local_sources: true, local_media_probe: true, local_clip: true, media_session: true, share_preview: true, upload_directory: true, autostart: true, reference_archive: true } };
        if (command === "settings_get") return settings;
        if (command === "settings_update") return Object.assign(settings, args.patch);
        if (command === "autostart_get") return { enabled: startupEnabled, available: true };
        if (command === "autostart_set") { startupEnabled = args.enabled; return { enabled: startupEnabled, available: true }; }
        if (command === "backend_discover") return [{ backend: "rust_compat", status: active ? "ready" : "needs_login", operations: { list: true, stat: true, rename: true, move: true, copy: true, mkdir: true, upload: true, download: true, cloud_task: true, share: true, star: true, trash: true, restore: true, media_probe: true, external_player: true, clip_download: true }, limitations: [], checked_at: new Date().toISOString() }];
        if (command === "auth_status") return auth();
        if (command === "accounts_list") return ["account-a", "account-b"].map(id => ({ id, display_name: id === "account-a" ? "a***@example.test" : "b***@example.test", active: active === id, state: "signed_in" }));
        if (command === "account_switch") { active = args.id; emit("nekopak://auth/changed", { state: "signed_in" }); return auth(); }
        if (command === "ui_prefs_get") return { account_id: active, prefs: structuredClone(accountPrefs[active] ?? {}) };
        if (command === "ui_prefs_update") {
          if (args.expected_account_id !== active) throw { code: "CONFLICT", message: "wrong account", retryable: false };
          accountPrefs[active] = structuredClone(args.prefs); return args.prefs;
        }
        if (command === "quota_get") return { backend: "rust_compat", source: "account", account_tier: "unknown", dimensions: [], limitations: [], checked_at: new Date().toISOString() };
        if (command === "transfers_list" || command === "jobs_list") return { items: [] };
        if (command === "files_list") return { items: args.parent_id.startsWith("local:") ? [{ id: "local:fixture:file", parent_id: args.parent_id, name: "Local movie.mp4", kind: "file", size_bytes: "100" }] : [] };
        if (command === "files_path") return args.id.startsWith("local:") ? [{ id: "local:fixture:", name: "Local fixture", kind: "folder", size_bytes: "0" }] : [];
        if (command === "shares_list") return { items: [] };
        if (command === "share_open") return { access_ref: "opaque-share", share_id: args.share_id, title: "Shared fixture", parent_id: "raw-share-root", entries: [{ id: "raw-share-folder", name: "Shared folder", kind: "folder", size_bytes: "0" }] };
        if (command === "share_folder") return { access_ref: "opaque-share", share_id: "share-fixture", title: "Shared fixture", parent_id: args.parent_id, entries: sharedEntries };
        if (command === "share_preview_entry") return { ...sharedEntries.find(entry => entry.id === args.file_id), id: `shared:opaque-share:${args.file_id}` };
        if (command === "media_probe") return { remote_id: args.remote_id, duration_ms: 12000, qualities: [{ id: "original", label: "Original", container: "original", readable: true }, { id: "720p", label: "720P", container: "mp4", readable: true }], keyframes: "indexed", preview: "unavailable", probed_at: new Date().toISOString() };
        if (command === "media_session_open") throw { code: "UNSUPPORTED", message: "IPC fixture media reader", retryable: false };
        if (command === "media_session_close") return;
        if (command === "transfer_start") return { task_id: `shared-task-${args.remote_id}` };
        if (command === "upload_directory_pick_and_start") return { task_ids: ["directory-file-1", "directory-file-2"], skipped: [], expanded_files: 2 };
        if (command === "share_restore") return { succeeded: args.ids, failed: [], replayed: false };
        if (command === "reference_list") return structuredClone(references);
        if (command === "reference_create") {
          const manifest = { manifest_id: `manifest-${references.length}`, account_id: active, created_at: new Date().toISOString(), content_sha256: "a".repeat(64), state: "prepared", roots: args.ids, entries: [{ path: "Original.txt", entry: { id: "original-file", name: "Original.txt", kind: "file", size_bytes: "12" }, state: "recorded" }] };
          references.push(manifest); return structuredClone(manifest);
        }
        if (command === "reference_status") return structuredClone(references.find(item => item.manifest_id === args.manifest_id));
        if (command === "reference_store_manifest") { const manifest = references.find(item => item.manifest_id === args.manifest_id); manifest.state = "verified"; manifest.remote_manifest_id = "cloud-manifest"; manifest.manifest_task_id = "upload-manifest"; return { task_id: "upload-manifest" }; }
        if (command === "reference_archive") { const manifest = references.find(item => item.manifest_id === args.manifest_id); if (!args.confirm_trash || manifest.state !== "verified") throw { code: "CONFLICT", message: "not verified", retryable: false }; manifest.state = "archived"; manifest.entries[0].state = "trashed"; return structuredClone(manifest); }
        if (command === "reference_restore") { const manifest = references.find(item => item.manifest_id === args.manifest_id); manifest.state = args.allow_recreate ? "restored" : "partial"; manifest.entries[0].state = args.allow_recreate ? "restored" : "failed"; return structuredClone(manifest); }
        if (command === "mount_capabilities") return { platform: "windows", checked_at: new Date().toISOString(), modes: ["cloud_files_sync_root", "winfsp_drive"].map(mode => ({ mode, available: true, checks: [{ id: "os", label: "OS fixture prerequisite", status: "pass", detail: "Synthetic IPC response" }] })) };
        if (command === "mount_configs_list") return structuredClone(configs);
        if (command === "mount_config_save") { const config = { ...args.config, config_id: args.config.config_id || `config-${configs.length}` }; const index = configs.findIndex(c => c.config_id === config.config_id); if (index >= 0) configs[index] = config; else configs.push(config); return config; }
        if (command === "mount_status") return states.get(args.id) ?? { id: args.id, status: "stopped", read_only: true };
        if (command === "mount_start") {
          if (window.__ipcFixture.failMount) throw { code: "INTEGRITY_FAILED", message: "fixture range verification failed", retryable: false };
          const config = configs.find(c => c.config_id === args.config_id);
          const state = { id: args.config_id, status: "mounted", mode: config.mode, read_only: true, mount_point: config.local_path ?? config.drive_letter }; states.set(args.config_id, state); return state;
        }
        if (command === "mount_stop") { states.set(args.id, { id: args.id, status: "stopped", read_only: true }); return; }
        if (command === "download_directory_pick") return "D:\\Mounts";
        throw { code: "UNSUPPORTED", message: `IPC fixture: ${command}`, retryable: false };
      },
    };
  });
  const page = await ctx.newPage();
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") problems.push(`console.error: ${m.text()}`);
  });
  page.on("request", (r) => {
    const u = r.url();
    if (!u.startsWith(BASE) && !u.startsWith("data:") && !u.startsWith("blob:")) external.add(u);
  });
  const t0 = Date.now();
  try {
    await page.goto(`${BASE}/?scenario=${opts.scenario ?? "free"}${opts.hash ?? "#/files"}`);
    await page.waitForSelector(".app, .onboarding", { timeout: 8000 });
    await fn(page);
    assert(problems.length === 0, problems.join(" | "));
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`  ✓ ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    results.push({ name, ok: false, ms: Date.now() - t0, error: e.message });
    console.log(`  ✗ ${name}\n      ${e.message}`);
    await page.screenshot({ path: join(shots, `FAIL-${name.replace(/[^\w一-龥-]+/g, "_")}.png`) }).catch(() => {});
  } finally {
    await ctx.close();
  }
}

const shot = (page, file) => page.screenshot({ path: join(shots, file), animations: "disabled" });
const grid = (page) => page.locator('[role="grid"]').first();
const esc = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/** Case-sensitive (Playwright's string hasText ignores case: "Report.pdf" ≠ "report.pdf"). */
const row = (page, text) => page.locator(".ft-row", { hasText: new RegExp(esc(text)) }).first();
/** Rows are virtualised: scroll the list until a row with `text` is rendered, then centre it. */
async function reveal(page, text) {
  const sc = page.locator(".ft-scroll").first();
  await sc.waitFor();
  const re = new RegExp(esc(text));
  await sc.evaluate((el) => (el.scrollTop = 0));
  for (let i = 0; i < 60; i++) {
    await page.waitForTimeout(120); // let the virtual window settle after scrolling
    const ok = await page.evaluate(([src, flags]) => {
      const r = new RegExp(src, flags);
      const el = [...document.querySelectorAll(".ft-row, .card")].find((x) => r.test(x.textContent ?? ""));
      if (!el) return false;
      el.scrollIntoView({ block: "center" });
      return true;
    }, [re.source, re.flags]);
    if (ok) {
      await page.waitForTimeout(150);
      if ((await page.locator(".ft-row, .card", { hasText: re }).count()) > 0) return;
    }
    await sc.evaluate((el) => (el.scrollTop += Math.max(200, el.clientHeight - 80)));
  }
  throw new Error(`列表中找不到“${text}”`);
}
const dialog = (page) => page.locator('[role="dialog"].modal');
const sideIsland = (page) => page.locator(".side-island");
const tasksBtn = (page) => page.locator(".dl-btn");
const cmds = (page) => page.locator(".pf-commands");
const toastText = async (page, text) => poll(async () => (await page.locator(".toast").allInnerTexts()).some((t) => t.includes(text)), `等待提示“${text}”`);

console.log(`NekoPak UI E2E → ${BASE}`);

await test("IPC 契约夹具：引用归档先核验，回收站与GCID取回分别确认", async p => {
  await p.getByRole("link", { name: "引用归档", exact: true }).click();
  await p.getByRole("button", { name: "选择目录创建清单", exact: true }).click();
  await dialog(p).getByRole("button", { name: "创建只读清单", exact: true }).click();
  await p.getByText("清单已生成", { exact: true }).waitFor();
  const archive = p.getByRole("button", { name: "归档文件…", exact: true });
  assert(await archive.getAttribute("aria-disabled") === "true", "只读清单尚未云核验不得归档");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "reference_archive" || c.command === "files_mutate").length) === 0, "create不得发破坏操作");
  await p.getByRole("button", { name: "上传并核验清单", exact: true }).click();
  await p.getByText("云端清单已核验", { exact: true }).waitFor();
  await archive.click();
  assert(await dialog(p).getByRole("button", { name: "确认归档", exact: true }).getAttribute("aria-disabled") === "true", "归档确认必须勾选");
  await dialog(p).getByRole("checkbox").check(); await dialog(p).getByRole("button", { name: "确认归档", exact: true }).click();
  await p.getByText("已归档", { exact: true }).waitFor();
  await p.getByRole("button", { name: "从回收站取回", exact: true }).click();
  await p.getByText("部分完成，需处理", { exact: true }).waitFor();
  assert(await p.evaluate(() => window.__ipcFixture.calls.find(c => c.command === "reference_restore").args.allow_recreate) === false, "首次取回不得允许GCID");
  await p.getByRole("button", { name: "允许重建缺失文件…", exact: true }).click();
  assert(await dialog(p).getByRole("button", { name: "允许重建并取回", exact: true }).getAttribute("aria-disabled") === "true", "GCID单独同意");
  await dialog(p).getByRole("checkbox").check(); await dialog(p).getByRole("button", { name: "允许重建并取回", exact: true }).click();
  await p.locator(".reference-detail > .spread").getByText("已取回", { exact: true }).waitFor();
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "reference_restore").map(c => c.args.allow_recreate).join(",")) === "false,true", "明确区分两种取回");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "reference_archive").every(c => c.args.confirm_trash === true)), "实际归档携带显式确认");
  await p.setViewportSize({ width: 640, height: 360 });
  assert(await p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1), "引用页面无横向溢出");
}, { ipcFixture: true });

await test("IPC 契约夹具：自动启动仅用户切换时写宿主配置", async p => {
  const startup = p.getByRole("switch", { name: "开机自动启动", exact: true });
  await poll(async () => await startup.getAttribute("aria-disabled") !== "true", "读取自动启动状态");
  assert(await startup.getAttribute("aria-checked") === "false", "初始不开启自动启动");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "autostart_set").length) === 0, "读取不得更改宿主配置");
  for (const enabled of [true, false]) {
    await startup.click();
    await poll(async () => await startup.getAttribute("aria-checked") === String(enabled), "显示宿主保存结果");
    await p.getByRole("link", { name: "网络", exact: true }).click();
    await p.getByRole("link", { name: "外观", exact: true }).click();
    await poll(async () => await startup.getAttribute("aria-checked") === String(enabled), "重开恢复宿主配置");
  }
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "autostart_set").map(c => c.args.enabled).join(",")) === "true,false", "每次用户切换只写一次");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "settings_update").length) === 0, "启动设置不得写入普通settings_update");
}, { ipcFixture: true, hash: "#/settings/general" });

await test("IPC 契约夹具：目录上传调用独立选择器且不写入本地映射", async p => {
  await cmds(p).getByRole("button", { name: "新建", exact: true }).click();
  await p.getByRole("menuitem", { name: "上传文件夹", exact: true }).click();
  await toastText(p, "已加入上传队列 2 项");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "upload_directory_pick_and_start" && c.args.parent_id === "root").length) === 1, "必须调用目录上传专属IPC");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "upload_pick_and_start").length) === 0, "目录上传不得打开单文件picker");
  await p.evaluate(() => { location.hash = "#/files/local%3Afixture%3A"; });
  await row(p, "Local movie.mp4").waitFor();
  assert(await cmds(p).getByRole("button", { name: "新建", exact: true }).getAttribute("aria-disabled") === "true", "本地映射禁用上传菜单");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "upload_directory_pick_and_start").length) === 1, "local映射不得发上传IPC");
}, { ipcFixture: true });

await test("IPC 契约夹具：分享预览保留原ID导航，播放和下载使用只读句柄", async p => {
  await p.getByRole("link", { name: "我的分享", exact: true }).click();
  await p.getByRole("button", { name: "打开分享", exact: true }).click();
  await dialog(p).getByLabel("分享链接", { exact: true }).fill("https://mypikpak.com/s/share-fixture");
  await dialog(p).getByRole("button", { name: "预览分享", exact: true }).click();
  await dialog(p).getByRole("button", { name: "Shared folder", exact: true }).click();
  const movie = p.locator(".shared-entries li", { hasText: "Shared movie.mp4" });
  await movie.waitFor();
  await movie.getByRole("checkbox").check();
  await movie.getByRole("button", { name: "播放或查看", exact: true }).click();
  await dialog(p).getByText("IPC fixture media reader", { exact: true }).waitFor();
  assert(await p.locator('.modal[aria-hidden="true"][inert]').count() === 1, "下层分享面板必须hidden+inert");
  await dialog(p).getByRole("button", { name: "关闭", exact: true }).focus();
  for (let n = 0; n < 9; n++) {
    await p.keyboard.press("Tab");
    assert(await p.evaluate(() => document.activeElement?.closest('[role="dialog"]')?.getAttribute("aria-hidden") !== "true" && !!document.activeElement?.closest('[role="dialog"]')), "焦点不得离开顶层对话框");
  }
  assert(await dialog(p).getByRole("button", { name: "下载片段", exact: true }).getAttribute("aria-disabled") === "true", "分享片段不能借云盘能力启用");
  assert(await dialog(p).getByRole("option", { name: "720P", exact: true }).count() === 0, "分享不应展示未验证的转码档");
  await dialog(p).getByRole("button", { name: "关闭", exact: true }).click();
  await movie.waitFor();
  assert(await movie.getByRole("checkbox").isChecked(), "播放器关闭后保留分享选择");
  await movie.getByRole("button", { name: "下载原始文件", exact: true }).click();
  await poll(() => p.evaluate(() => window.__ipcFixture.calls.some(c => c.command === "transfer_start" && c.args.remote_id === "shared:opaque-share:raw-shared-video" && c.args.options === undefined)), "使用encoded句柄下载原始文件");
  await dialog(p).getByRole("button", { name: "转存所选", exact: true }).click();
  await dialog(p).getByRole("button", { name: "保存到这里", exact: true }).click();
  await movie.waitFor();
  await poll(() => p.evaluate(() => window.__ipcFixture.calls.some(c => c.command === "share_restore" && c.args.access_ref === "opaque-share" && c.args.ids[0] === "raw-shared-video")), "转存使用原始ID");
  assert(await p.evaluate(() => window.__ipcFixture.calls.some(c => c.command === "share_folder" && c.args.parent_id === "raw-share-folder")), "导航保留原始目录ID");
  assert(await p.evaluate(() => window.__ipcFixture.calls.some(c => c.command === "media_session_open" && c.args.remote_id === "shared:opaque-share:raw-shared-video")), "播放器使用encoded句柄");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "files_mutate").length) === 0, "分享入口不得调用files_mutate");
}, { ipcFixture: true });

await test("IPC 契约夹具：access 域名屏蔽独立开关与保存", async p => {
  const block = p.getByRole("switch", { name: "屏蔽 access 域名", exact: true });
  const network = p.getByRole("switch", { name: "启用网络增强", exact: true });
  assert(await block.getAttribute("aria-checked") === "false", "屏蔽默认关闭");
  assert(await network.getAttribute("aria-checked") === "false", "网络增强保持关闭");
  for (const domain of ["access.mypikpak.com", "access.mypikpak.net", "access.pikpak.me", "access.pikpakdrive.com"]) await p.locator(".access-domains code", { hasText: domain }).waitFor();
  for (const value of [true, false]) {
    await block.click();
    assert(await network.getAttribute("aria-checked") === "false", "屏蔽不应开启网络增强");
    await p.getByRole("button", { name: "保存", exact: true }).click();
    await poll(() => p.evaluate(value => window.__ipcFixture.calls.filter(c => c.command === "settings_update").some(c => c.args.patch.experimental_network?.enabled === false && c.args.patch.experimental_network?.block_access_domains === value), value), "独立保存屏蔽设置");
    await p.getByRole("link", { name: "外观", exact: true }).click();
    await p.getByRole("link", { name: "网络", exact: true }).click();
    assert(await block.getAttribute("aria-checked") === String(value), "重新打开网络页恢复已保存值");
  }
  await p.setViewportSize({ width: 640, height: 360 });
  assert(await p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1), "域名文案不得造成窄窗口横向溢出");
}, { ipcFixture: true, hash: "#/settings/network" });

await test("IPC 契约夹具：账号切换恢复各自固定目录且不写浏览器存储", async p => {
  await p.getByRole("link", { name: "Private A", exact: true }).waitFor();
  const second = p.locator(".account-list li", { hasText: "b***@example.test" });
  await second.getByRole("button", { name: "切换账号", exact: true }).click();
  await p.getByRole("link", { name: "Private B", exact: true }).waitFor();
  assert(await p.getByRole("link", { name: "Private A", exact: true }).count() === 0, "切换后不能残留A固定目录");
  await p.getByRole("button", { name: "添加账号", exact: true }).click();
  await dialog(p).getByLabel("账号", { exact: true }).focus();
  await p.keyboard.press("Escape");
  await dialog(p).waitFor({ state: "detached" });
  await p.getByRole("link", { name: "外观", exact: true }).click();
  await p.getByRole("radio", { name: "紧凑", exact: true }).click();
  await poll(() => p.evaluate(() => window.__ipcFixture.calls.some(c => c.command === "ui_prefs_update" && c.args.expected_account_id === "account-b" && c.args.prefs.density === "compact")), "按B身份保存显示设置");
  const cached = await p.evaluate(() => localStorage.getItem("nekopak.prefs.v2"));
  assert(!cached.includes("Private") && !cached.includes('"pins"'), "原生固定目录不得保存在浏览器存储");
}, { ipcFixture: true, hash: "#/settings/account" });

await test("IPC 契约夹具：只读本地目录禁用云端修改及快捷键", async p => {
  await p.evaluate(() => { location.hash = "#/files/local%3Afixture%3A"; });
  await row(p, "Local movie.mp4").click();
  for (const label of ["移动到…", "复制到…", "重命名（F2）", "分享…", "移至回收站（Delete）"]) assert(await cmds(p).getByRole("button", { name: label, exact: true }).getAttribute("aria-disabled") === "true", `${label}应禁用`);
  await grid(p).focus(); await p.keyboard.press("F2"); await p.keyboard.press("Delete"); await p.keyboard.press("Control+x");
  assert(await dialog(p).count() === 0, "本地快捷键不应打开远端修改表单");
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "files_mutate").length) === 0, "不得提交local IDs到files_mutate");
}, { ipcFixture: true });

await test("IPC 契约夹具：挂载需显式配置和连接，失败保持未连接", async p => {
  await p.getByRole("link", { name: "挂载", exact: true }).click();
  await p.locator(".mount-mode").first().waitFor();
  const cloud = p.locator(".mount-mode").filter({ hasText: "同步文件夹（按需文件）" });
  await cloud.getByRole("button", { name: "创建同步文件夹", exact: true }).click();
  await dialog(p).getByRole("button", { name: "选择父文件夹", exact: true }).click();
  assert((await p.locator("#mount-local").inputValue()).startsWith("D:\\Mounts\\NekoPak-"), "选择父目录后使用新子目录");
  await dialog(p).getByRole("button", { name: "保存配置", exact: true }).click();
  await cloud.getByText("已配置，未连接", { exact: true }).waitFor();
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "mount_start").length) === 0, "保存配置不能自动挂载");
  await p.evaluate(() => { window.__ipcFixture.failMount = true; });
  await cloud.getByRole("button", { name: "连接挂载", exact: true }).click();
  await cloud.getByText("fixture range verification failed", { exact: true }).waitFor();
  assert(await cloud.getByText("已挂载（只读）", { exact: true }).count() === 0, "Range失败不能显示成功");
  await p.evaluate(() => { window.__ipcFixture.failMount = false; });
  await cloud.getByRole("button", { name: "连接挂载", exact: true }).click();
  await cloud.getByText("已挂载（只读）", { exact: true }).waitFor();
  await cloud.getByRole("button", { name: "停止挂载", exact: true }).click();
  await dialog(p).getByText("停止后保留同步根和占位文件。重新连接前，尚未下载的文件暂时无法打开。", { exact: true }).waitFor();
  await dialog(p).getByRole("button", { name: "停止挂载", exact: true }).click();
  await cloud.getByText("已配置，未连接", { exact: true }).waitFor();
  assert(await p.evaluate(() => window.__ipcFixture.calls.filter(c => c.command === "mount_start").every(c => c.args.read_only === true)), "始终只读");
  await p.locator(".mount-notice", { hasText: "WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos" }).waitFor();
}, { ipcFixture: true, hash: "#/mount" });

await test("多标签：独立目录历史、选择恢复、快捷键与关闭", async p => {
  await grid(p).waitFor();
  await row(p,"我的资料").click({button:"right"});
  await p.getByRole("menuitem",{name:"在新标签页打开",exact:true}).click();
  await poll(async()=>await p.locator('.workspace-tab').count()===2,"第二个标签页");
  await row(p,"C 语言程序设计 习题集.zip").click();
  const secondId = await p.locator('.workspace-tab.active [role=tab]').getAttribute('id');
  await p.locator('.workspace-tab [role=tab]').first().click();
  await row(p,"我的资料").waitFor();
  await p.locator(`#${secondId}`).click();
  await poll(async()=>await row(p,"C 语言程序设计 习题集.zip").getAttribute('aria-selected')==='true',"标签页恢复选择");
  await p.keyboard.press('Control+t');
  await poll(async()=>await p.locator('.workspace-tab').count()===3,"Ctrl+T");
  await p.keyboard.press('Control+w');
  await poll(async()=>await p.locator('.workspace-tab').count()===2,"Ctrl+W");
});

await test("五语切换：即时更新、同一文档、重启保留",async p=>{
  await p.goto(`${BASE}/?scenario=free#/settings`);
  await p.locator('.settings-section').waitFor();
  await p.evaluate(()=>{window.languageDocumentMarker='same-document';});
  const langs=[['en','Language','Appearance'],['zh-TW','介面語言','外觀'],['ja-JP','表示言語','外観'],['ko-KR','화면 언어','모양'],['zh-CN','界面语言','外观']];
  for(const [locale,,heading] of langs){
    await p.locator('.set-control select').first().selectOption(locale);
    await poll(async()=>await p.locator('html').getAttribute('lang')===locale,locale);
    assert(await p.evaluate(()=>window.languageDocumentMarker)==='same-document','切换不能刷新文档');
    await p.getByRole('heading',{name:heading,exact:true}).waitFor();
  }
  await p.locator('.set-control select').first().selectOption('en');
  await p.getByRole('heading',{name:'Appearance',exact:true}).waitFor();
  await p.reload();
  await p.getByRole('heading',{name:'Appearance',exact:true}).waitFor();
  assert(await p.locator('html').getAttribute('lang')==='en','重载保留English');
});

/* ---------------------------------------------------------------- */

await test("首次启动：NekoPak 内登录 → 进入云盘；界面没有后端选择", async (p) => {
  await p.getByRole("heading", { name: "登录 PikPak" }).waitFor();
  assert((await p.locator('input[type="password"]').count()) === 1, "应有密码框");
  assert((await p.getByText(/官方 CLI|后端|直连|Connected Apps/).count()) === 0, "登录页不应出现后端/CLI 字样");
  await shot(p, "01-sign-in.png");
  await p.getByLabel("账号").fill("kiana@example.com");
  await p.getByLabel("密码", { exact: true }).fill("secret");
  await p.locator(".signin").getByRole("button", { name: "登录", exact: true }).click();
  await grid(p).waitFor();
  assert((await p.locator(".ft-row").count()) >= 20, "根目录应有文件");
  assert((await p.locator('input[type="password"]').count()) === 0, "登录后不应残留密码框");
}, { accepted: false, scenario: "unauthorized" });

await test("登录：密码错误、PikPak 安全验证、短信验证码", async (p) => {
  await p.getByRole("heading", { name: "登录 PikPak" }).waitFor();
  const user = p.getByLabel("账号");
  const pass = p.getByLabel("密码", { exact: true });
  await user.fill("kiana@example.com");
  await pass.fill("wrong");
  await p.locator(".signin").getByRole("button", { name: "登录", exact: true }).click();
  await p.getByRole("alert").filter({ hasText: "账号或密码不正确" }).waitFor();
  assert((await pass.inputValue()) === "", "密码错误后应清空密码框");
  await pass.fill("captcha");
  await p.locator(".signin").getByRole("button", { name: "登录", exact: true }).click();
  await p.getByText("PikPak 要求先完成一次安全验证").waitFor();
  await p.getByRole("button", { name: "返回" }).click();
  await user.fill("+86 138 0000 1234");
  await pass.fill("x");
  await p.locator(".signin").getByRole("button", { name: "登录", exact: true }).click();
  await p.getByText("验证码已发送到手机").waitFor();
  await p.getByLabel("验证码").fill("000000");
  await p.getByRole("button", { name: "验证并登录" }).click();
  await p.getByRole("alert").filter({ hasText: "验证码不正确" }).waitFor();
  await p.getByLabel("验证码").fill("123456");
  await p.getByRole("button", { name: "验证并登录" }).click();
  await grid(p).waitFor();
}, { scenario: "unauthorized", accepted: false });

await test("文件：双击进入、面包屑、Backspace 返回、键盘多选", async (p) => {
  await grid(p).waitFor();
  await row(p, "我的资料").dblclick();
  await poll(async () => (await p.locator("h1.crumb.current").innerText()).trim() === "我的资料", "进入“我的资料”");
  await grid(p).focus();
  await p.keyboard.press("Backspace");
  await poll(async () => (await p.locator("h1.crumb.current").innerText()).trim() === "网盘", "返回根目录");
  await grid(p).focus();
  const top0 = (await p.locator(".ft-scroll").boundingBox()).y;
  await p.keyboard.press("Home");
  await p.keyboard.press("ArrowDown");
  const top1 = (await p.locator(".ft-scroll").boundingBox()).y;
  assert(top0 === top1, `选择后列表发生位移 ${top0} → ${top1}`);
  await p.keyboard.press("Shift+ArrowDown");
  await p.keyboard.press("Shift+ArrowDown");
  await poll(async () => (await p.locator(".sel-count").innerText()).startsWith("已选 3 项"), "Shift 扩展选择 3 项");
  await p.keyboard.press("Control+a");
  const total = await p.locator(".cmd-meta").first().innerText();
  await poll(async () => (await p.locator(".sel-count").innerText()).includes(`已选 ${parseInt(total.replace(/,/g, ""))} 项`), "Ctrl+A 全选");
  await p.keyboard.press("Escape");
  await poll(async () => (await p.locator(".sel-count").count()) === 0, "Esc 清除选择");
});

await test("右键菜单：键盘打开、方向键、属性中显示 2^53+1 精确字节", async (p) => {
  await grid(p).waitFor();
  await grid(p).focus();
  await p.keyboard.press("Home");
  await p.keyboard.press("Shift+F10");
  await p.locator('[role="menu"]').waitFor();
  await p.keyboard.press("ArrowDown");
  assert(await p.locator(".menu-item.active").count() === 1, "菜单应有活动项");
  await p.keyboard.press("Escape");
  assert(await p.locator('[role="menu"]').count() === 0, "Esc 关闭菜单");
  await row(p, "精度测试").click({ button: "right" });
  const items = await p.locator(".menu-item").allInnerTexts();
  for (const q of ["剪切", "复制", "重命名", "添加星标", "分享…"]) assert((await p.locator(`.menu-qbtn[aria-label^="${q}"]`).count()) === 1, `快捷行缺少 ${q}`);
  for (const t of ["下载", "移动到…", "复制路径", "属性", "移至回收站"]) assert(items.some((x) => x.includes(t)), `菜单缺少 ${t}`);
  assert(!items.some((x) => x.includes("下载指定段落")), "非视频不应出现片段下载");
  await shot(p, "04-context-menu.png");
  await p.locator(".menu-item", { hasText: "属性" }).last().click();
  await sideIsland(p).getByText("9,007,199,254,740,993 字节").waitFor();
});

await test("重命名：Windows 保留名警告、同名阻止、成功提交", async (p) => {
  await grid(p).waitFor();
  await row(p, "空文件.txt").click();
  await grid(p).focus();
  await p.keyboard.press("F2");
  const input = p.locator("#name-input");
  await input.waitFor();
  await input.fill("PRN");
  await dialog(p).getByText("Windows 保留设备名").waitFor();
  await input.fill("Report.pdf");
  await dialog(p).locator(".field-error", { hasText: "此文件夹中已有同名项" }).waitFor();
  await shot(p, "05-rename-validation.png");
  await input.fill("e2e 重命名 ✓.txt");
  await dialog(p).getByRole("button", { name: "重命名" }).click();
  await toastText(p, "已重命名");
  await row(p, "e2e 重命名 ✓.txt").waitFor();
});

await test("新建文件夹（Ctrl+Shift+N）与移至回收站 → 还原 → 永久删除需确认", async (p) => {
  await grid(p).waitFor();
  await p.keyboard.press("Control+Shift+N");
  await p.locator("#name-input").fill("E2E 临时文件夹");
  await dialog(p).getByRole("button", { name: "新建" }).click();
  await toastText(p, "已新建文件夹");
  await row(p, "E2E 临时文件夹").click();
  await grid(p).focus();
  await p.keyboard.press("Delete");
  await dialog(p).getByRole("button", { name: "移至回收站" }).click();
  await toastText(p, "已移至回收站");
  await p.getByRole("link", { name: "回收站" }).click();
  await row(p, "E2E 临时文件夹").click();
  await p.getByRole("button", { name: "还原" }).click();
  await toastText(p, "已还原");
  await row(p, "旧版本 安装包.exe").click();
  await p.getByRole("button", { name: "永久删除" }).click();
  const confirm = dialog(p).getByRole("button", { name: "永久删除" });
  assert((await confirm.getAttribute("aria-disabled")) === "true", "未勾选前永久删除应禁用");
  await dialog(p).getByRole("checkbox").check();
  await confirm.click();
  await toastText(p, "已永久删除");
});

await test("超大目录：12,000 项分页加载且保持虚拟化", async (p) => {
  await grid(p).waitFor();
  await row(p, "超大目录").dblclick();
  await poll(async () => (await p.locator("h1.crumb.current").innerText()).startsWith("超大目录"), "进入超大目录");
  const scroller = p.locator(".ft-scroll");
  await poll(async () => {
    await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
    return (await p.locator(".cmd-meta").first().innerText()) === "12,000 项";
  }, "加载全部 12,000 项", 20000, 250);
  const rendered = await p.locator(".ft-row").count();
  assert(rendered < 120, `DOM 中渲染了 ${rendered} 行，未虚拟化`);
  await shot(p, "06-huge-dir.png");
});

await test("NOT_FOUND：已删除的文件夹显示原因并可返回", async (p) => {
  await grid(p).waitFor();
  await row(p, "已在其他设备删除的文件夹").dblclick();
  await p.locator('[data-error-code="NOT_FOUND"]').waitFor();
  await p.getByRole("button", { name: "返回上一级" }).click();
  await grid(p).waitFor();
});

await test("下载：加入队列；大小写同名目标冲突给出 CONFLICT", async (p) => {
  await grid(p).waitFor();
  await reveal(p, "Report.pdf");
  await row(p, "Report.pdf").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "已加入传输队列 1 项");
  await reveal(p, "report.pdf");
  await row(p, "report.pdf").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "目标位置已有同名文件或任务");
  await tasksBtn(p).click();
  await p.locator(".task", { hasText: "Report.pdf" }).first().waitFor();
});

await test("传输：暂停 / 继续；乱序事件被丢弃", async (p) => {
  await p.locator('.task[data-task="task-1"]').waitFor();
  const t1 = p.locator('.task[data-task="task-1"]');
  await t1.getByRole("button", { name: "暂停" }).click();
  await poll(async () => (await t1.locator(".task-status").innerText()).includes("已暂停"), "task-1 已暂停");
  await t1.getByRole("button", { name: "继续" }).click();
  await poll(async () => /排队中|下载中/.test(await t1.locator(".task-status").innerText()), "task-1 继续");
  await shot(p, "07-transfers-active.png");
  await p.waitForTimeout(2500);
  await p.getByRole("button", { name: "设置（Ctrl+,）" }).click();
  await p.getByRole("link", { name: "开发者", exact: true }).click();
  await poll(async () => Number((await p.locator("[data-dropped]").innerText()).replace(/,/g, "")) > 0, "应统计被丢弃的过期事件", 8000, 250);
}, { hash: "#/transfers", faults: { out_of_order: true } });

await test("故障注入：37% 断网 → 重试等待 → 继续", async (p) => {
  await grid(p).waitFor();
  await row(p, "我的资料").dblclick();
  await row(p, "实验报告.docx").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "已加入传输队列");
  await tasksBtn(p).click();
  // Free the concurrency slots so the new task can start.
  await p.getByRole("button", { name: "全部暂停" }).click();
  await toastText(p, "已暂停");
  const task = p.locator(".task", { hasText: "实验报告.docx" }).filter({ has: p.locator(".task-kind[title='下载']") }).first();
  await task.getByRole("button", { name: "继续" }).click();
  await poll(async () => (await task.innerText()).includes("重试等待"), "进入重试等待", 10000, 80);
  assert((await task.innerText()).includes("37% 处断网"), "应显示断网原因");
}, { faults: { drop_at_37: true } });

await test("故障注入：完整性校验失败保留 .part", async (p) => {
  await grid(p).waitFor();
  await row(p, "项目 Projects").dblclick();
  await row(p, "Cargo.toml").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "已加入传输队列");
  await tasksBtn(p).click();
  await p.getByRole("button", { name: "全部暂停" }).click();
  await toastText(p, "已暂停");
  await p.locator(".task", { hasText: "Cargo.toml" }).getByRole("button", { name: "继续" }).click();
  await p.locator(".task", { hasText: "Cargo.toml" }).getByText("完整性校验失败").waitFor({ timeout: 10000 });
  await shot(p, "08-transfers-failed.png");
}, { faults: { integrity_fail: true } });

await test("模拟重启：从任务快照恢复", async (p) => {
  await p.locator('.task[data-task="task-1"]').waitFor();
  await p.locator(".sb-flavor").click();
  await dialog(p).getByRole("button", { name: "模拟应用重启" }).click();
  await toastText(p, "已模拟重启");
}, { hash: "#/transfers" });

await test("云下载：校验链接、提交、不把“提交”当“完成”", async (p) => {
  await grid(p).waitFor();
  await cmds(p).getByRole("button", { name: "添加链接" }).click();
  const ta = p.locator("#cloud-src");
  await ta.fill("这不是链接");
  await sideIsland(p).getByText("有 1 行不是可识别").waitFor();
  assert((await sideIsland(p).getByRole("button", { name: "提交云下载" }).getAttribute("aria-disabled")) === "true", "无效链接时提交应禁用");
  // Two links: no per-file preview, so the size stays unknown until the server resolves it.
  await ta.fill("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=E2E%20Test%20ISO\nmagnet:?xt=urn:btih:1123456789abcdef0123456789abcdef01234567&dn=Second");
  await sideIsland(p).getByRole("button", { name: "提交云下载" }).click();
  await toastText(p, "提交成功不代表下载完成");
  await tasksBtn(p).click();
  await p.getByRole("tab", { name: /云下载/ }).click();
  const t = p.locator(".task", { hasText: "E2E Test ISO" });
  await t.waitFor();
  assert((await t.innerText()).includes("大小未知"), "元数据解析前应显示“大小未知”");
  assert((await t.locator(".lp-unknown .lp-fill").count()) === 0, "总大小未知时进度条不得有填充");
});

await test("限额耗尽：横幅、禁用下载与云下载、列表仍可浏览", async (p) => {
  await p.getByText("额度已用完，重置前不能开始新的下载").waitFor();
  await grid(p).waitFor();
  await row(p, "片段.mp4").click();
  const dl = cmds(p).getByRole("button", { name: "下载", exact: true });
  assert((await dl.getAttribute("aria-disabled")) === "true", "下载应禁用");
  assert(((await dl.getAttribute("title")) ?? "").includes("配额已用尽"), "下载按钮应说明原因");
  assert((await p.getByText(/切换后端|Connected Apps/).count()) === 0, "不应出现后端/Connected Apps 说辞");
  await shot(p, "13-quota-exhausted.png");
  await p.goto(`${BASE}/?scenario=quota_exhausted#/settings/quota`);
  await p.locator(".quota-row.q-exhausted").first().waitFor();
}, { scenario: "quota_exhausted" });

await test("网络中断：列表错误可重试、任务进入重试、扫描失败不生成删除", async (p) => {
  await p.locator('[data-error-code="TRANSIENT_NETWORK"]').waitFor();
  assert(await p.getByRole("button", { name: "重试" }).count() > 0, "应提供重试");
  await shot(p, "14-network-down.png");
  await tasksBtn(p).click();
  await poll(async () => (await p.locator(".task-status", { hasText: "重试等待" }).count()) > 0, "任务进入重试等待");
  await p.getByRole("link", { name: "自动同步" }).click();
  await p.getByRole("button", { name: "扫描并生成计划" }).click();
  await p.getByText("扫描未完成，因此没有生成任何操作").waitFor({ timeout: 8000 });
}, { scenario: "network_down" });

await test("令牌过期：横幅、保留历史、重新授权后恢复", async (p) => {
  await p.getByText("登录已过期，请重新登录。").first().waitFor();
  await p.getByRole("heading", { name: "登录已过期" }).waitFor();
  await shot(p, "15-auth-expired.png");
  await tasksBtn(p).click();
  await p.locator('.task[data-task="task-9"]').waitFor();
  await p.getByRole("button", { name: "重新登录" }).click();
  await p.getByLabel("账号").fill("kiana@example.com");
  await p.getByLabel("密码", { exact: true }).fill("secret");
  await p.locator(".signin").getByRole("button", { name: "登录", exact: true }).click();
  await poll(async () => (await p.locator(".banner", { hasText: "登录已过期" }).count()) === 0, "横幅消失");
  await p.getByRole("link", { name: "网盘", exact: true }).click();
  await grid(p).waitFor();
}, { faults: { auth_expired: true } });

await test("自动同步：首次同步无删除 → 执行；含删除需勾选确认；冲突阻止执行", async (p) => {
  await p.getByRole("button", { name: /课程资料备份/ }).click();
  await p.getByRole("button", { name: "扫描并生成计划" }).click();
  await p.getByRole("button", { name: "执行计划…" }).waitFor({ timeout: 8000 });
  assert((await p.locator(".plan-action.destructive").count()) === 0, "首次同步不应含删除");
  await p.getByRole("button", { name: "执行计划…" }).click();
  await dialog(p).getByRole("button", { name: "执行计划" }).click();
  await p.getByText("已执行。如需再次同步，请重新扫描。").waitFor({ timeout: 8000 });

  await p.getByRole("button", { name: /影视 云端/ }).click();
  await p.getByRole("button", { name: "扫描并生成计划" }).click();
  await p.getByRole("button", { name: "检查并执行…" }).waitFor({ timeout: 8000 });
  assert((await p.locator(".plan-action.destructive").count()) === 2, "应列出 2 个删除");
  await p.getByRole("button", { name: "检查并执行…" }).click();
  const go = dialog(p).getByRole("button", { name: "执行并删除 2 项" });
  assert((await go.getAttribute("aria-disabled")) === "true", "未确认删除前应禁用");
  await shot(p, "10-sync-delete-confirm.png");
  await dialog(p).getByRole("checkbox").check();
  await go.click();
  await p.getByText("已执行。如需再次同步，请重新扫描。").waitFor({ timeout: 8000 });

  await p.getByRole("button", { name: /工作区备份/ }).click();
  await p.getByRole("button", { name: "扫描并生成计划" }).click();
  const apply = p.getByRole("button", { name: "执行计划…" });
  await apply.waitFor({ timeout: 8000 });
  assert((await apply.getAttribute("aria-disabled")) === "true", "冲突未解决时应禁用执行");
}, { hash: "#/sync" });

await test("挂载：同步根与 P: 分开展示，前置检查未通过时禁用", async (p) => {
  await p.getByRole("heading", { name: "同步文件夹（按需文件）" }).waitFor();
  await p.getByRole("heading", { name: "虚拟盘符 P:" }).waitFor();
  for (const name of ["只读挂载为 P:", "创建同步文件夹"]) {
    assert((await p.getByRole("button", { name }).getAttribute("aria-disabled")) === "true", `${name} 应禁用`);
  }
  assert((await p.locator(".check-fail").count()) >= 4, "应列出未通过的检查");
  assert((await p.getByText("安装 WinFsp 2.x 后重新检测").count()) > 0, "应说明需要先安装 WinFsp");
  assert((await p.getByText(/CLI|路线图|假状态|mock/).count()) === 0, "挂载页不应出现内部说辞");
  await shot(p, "11-mount.png");
}, { hash: "#/mount" });

await test("设置：主题切换、网络默认跟随系统、没有后端页", async (p) => {
  await p.getByRole("radio", { name: "深色" }).click();
  await poll(async () => (await p.evaluate(() => document.documentElement.dataset.theme)) === "dark", "切换深色");
  await p.getByRole("link", { name: "网络", exact: true }).click();
  assert((await p.getByRole("radio", { name: "跟随系统" }).getAttribute("aria-checked")) === "true", "代理默认跟随系统");
  assert((await p.getByRole("radio", { name: "自定义" }).getAttribute("aria-disabled")) === "true", "自定义代理暂不支持");
  assert((await p.getByRole("switch", { name: "连接诊断日志" }).getAttribute("aria-checked")) === "false", "诊断日志默认关闭");
  await shot(p, "20-settings-network-dark.png");
  assert((await p.locator(".settings-link", { hasText: /后端|连接/ }).count()) === 0, "不应有后端/连接设置页");
  await p.getByRole("link", { name: "配额", exact: true }).click();
  await p.locator(".quota-row").first().waitFor();
  await p.getByRole("radio", { name: "浅色" }).count();
}, { hash: "#/settings" });

await test("关于：一句话说明与来源清单", async (p) => {
  await p.getByText("与 PikPak 官方无关").first().waitFor();
  for (const n of ["Piko", "pikpaktui", "pikpakcli", "Enhancement Master", "Rclone", "WinFsp", "Preact"]) {
    assert((await p.locator(".data-table", { hasText: n }).count()) > 0, `缺少来源：${n}`);
  }
  await shot(p, "16-about.png");
}, { hash: "#/about" });

await test("Tab 键可遍历主要控件", async (p) => {
  await grid(p).waitFor();
  await p.locator(".skip-link").focus();
  const seen = [];
  for (let i = 0; i < 40; i++) {
    await p.keyboard.press("Tab");
    seen.push(await p.evaluate(() => {
      const el = document.activeElement;
      return `${el?.tagName}|${el?.getAttribute("role") ?? ""}|${el?.getAttribute("aria-label") ?? el?.textContent?.trim().slice(0, 12) ?? ""}`;
    }));
  }
  const joined = seen.join("\n");
  for (const need of ["搜索", "网盘", "新建", "|grid|"]) assert(joined.includes(need), `Tab 未到达：${need}\n${joined}`);
});


/* ---------------- v3 features ---------------- */

await test("侧栏：下载是独立按钮，显示速度与进行中数量", async (p) => {
  await grid(p).waitFor();
  assert((await p.locator(".mode-switch, .mode-btn").count()) === 0, "不应再有“文件 | 任务”连体切换");
  const dl = tasksBtn(p);
  await poll(async () => /下载/.test(await dl.innerText()) && (await dl.locator(".dl-count").count()) === 1, "下载按钮应显示数量");
  await dl.click();
  await poll(async () => (await dl.getAttribute("aria-current")) === "page", "进入下载页后按钮高亮");
  await p.getByRole("link", { name: "网盘", exact: true }).click();
  await grid(p).waitFor();
});

await test("片段下载：起止点、关键帧对齐、画质、离开时确认", async (p) => {
  await grid(p).waitFor();
  await row(p, "片段.mp4").click({ button: "right" });
  await p.getByRole("menuitem", { name: "下载指定段落…" }).click();
  const side = sideIsland(p);
  await side.getByText("全长").waitFor();
  await side.locator(".clip-preview img").waitFor();
  await side.getByRole("radio", { name: "终点" }).click();
  const time = side.getByLabel("终点时间");
  await time.fill("16:59");
  await time.press("Enter");
  await side.getByRole("radio", { name: "起点" }).click();
  await side.getByLabel("起点时间").fill("6:04");
  await side.getByLabel("起点时间").press("Enter");
  await poll(async () => (await side.locator(".clip-scale").innerText()).includes("06:04 至 16:59"), "范围应为 06:04 至 16:59");
  await side.getByRole("slider", { name: "片段起点" }).focus();
  await p.keyboard.press("ArrowRight");
  await poll(async () => (await side.locator(".clip-scale").innerText()).includes("06:05 至 16:59"), "方向键微调 1 秒");
  await side.getByRole("button", { name: "后退 1 秒" }).click();
  await poll(async () => (await side.locator(".clip-scale").innerText()).includes("06:04 至"), "−1 按钮");
  await poll(async () => /起点对齐到前一个关键帧 06:0[0-4]\./.test(await side.locator(".clip-sum").innerText()), "应显示实际对齐的关键帧");
  await side.getByRole("radio", { name: "720P" }).click();
  await poll(async () => (await side.locator(".clip-sum").innerText()).includes("保存为 MP4"), "转码档保存为 MP4");
  await shot(p, "21-clip-download.png");
  // Closing with edits asks first.
  await side.getByRole("button", { name: "关闭面板（Esc）" }).click();
  await dialog(p).getByText("放弃下载片段？").waitFor();
  await dialog(p).getByRole("button", { name: "取消" }).click();
  await side.getByRole("button", { name: "下载片段" }).click();
  await toastText(p, "已加入片段下载");
  await tasksBtn(p).click();
  const t = p.locator(".task", { hasText: "片段 [720P]" }).first();
  await t.waitFor();
  assert(/06:0\d–16:59/.test(await t.locator(".task-tag").first().innerText()), "任务应标出片段范围");
});

await test("下载指定画质：不可读档位禁用，记住默认画质", async (p) => {
  await grid(p).waitFor();
  await row(p, "4K 演示").click({ button: "right" });
  await p.getByRole("menuitem", { name: "下载指定画质…" }).click();
  await dialog(p).locator(".radio-card", { hasText: "1080P" }).waitFor();
  assert(await dialog(p).locator(".radio-card.off", { hasText: "1080P" }).count() === 1, "1080P 不可读时应禁用");
  await dialog(p).locator(".radio-card", { hasText: "720P" }).click();
  await dialog(p).getByText("以后按此画质直接下载").click();
  await shot(p, "22-quality-dialog.png");
  await dialog(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "以后按此画质直接下载");
  await p.getByRole("button", { name: "设置（Ctrl+,）" }).click();
  await p.getByRole("link", { name: "下载与传输", exact: true }).click();
  await poll(async () => (await p.locator("#dl-q").inputValue()) === "720p", "默认画质应变为 720P");
}, { hash: "#/files/d-movies" });

await test("文件夹下载：展开为多个任务", async (p) => {
  await grid(p).waitFor();
  await row(p, "2026 课程").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "含文件夹中的");
}, { hash: "#/files/d-docs" });

await test("星标：Ctrl+D、星标页、Ctrl+Z 撤销", async (p) => {
  await grid(p).waitFor();
  await reveal(p, "Report.pdf");
  await row(p, "Report.pdf").click();
  await grid(p).focus();
  await p.keyboard.press("Control+d");
  await toastText(p, "已添加星标");
  await row(p, "Report.pdf").locator(".star-mark").waitFor();
  await p.getByRole("link", { name: "星标", exact: true }).click();
  await row(p, "Report.pdf").waitFor();
  await shot(p, "32-starred.png");
  await p.keyboard.press("Control+z");
  await toastText(p, "已撤销");
  await poll(async () => (await p.locator(".ft-row", { hasText: "Report.pdf" }).count()) === 0, "撤销后从星标页消失");
});

await test("命令面板：Ctrl+K 跳转页面与执行命令", async (p) => {
  await grid(p).waitFor();
  await p.keyboard.press("Control+k");
  await p.locator(".palette input:focus").waitFor();
  await p.keyboard.insertText("回收");
  await p.locator(".palette-item.on", { hasText: "回收站" }).waitFor();
  await p.keyboard.press("Enter");
  await p.getByRole("heading", { name: "回收站" }).waitFor();
  await p.getByRole("link", { name: "网盘", exact: true }).click();
  await grid(p).waitFor();
  await p.keyboard.press("Control+k");
  await p.locator(".palette input:focus").waitFor();
  await p.keyboard.insertText("xinjian");
  await shot(p, "27-command-palette.png");
  await p.keyboard.press("Enter");
  await p.locator("#name-input").waitFor();
  await p.keyboard.press("Escape");
});

await test("批量重命名：预览、冲突阻止、执行与撤销", async (p) => {
  await grid(p).waitFor();
  await grid(p).focus();
  await p.keyboard.press("Control+a");
  await p.keyboard.press("F2");
  await dialog(p).getByText("批量重命名").first().waitFor();
  await dialog(p).getByLabel("查找").fill("\\d");
  await dialog(p).getByRole("switch", { name: "正则表达式" }).click();
  await dialog(p).locator(".br-err").first().waitFor();
  const go = dialog(p).getByRole("button", { name: /重命名/ }).last();
  assert((await go.getAttribute("aria-disabled")) === "true", "有冲突时应禁用");
  await dialog(p).getByLabel("查找").fill("讲");
  await dialog(p).getByLabel("替换为").fill("课");
  await dialog(p).getByRole("radio", { name: "加在前面" }).click();
  await poll(async () => (await dialog(p).locator(".br-row.st-ok").count()) >= 2, "预览应显示改名项");
  await shot(p, "23-batch-rename.png");
  await go.click();
  await toastText(p, "已重命名");
  await row(p, "第1课").waitFor();
  await p.keyboard.press("Control+z");
  await toastText(p, "已撤销批量重命名");
  await row(p, "第1讲").waitFor();
}, { hash: "#/files/d-courses" });

await test("查找重复：扫描、建议保留最早一份、移入回收站可撤销", async (p) => {
  await grid(p).waitFor();
  await cmds(p).getByRole("button", { name: "整理" }).click();
  await p.getByRole("menuitem", { name: "在此文件夹中查找重复" }).click();
  await p.locator(".dup-group").first().waitFor({ timeout: 8000 });
  assert((await p.locator(".dup-group").count()) >= 5, "应找到至少 5 组重复");
  await p.getByRole("button", { name: "选中建议移走项" }).click();
  for (const g of await p.locator(".dup-group").all()) {
    assert((await g.locator(".dup-item:not(.on)").count()) >= 1, "每组至少保留一份");
  }
  await shot(p, "24-duplicates.png");
  await p.getByRole("button", { name: /移至回收站（\d+）/ }).click();
  await dialog(p).getByRole("button", { name: "移至回收站" }).click();
  await toastText(p, "个重复文件移至回收站");
  await poll(async () => (await p.getByText("没有发现重复文件").count()) === 1, "移走后不再有重复");
  await p.keyboard.press("Control+z");
  await toastText(p, "已还原");
});

await test("拖放移动与撤销；剪切粘贴", async (p) => {
  await grid(p).waitFor();
  const sourceId = await row(p, "空文件.txt").getAttribute("data-id");
  const targetId = await row(p, "空文件夹").getAttribute("data-id");
  // Keep both rows inside the viewport before Playwright computes drag endpoints.
  // Its two independent scrollIntoView calls otherwise invalidate the first row's
  // coordinates in a virtual list (observed on Windows Edge).
  await p.evaluate(([sourceId, targetId]) => {
    const source = document.querySelector(`[data-id="${sourceId}"]`);
    const target = document.querySelector(`[data-id="${targetId}"]`);
    const scroll = source.closest(".ft-scroll");
    const bottom = Math.max(source.getBoundingClientRect().bottom, target.getBoundingClientRect().bottom);
    scroll.scrollTop += Math.max(0, bottom - scroll.getBoundingClientRect().bottom + 12);
  }, [sourceId, targetId]);
  await p.waitForTimeout(120);
  await p.evaluate(() => {
    window.__e2eDragEvents = [];
    for (const type of ["dragstart", "drop"]) document.addEventListener(type, e => window.__e2eDragEvents.push({ type, id: e.target.closest("[data-id]")?.dataset.id, ctrl: e.ctrlKey }), true);
  });
  await row(p, "空文件.txt").dragTo(row(p, "空文件夹"));
  const dragEvents = await p.evaluate(() => window.__e2eDragEvents);
  assert(dragEvents.some(e => e.type === "dragstart" && e.id === sourceId), `没有拖动预期源：${JSON.stringify(dragEvents)}`);
  assert(dragEvents.some(e => e.type === "drop" && e.id === targetId), "没有放入预期目录");
  await toastText(p, "已移动");
  await poll(async () => (await p.locator(".ft-row", { hasText: "空文件.txt" }).count()) === 0, "拖放后离开当前文件夹");
  await p.keyboard.press("Control+z");
  await toastText(p, "已撤销移动");
  await row(p, "空文件.txt").waitFor();
  await reveal(p, "Report.pdf");
  await row(p, "Report.pdf").click();
  await grid(p).focus();
  await p.keyboard.press("Control+x");
  await toastText(p, "已剪切 1 项");
  assert((await row(p, "Report.pdf").getAttribute("class")).includes("is-cut"), "剪切项应变淡");
  await reveal(p, "空文件夹");
  await row(p, "空文件夹").dblclick();
  await cmds(p).getByRole("button", { name: "粘贴 1 项" }).click();
  await toastText(p, "已移动");
  await row(p, "Report.pdf").waitFor();
});

await test("框选与 Ctrl+滚轮缩放", async (p) => {
  await grid(p).waitFor();
  const box = await p.locator(".ft-scroll").boundingBox();
  const last = await p.locator(".ft-row").last().boundingBox();
  await p.mouse.move(box.x + box.width / 2, last.y + last.height + 30);
  await p.mouse.down();
  await p.mouse.move(box.x + box.width / 2, box.y + 60, { steps: 8 });
  await p.mouse.up();
  await poll(async () => /已选 [4-9] 项/.test(await p.locator(".sel-count").innerText()), "框选应选中多项");
  await p.keyboard.press("Escape");
  await p.mouse.move(box.x + box.width / 2, box.y + 120);
  await p.keyboard.down("Control");
  await p.mouse.wheel(0, -120);
  await p.keyboard.up("Control");
  await p.locator(".ft-grid").waitFor();
}, { hash: "#/files/d-docs" });

await test("空间分布：方块图与下钻", async (p) => {
  await p.locator(".tm-tile").first().waitFor();
  assert((await p.locator(".tm-tile").count()) >= 3, "应有多个方块");
  await shot(p, "25-space.png");
  await p.locator(".tm-tile.kind-folder").first().click();
  await poll(async () => (await p.locator(".mini-crumbs").innerText()).includes("我的资料"), "下钻后显示位置");
}, { hash: "#/space/d-docs" });

await test("压缩包：浏览、密码、云端解压完成提示", async (p) => {
  await grid(p).waitFor();
  await row(p, "C 语言程序设计 习题集.zip").dblclick();
  await p.locator(".arc-row", { hasText: "第1章 基础" }).waitFor();
  await p.getByRole("button", { name: "第1章 基础" }).click();
  await poll(async () => (await p.locator(".arc-row:not(.arc-head)").count()) === 8, "进入压缩包内文件夹");
  await shot(p, "26-archive.png");
  await p.getByRole("button", { name: "全部解压" }).click();
  await dialog(p).getByRole("button", { name: "解压到这里" }).click();
  await toastText(p, "已开始在云端解压");
  await toastText(p, "已完成");
  await p.goto(`${BASE}/?scenario=free#/files/d-projects`);
  await row(p, "超大压缩包.zip").dblclick();
  await p.getByRole("heading", { name: "需要解压密码" }).waitFor();
  const lock = p.locator(".arc-lock");
  await lock.getByLabel("密码").fill("nope");
  await lock.getByRole("button", { name: "打开" }).click();
  await lock.getByRole("alert").filter({ hasText: "密码不正确" }).waitFor();
  await lock.getByLabel("密码").fill("nekopak");
  await lock.getByRole("button", { name: "打开" }).click();
  await p.locator(".arc-row", { hasText: "数据集" }).waitFor();
}, { hash: "#/files/d-docs" });

await test("分享：创建链接、我的分享、取消分享", async (p) => {
  await grid(p).waitFor();
  await row(p, "片段.mp4").click();
  await cmds(p).getByRole("button", { name: "分享…" }).click();
  await dialog(p).getByRole("button", { name: "创建分享链接" }).click();
  await dialog(p).getByText("已创建分享").waitFor();
  await shot(p, "28-share.png");
  await dialog(p).getByRole("button", { name: "完成" }).click();
  await p.getByRole("link", { name: "我的分享", exact: true }).click();
  const r = p.locator(".share-row", { hasText: "片段.mp4" }).first();
  await r.waitFor();
  await r.getByRole("button", { name: "更多操作" }).click();
  await p.getByRole("menuitem", { name: "取消分享" }).click();
  await dialog(p).getByRole("button", { name: "取消分享" }).click();
  await toastText(p, "已取消分享");
  await poll(async () => (await r.innerText()).includes("已取消"), "状态变为已取消");
});

await test("添加链接：预览资源并只保存所选文件", async (p) => {
  await grid(p).waitFor();
  await cmds(p).getByRole("button", { name: "添加链接" }).click();
  await p.locator("#cloud-src").fill("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Frieren");
  await sideIsland(p).locator(".link-files li").first().waitFor();
  const head = sideIsland(p).locator(".link-pick-head");
  const before = await head.innerText();
  await sideIsland(p).getByRole("button", { name: /^字幕/ }).click();
  await poll(async () => (await head.innerText()) !== before, "按类型取消后已选数量变化");
  await shot(p, "29-add-link-preview.png");
  await sideIsland(p).getByRole("button", { name: "提交云下载" }).click();
  await toastText(p, "提交成功不代表下载完成");
  await tasksBtn(p).click();
  await p.locator(".task", { hasText: "Frieren（已选" }).first().waitFor();
});

await test("网格大卡片截图", async (p) => {
  await p.locator(".card").first().waitFor();
  await shot(p, "31-grid-large.png");
}, { hash: "#/files/d-photos", init: { view: "grid", gridSize: "l" } });

/* ---------------- scale / overflow / screenshots ---------------- */

const SCALES = [
  { label: "100%", viewport: { width: 1280, height: 720 }, dpr: 1 },
  { label: "125%", viewport: { width: 1024, height: 576 }, dpr: 1.25 },
  { label: "150%", viewport: { width: 853, height: 480 }, dpr: 1.5 },
  { label: "200%", viewport: { width: 640, height: 360 }, dpr: 2 },
];
const PAGES = ["#/files", "#/transfers", "#/cloud", "#/sync", "#/mount", "#/settings/quota", "#/settings/network", "#/trash", "#/about", "#/starred", "#/shares", "#/space/d-docs", "#/settings/transfers"];

for (const sc of SCALES) {
  for (const theme of ["light", "dark"]) {
    await test(`缩放 ${sc.label} ${theme === "dark" ? "深色" : "浅色"}：无横向溢出`, async (p) => {
      const bad = [];
      for (const h of PAGES) {
        await p.goto(`${BASE}/?scenario=free${h}`);
        await p.waitForSelector(".pf", { timeout: 8000 });
        await p.waitForTimeout(250);
        const o = await p.evaluate(() => {
          const out = [];
          const de = document.documentElement;
          if (de.scrollWidth > de.clientWidth + 1) out.push(`document ${de.scrollWidth}>${de.clientWidth}`);
          for (const sel of [".pf-caption", ".pf-commands", ".settings-section", ".sync-detail-inner", ".mount-grid", ".pf-foot"]) {
            for (const el of document.querySelectorAll(sel)) {
              if (el.scrollWidth > el.clientWidth + 1) out.push(`${sel} ${el.scrollWidth}>${el.clientWidth}`);
            }
          }
          return out;
        });
        if (o.length) bad.push(`${h}: ${o.join(", ")}`);
        if (h === "#/files") await shot(p, `scale-${sc.label.replace("%", "")}-${theme}-files.png`);
      }
      assert(bad.length === 0, bad.join(" | "));
    }, { viewport: sc.viewport, dpr: sc.dpr, theme });
  }
}

await test("截图：浅色/深色主页面", async (p) => {
  await grid(p).waitFor();
  await row(p, "片段.mp4").click();
  await p.mouse.move(5, 5);
  await shot(p, "02-files-light.png");
  await p.goto(`${BASE}/?scenario=free#/settings/quota`);
  await p.locator(".quota-row").first().waitFor();
  await shot(p, "12-settings-quota.png");
});

await test("截图：深色文件页与对比度抽查", async (p) => {
  await grid(p).waitFor();
  await shot(p, "03-files-dark.png");
  const low = await p.evaluate(() => {
    const lum = (c) => {
      const m = c.match(/\d+(\.\d+)?/g).map(Number);
      const f = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      return 0.2126 * f(m[0]) + 0.7152 * f(m[1]) + 0.0722 * f(m[2]);
    };
    const bg = (el) => {
      while (el) {
        const c = getComputedStyle(el).backgroundColor;
        if (c && !c.startsWith("rgba(0, 0, 0, 0)") && c !== "transparent") return c;
        el = el.parentElement;
      }
      return "rgb(0,0,0)";
    };
    const out = [];
    for (const el of document.querySelectorAll(".muted, .dim, .ft-cell, .nav-item, .field-hint, .crumb, .cmd-meta, .sb-section")) {
      if (!el.textContent?.trim()) continue;
      const a = lum(getComputedStyle(el).color);
      const b = lum(bg(el));
      const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
      if (ratio < 4.5) out.push(`${el.className} ${ratio.toFixed(2)}`);
    }
    return [...new Set(out)].slice(0, 8);
  });
  assert(low.length === 0, `低对比度文字：${low.join(", ")}`);
}, { theme: "dark" });

/* ---------------------------------------------------------------- */

await browser.close();
server?.kill();

const passed = results.filter((r) => r.ok).length;
const report = {
  base: BASE,
  filter: process.env.E2E_FILTER ?? null,
  ran_at: new Date().toISOString(),
  passed,
  failed: results.length - passed,
  external_requests: [...external],
  results,
};
writeFileSync(join(root, "docs", "evidence", process.env.E2E_FILTER ? "ui-v3-e2e-focused.json" : "ui-v3-e2e.json"), JSON.stringify(report, null, 2));
console.log(`\n${passed}/${results.length} passed; external requests: ${external.size}`);
if (external.size) console.log([...external].join("\n"));
process.exit(results.length > 0 && passed === results.length && external.size === 0 ? 0 : 1);
