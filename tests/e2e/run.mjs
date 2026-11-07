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
const shots = join(root, "docs", "screenshots");
mkdirSync(shots, { recursive: true });
for (const f of readdirSync(shots)) if (f.startsWith("FAIL-")) rmSync(join(shots, f));
const PORT = 4179;
const BASE = process.env.E2E_BASE ?? `http://localhost:${PORT}`;

let server;
if (!process.env.E2E_BASE) {
  // Run vite's bin directly (not via npx) so kill() really stops the server.
  server = spawn(process.execPath, [join(root, "node_modules", "vite", "bin", "vite.js"), "preview", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
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
 * @param {{ scenario?: string, faults?: object, viewport?: {width:number,height:number}, dpr?: number, accepted?: boolean, theme?: string, hash?: string }} opts
 */
async function test(name, fn, opts = {}) {
  const ctx = await browser.newContext({
    viewport: opts.viewport ?? { width: 1280, height: 720 },
    deviceScaleFactor: opts.dpr ?? 1,
    locale: "zh-CN",
    colorScheme: opts.theme === "dark" ? "dark" : "light",
    permissions: ["clipboard-read", "clipboard-write"],
  });
  const faults = { latency_ms: 30, ...(opts.faults ?? {}) };
  await ctx.addInitScript(([accepted, faults]) => {
    try {
      if (!sessionStorage.getItem("e2e-init")) {
        sessionStorage.setItem("e2e-init", "1");
        if (accepted) localStorage.setItem("nekopak.disclaimer.v1", "1");
        localStorage.setItem("nekopak.mock.faults", JSON.stringify(faults));
      }
    } catch {}
  }, [opts.accepted !== false, faults]);
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
const row = (page, text) => page.locator(".ft-row", { hasText: text }).first();
const dialog = (page) => page.locator('[role="dialog"].modal');
const sideIsland = (page) => page.locator(".side-island");
const tasksBtn = (page) => page.getByRole("link", { name: "任务", exact: true });
const cmds = (page) => page.locator(".pf-commands");
const toastText = async (page, text) => poll(async () => (await page.locator(".toast").allInnerTexts()).some((t) => t.includes(text)), `等待提示“${text}”`);

console.log(`NekoPak UI E2E → ${BASE}`);

/* ---------------------------------------------------------------- */

await test("首次启动：非官方声明 → 已连接 → 进入云盘", async (p) => {
  await p.getByText("NekoPak 不是 PikPak 官方产品").waitFor();
  await shot(p, "01-onboarding.png");
  await p.getByRole("button", { name: "我已了解，继续" }).click();
  await p.getByRole("heading", { name: "已连接" }).waitFor();
  assert(await p.getByText("每日下行").count() > 0, "完成页应显示配额维度");
  await p.getByRole("button", { name: "进入云盘" }).click();
  await grid(p).waitFor();
  assert((await p.locator(".ft-row").count()) >= 20, "根目录应有文件");
}, { accepted: false });

await test("未授权：取消 / 网络失败 / 成功 三种授权结果", async (p) => {
  await p.getByRole("heading", { name: "连接你的账号" }).waitFor();
  assert(await p.locator(".backend-opt.off").count() >= 3, "未安装/不可用后端应禁用");
  const start = () => p.getByRole("button", { name: "在浏览器中授权" }).click();
  await start();
  await p.getByText("等待你在浏览器中完成授权").waitFor();
  await p.getByRole("button", { name: "模拟用户取消" }).click();
  await p.getByText("授权已取消").waitFor();
  await start();
  await p.getByRole("button", { name: "模拟网络失败" }).click();
  await p.locator('[data-error-code="TRANSIENT_NETWORK"]').waitFor();
  await start();
  await p.getByRole("button", { name: "模拟授权成功" }).click();
  await p.getByRole("heading", { name: "已连接" }).waitFor();
}, { scenario: "unauthorized" });

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
  assert((await p.locator('.menu-qbtn[aria-label^="下载"]').count()) === 1 && items.some((t) => t.includes("用外部播放器打开")), "菜单项缺失");
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
  // Virtual rows outside the viewport do not exist in the DOM; scroll until both are mounted.
  await poll(async () => {
    if (await row(p, "Report.pdf").count()) return true;
    await grid(p).evaluate(el => { el.scrollTop += 180; });
    return false;
  }, "滚动到 Report.pdf");
  await row(p, "Report.pdf").click();
  await cmds(p).getByRole("button", { name: "下载", exact: true }).click();
  await toastText(p, "已加入传输队列 1 项");
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
  await poll(async () => /已丢弃 [\d,]+ 条/.test(await p.locator(".cmd-meta").first().innerText()), "应统计被丢弃的过期事件", 8000, 250);
  await shot(p, "07-transfers-active.png");
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
  await ta.fill("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=E2E%20Test%20ISO");
  await shot(p, "09-cloud-dialog.png");
  await sideIsland(p).getByRole("button", { name: "提交云下载" }).click();
  await toastText(p, "提交成功不代表下载完成");
  await tasksBtn(p).click();
  await p.getByRole("tab", { name: /云下载/ }).click();
  const t = p.locator(".task", { hasText: "E2E Test ISO" });
  await t.waitFor();
  assert((await t.innerText()).includes("总大小未提供"), "元数据解析前应显示“总大小未提供”");
  assert((await t.locator(".lp-unknown .lp-fill").count()) === 0, "总大小未知时进度条不得有填充");
});

await test("限额耗尽：横幅、禁用下载与云下载、列表仍可浏览", async (p) => {
  await p.getByText("Connected Apps 配额已用尽").waitFor();
  await grid(p).waitFor();
  await row(p, "片段.mp4").click();
  const dl = cmds(p).getByRole("button", { name: "下载", exact: true });
  assert((await dl.getAttribute("aria-disabled")) === "true", "下载应禁用");
  assert(((await dl.getAttribute("title")) ?? "").includes("配额已用尽"), "下载按钮应说明原因");
  assert((await p.getByText("切换后端重置").count()) === 0, "不得提供切换后端重置额度");
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
  await p.getByText("登录已过期。文件列表与任务历史已保留").waitFor();
  await p.getByRole("heading", { name: "登录已过期" }).waitFor();
  await shot(p, "15-auth-expired.png");
  await tasksBtn(p).click();
  await p.locator('.task[data-task="task-9"]').waitFor();
  await p.goto(`${BASE}/?scenario=free#/settings/account`);
  await p.getByRole("button", { name: "在浏览器中授权" }).click();
  await p.getByRole("button", { name: "模拟授权成功" }).click();
  await poll(async () => (await p.getByText("登录已过期。文件列表与任务历史已保留").count()) === 0, "横幅消失");
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
  assert((await p.getByText("NekoPak 不会自动安装驱动").count()) > 0, "应说明不自动安装驱动");
  await shot(p, "11-mount.png");
}, { hash: "#/mount" });

await test("设置：主题切换、高级网络默认关闭且需确认、后端能力", async (p) => {
  await p.getByRole("radio", { name: "深色" }).click();
  await poll(async () => (await p.evaluate(() => document.documentElement.dataset.theme)) === "dark", "切换深色");
  await p.getByRole("link", { name: "网络", exact: true }).click();
  const sw = p.getByRole("switch", { name: "启用高级网络设置" });
  assert((await sw.getAttribute("aria-checked")) === "false", "默认应关闭");
  await sw.click();
  await dialog(p).getByText("不会修改系统 Hosts").waitFor();
  await dialog(p).getByRole("button", { name: "取消" }).click();
  assert((await sw.getAttribute("aria-checked")) === "false", "取消后仍关闭");
  assert((await p.getByRole("switch", { name: "自定义 DNS" }).getAttribute("aria-disabled")) === "true", "自定义 DNS 应禁用");
  await shot(p, "20-settings-network-dark.png");
  await p.getByRole("link", { name: "连接", exact: true }).click();
  assert((await p.locator(".backend-card").count()) === 4, "连接页应列出真实连接候选，Mock 不应混入");
  assert(!(await p.locator(".backend-list").innerText()).includes("OFFLINE SIMULATION"), "产品连接页不应直接显示调试文本");
  await p.getByRole("link", { name: "配额", exact: true }).click();
  await p.locator(".quota-row").first().waitFor();
  await p.getByRole("radio", { name: "浅色" }).count();
}, { hash: "#/settings" });

await test("关于：非官方声明与来源清单", async (p) => {
  await p.getByText("不是 PikPak 官方产品").first().waitFor();
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

/* ---------------- scale / overflow / screenshots ---------------- */

const SCALES = [
  { label: "100%", viewport: { width: 1280, height: 720 }, dpr: 1 },
  { label: "125%", viewport: { width: 1024, height: 576 }, dpr: 1.25 },
  { label: "150%", viewport: { width: 853, height: 480 }, dpr: 1.5 },
  { label: "200%", viewport: { width: 640, height: 360 }, dpr: 2 },
];
const PAGES = ["#/files", "#/transfers", "#/cloud", "#/sync", "#/mount", "#/settings/quota", "#/settings/network", "#/trash", "#/about"];

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
  ran_at: new Date().toISOString(),
  passed,
  failed: results.length - passed,
  external_requests: [...external],
  results,
};
writeFileSync(join(root, "docs", "e2e-report.json"), JSON.stringify(report, null, 2));
console.log(`\n${passed}/${results.length} passed; external requests: ${external.size}`);
if (external.size) console.log([...external].join("\n"));
process.exit(passed === results.length && external.size === 0 ? 0 : 1);
