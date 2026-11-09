#!/usr/bin/env node
// Browser IPC fixtures validate mobile UI only, never Android storage or live accounts.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright";

const root = process.cwd(), output = resolve(root, "target/mobile-ui");
mkdirSync(output, { recursive: true });
const port = 4183, base = process.env.E2E_BASE ?? `http://localhost:${port}`;
let server;
if (!process.env.E2E_BASE) {
  server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "preview", "--port", String(port), "--strictPort"], { cwd: root, stdio: "pipe", windowsHide: true });
  await new Promise((ok, fail) => {
    const timeout = setTimeout(() => fail(new Error("Vite preview did not start")), 20000);
    server.stdout.on("data", chunk => { if (String(chunk).includes(String(port))) { clearTimeout(timeout); ok(); } });
    server.on("exit", code => { clearTimeout(timeout); fail(new Error(`Vite exited: ${code}`)); });
  });
}
const browser = await chromium.launch({ channel: process.env.E2E_BROWSER_CHANNEL ?? (process.platform === "win32" ? "msedge" : undefined) });
const results = [], errors = [];

function fixture({ signedIn, mobile = true }) {
  localStorage.setItem("nekopak.disclaimer.v1", "1");
  localStorage.setItem("nekopak.language", "zh-CN");
  let signed = signedIn, id = 0;
  const callbacks = new Map();
  const now = "2026-11-09T08:00:00Z";
  const folder = { id: "folder-a", parent_id: "root", name: "手机测试文件夹", kind: "folder", size_bytes: "0" };
  const file = { id: "file-a", parent_id: "root", name: "示例文件.txt", kind: "file", size_bytes: "12" };
  const task = { id: "completed-a", remote_id: "file-a", backend: "rust_compat", name: file.name, kind: "download", state: "completed", destination: "/data/user/0/io.nekopak.desktop/files/Downloads/示例文件.txt", transferred_bytes: "12", total_bytes: "12", revision: "1", simulated: false, created_at: now, updated_at: now };
  const settings = { schema_version: 1, theme: "light", language: "zh-CN", download_dir: "/data/user/0/io.nekopak.desktop/files/Downloads", max_concurrent_transfers: 1, bandwidth_limit_kib_s: 0, ask_download_directory: false, close_to_tray: false, cache: { dir: "", max_bytes: "536870912" }, experimental_network: { enabled: false, proxy_mode: "system", user_proxy_configured: false, custom_dns_mode: false, connection_diagnostics: false }, telemetry: false, download: { default_quality: "original", connections_per_task: 1 }, media: { player_path: "" } };
  window.__mobileFixture = { calls: [], exportResult: false };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: callback => { callbacks.set(++id, callback); return id; },
    unregisterCallback: key => callbacks.delete(key),
    invoke: async (command, args = {}) => {
      // Never retain even synthetic password values in fixture diagnostics.
      window.__mobileFixture.calls.push({ command, args: command === "auth_sign_in" ? {} : structuredClone(args) });
      if (command === "plugin:event|listen") return ++id;
      if (command === "plugin:event|unlisten") return;
      if (command.startsWith("plugin:window|")) return false;
      if (command === "app_status") return { version: "mobile-fixture", contract_version: "0.2", mode: "rust_compat_preview", storage: { status: "ready", schema_version: 1 }, features: { mobile, desktop_window: !mobile, tray: !mobile, directory_picker: !mobile, player_config: !mobile, media_external: !mobile, transfer_open_directory: !mobile, transfer_export: mobile, background_transfers: !mobile, auth_sign_in: true, auth_logout: true, settings: true, ui_prefs: true, media_session: true, upload: !mobile, upload_directory: !mobile, local_sources: false, sync: false, mount: false, autostart: false } };
      if (command === "auth_status") return { backend: "rust_compat", authenticated: signed, state: signed ? "signed_in" : "signed_out", account_id: signed ? "synthetic-account" : undefined, display_name: "fixture@example.test", credential_owner: "os_vault", account_tier: "unknown", checked_at: now };
      if (command === "auth_sign_in") {
        if (args.password === "synthetic-rejected") throw { code: "PERMISSION_DENIED", message: "测试凭据未通过", retryable: false, details: { reason: "invalid_credentials" } };
        signed = true; return { state: "signed_in" };
      }
      if (command === "backend_discover") return [{ backend: "rust_compat", status: signed ? "ready" : "needs_login", operations: { list: true, stat: true, search: true, mkdir: true, rename: true, upload: true, download: true, cloud_task: true, trash: true, move: true, copy: true, star: true, external_player: true }, limitations: [], checked_at: now }];
      if (command === "settings_get") return settings;
      if (command === "settings_update") return Object.assign(settings, args.patch);
      if (command === "ui_prefs_get") return { account_id: "synthetic-account", prefs: { pins: [] } };
      if (command === "ui_prefs_update") return args.prefs;
      if (command === "quota_get") return { backend: "rust_compat", source: "account", account_tier: "unknown", dimensions: [], limitations: [], checked_at: now };
      if (command === "transfers_list") return { items: signed ? [task] : [] };
      if (command === "jobs_list" || command === "shares_list") return { items: [] };
      if (command === "files_list") return { items: args.parent_id === "root" ? [folder, file] : [{ ...file, id: "nested", parent_id: folder.id }] };
      if (command === "files_path") return args.id === folder.id ? [folder] : [];
      if (command === "files_stat") return args.id === folder.id ? folder : file;
      if (command === "transfer_export") return window.__mobileFixture.exportResult;
      throw { code: "UNSUPPORTED", message: `Mobile fixture: ${command}`, retryable: false };
    },
  };
}

async function assertFits(page, name) {
  const overflow = await page.evaluate(() => {
    const selectors = ["html", "body", ".workspace", ".pf-body", ".canvas", ".settings-layout", ".settings-body", ".ob-island", ".ft-scroll", ".task-scroll"];
    return selectors.flatMap(selector => [...document.querySelectorAll(selector)].filter(el => el.getClientRects().length && el.scrollWidth > el.clientWidth + 2).map(el => ({ selector, width: el.clientWidth, content: el.scrollWidth })));
  });
  assert.deepEqual(overflow, [], `${name} has horizontal content overflow`);
}

async function context(width, signedIn = true, mobile = true) {
  const ctx = await browser.newContext({ viewport: { width, height: 844 }, hasTouch: true, isMobile: true, locale: "zh-CN" });
  await ctx.addInitScript(fixture, { signedIn, mobile });
  const page = await ctx.newPage();
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/*", route => {
    const url = route.request().url();
    if (url.startsWith(base) || /^(data|blob):/.test(url)) return route.continue();
    errors.push(`Unexpected external request: ${url}`); return route.abort();
  });
  await page.goto(base);
  return { ctx, page };
}

try {
  for (const width of [360, 390, 412]) {
    const { ctx, page } = await context(width);
    try {
      const row = page.locator('[data-id="folder-a"]');
      await row.waitFor();
      assert.equal(await page.locator(".win-controls").count(), 0);
      await assertFits(page, `${width} files`);
      // A focused selection button owns Enter/Space. Grid keyboard navigation
      // must not open the focused folder or suppress the button's native click.
      const folderCheck = row.getByRole("button", { name: "选择 手机测试文件夹", exact: true });
      await folderCheck.focus();
      await page.keyboard.press("Enter");
      assert.equal(await folderCheck.getAttribute("aria-pressed"), "true");
      assert.match(page.url(), /#\/files$/);
      await page.keyboard.press("Space");
      assert.equal(await folderCheck.getAttribute("aria-pressed"), "false");
      assert.match(page.url(), /#\/files$/);
      await page.getByRole("button", { name: "网格视图", exact: true }).tap();
      const cardCheck = page.locator('[data-id="folder-a"] .card-check');
      await cardCheck.focus();
      await page.keyboard.press("Enter");
      assert.equal(await cardCheck.getAttribute("aria-pressed"), "true");
      await page.keyboard.press("Space");
      assert.equal(await cardCheck.getAttribute("aria-pressed"), "false");
      assert.match(page.url(), /#\/files$/);
      await page.getByRole("button", { name: "列表视图", exact: true }).tap();
      // Explicit selection never opens the folder; a tap on its name does.
      await row.getByRole("button", { name: "选择 手机测试文件夹", exact: true }).tap();
      assert.equal(await row.getAttribute("aria-selected"), "true");
      assert.match(page.url(), /#\/files$/);
      await row.getByRole("button", { name: "选择 手机测试文件夹", exact: true }).tap();
      await row.locator(".ft-name").tap();
      await page.waitForURL("**/#/files/folder-a");
      await page.locator('[data-id="nested"]').waitFor();
      assert.equal(await page.evaluate(() => !window.dispatchEvent(new CustomEvent("nekopak:back", { cancelable: true }))), true);
      await page.waitForURL("**/#/files");
      assert.equal(await page.evaluate(() => !window.dispatchEvent(new CustomEvent("nekopak:back", { cancelable: true }))), false, "root must permit native back");
      await page.getByRole("button", { name: "新建", exact: true }).tap();
      assert.equal(await page.getByRole("menuitem", { name: /上传文件\s+Android/ }).first().getAttribute("aria-disabled"), "true");
      await page.getByRole("menuitem", { name: /新建文件夹/ }).tap();
      await page.getByRole("dialog", { name: "新建文件夹", exact: true }).waitFor();
      assert.equal(await page.evaluate(() => !window.dispatchEvent(new CustomEvent("nekopak:back", { cancelable: true }))), true, "system back closes the dialog");
      await page.getByRole("dialog", { name: "新建文件夹", exact: true }).waitFor({ state: "hidden" });
      await page.getByRole("button", { name: "打开导航", exact: true }).tap();
      await page.evaluate(() => { document.querySelector('#app-navigation .sb-account button:last-child').focus(); });
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement?.classList.contains("mobile-nav-close")), true, "drawer traps forward focus");
      await page.keyboard.press("Shift+Tab");
      assert.equal(await page.evaluate(() => document.activeElement === document.querySelector('#app-navigation .sb-account button:last-child')), true, "drawer traps reverse focus");
      assert.equal(await page.evaluate(() => !window.dispatchEvent(new CustomEvent("nekopak:back", { cancelable: true }))), true, "system back closes navigation");
      await page.getByRole("button", { name: "打开导航", exact: true }).tap();
      await page.getByRole("button", { name: "设置（Ctrl+,）", exact: true }).tap();
      await page.locator('.settings-link[aria-current="page"]').waitFor();
      await assertFits(page, `${width} general settings`);
      assert.equal(await page.getByRole("switch", { name: "关闭窗口时最小化到托盘", exact: true }).getAttribute("aria-disabled"), "true");
      await page.getByRole("link", { name: "下载与传输", exact: true }).tap();
      await page.locator("#native-download-dir").waitFor();
      assert.equal(await page.getByRole("button", { name: "选择文件夹", exact: true }).getAttribute("aria-disabled"), "true");
      assert.equal(await page.getByRole("button", { name: "选择播放器", exact: true }).getAttribute("aria-disabled"), "true");
      await assertFits(page, `${width} transfer settings`);
      await page.getByRole("button", { name: "打开导航", exact: true }).tap();
      await page.locator(".dl-btn").tap();
      await page.locator('[data-task="completed-a"]').waitFor();
      await page.getByRole("button", { name: "更多操作", exact: true }).tap();
      await page.getByRole("menuitem", { name: "导出文件", exact: true }).tap();
      assert.equal(await page.getByText("文件已导出", { exact: true }).count(), 0, "cancel must not claim success");
      await page.evaluate(() => { window.__mobileFixture.exportResult = true; });
      await page.getByRole("button", { name: "更多操作", exact: true }).tap();
      await page.getByRole("menuitem", { name: "导出文件", exact: true }).tap();
      await page.getByText("文件已导出", { exact: true }).waitFor();
      const calls = await page.evaluate(() => window.__mobileFixture.calls);
      assert.equal(calls.filter(call => call.command === "transfer_export").length, 2);
      assert(!calls.some(call => ["download_directory_pick", "player_config_pick", "upload_pick_and_start"].includes(call.command)));
      await assertFits(page, `${width} transfers`);
      await page.screenshot({ path: resolve(output, `transfers-${width}.png`) });
      results.push({ name: `mobile IPC UI ${width}px`, passed: true });
    } catch (error) { await page.screenshot({ path: resolve(output, `failure-${width}.png`) }); throw error; }
    finally { await ctx.close(); }
  }
  const { ctx, page } = await context(360, false);
  try {
    await page.getByRole("heading", { name: "登录 PikPak", exact: true }).waitFor();
    await assertFits(page, "360px native sign-in");
    await page.locator('input[name="username"]').fill("synthetic@example.test");
    await page.locator('input[name="password"]').fill("synthetic-rejected");
    await page.getByRole("button", { name: "登录", exact: true }).tap();
    await page.getByText("测试凭据未通过", { exact: true }).waitFor();
    assert.equal(await page.locator('input[name="password"]').inputValue(), "");
    await page.screenshot({ path: resolve(output, "login-360.png") });
    await page.locator('input[name="password"]').fill("synthetic-accepted");
    await page.getByRole("button", { name: "登录", exact: true }).tap();
    await page.locator('[data-id="folder-a"]').waitFor();
    results.push({ name: "mobile sign-in fixture clears password and navigates", passed: true });
  } finally { await ctx.close(); }
  const desktop = await context(390, true, false);
  try {
    await desktop.page.locator('[data-id="folder-a"]').waitFor();
    await desktop.page.getByRole("button", { name: "打开导航", exact: true }).tap();
    await desktop.page.getByRole("button", { name: "设置（Ctrl+,）", exact: true }).tap();
    await desktop.page.getByRole("link", { name: "下载与传输", exact: true }).tap();
    await desktop.page.locator("#native-download-dir").waitFor();
    assert.equal(await desktop.page.getByRole("button", { name: "选择文件夹", exact: true }).getAttribute("aria-disabled"), null);
    assert.equal(await desktop.page.getByRole("button", { name: "选择播放器", exact: true }).getAttribute("aria-disabled"), null);
    assert.equal(await desktop.page.locator(".mobile-storage-note").count(), 0);
    results.push({ name: "narrow layout does not infer Android platform", passed: true });
  } finally { await desktop.ctx.close(); }
  assert.deepEqual(errors, []);
  writeFileSync(resolve(output, "results.json"), JSON.stringify({ scope: "Browser mobile IPC fixture, not device or live service acceptance", results, errors }, null, 2));
  console.log(JSON.stringify({ passed: results.length, errors, output }));
} finally { await browser.close(); server?.kill(); }
