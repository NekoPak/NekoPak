// Attach only to an explicitly launched isolated development WebView2. No browser-side Mock bridge.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';

const endpoint = process.env.NEKOPAK_TEST_CDP;
if (!endpoint || !/^http:\/\/127\.0\.0\.1:\d+$/.test(endpoint)) throw new Error('Set an explicit loopback NEKOPAK_TEST_CDP endpoint for the isolated dev app.');
const browser = await chromium.connectOverCDP(endpoint);
const pages = browser.contexts().flatMap(context => context.pages()).filter(page => page.url().startsWith('http://tauri.localhost'));
assert.equal(pages.length, 1, 'Expected one NekoPak WebView');
const page = pages[0];
page.setDefaultTimeout(10_000);
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const results = [];
mkdirSync('docs/screenshots', { recursive: true });
async function poll(fn, timeout = 10_000) { const until = Date.now() + timeout; do { if (await fn()) return; await new Promise(r => setTimeout(r, 80)); } while (Date.now() < until); throw new Error('condition timed out'); }
const wire = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const route = async hash => { await page.evaluate(hash => { location.hash = hash; }, hash); };
async function test(name, run) { await run(); results.push({ name, passed: true }); console.log('PASS ' + name); }

try {
  const status = await wire('app_status');
  const cli = status.mode === 'official_cli_preview';
  assert.ok(['mock', 'official_cli_preview'].includes(status.mode));
  await test('real Tauri transport and correct mode, no browser fallback', async () => {
    await poll(async () => (await page.locator('.banner').allInnerTexts()).some(text => text.includes(cli ? '官方 CLI 只读预览' : '离线界面预览')));
    assert.equal(await page.locator('.sb-flavor').isDisabled(), true);
    assert.equal(status.storage.status, 'ready');
    assert.equal(await page.locator('.mode-switch').count(), 0);
    assert.equal(await page.locator('.sidebar').getByRole('link', { name: '任务', exact: true }).count(), 1);
  });
  await test('first-run flow reflects account or offline simulation honestly', async () => {
    await page.evaluate(() => localStorage.removeItem('nekopak.disclaimer.v1'));
    await page.reload();
    await page.getByRole('button', { name: '我已了解，继续' }).click();
    if (cli) {
      await page.getByRole('button', { name: '稍后再说' }).click();
    } else {
      await page.getByRole('heading', { name: '离线演示已就绪' }).waitFor();
      await page.getByRole('button', { name: '进入云盘' }).click();
      assert.equal((await wire('auth_status')).authenticated, false);
    }
  });
  if (!cli) {
    await test('Rust directory navigation and breadcrumb ancestry', async () => {
      await page.locator('.ft-row[data-id="documents"]').dblclick();
      await page.locator('.ft-row[data-id="nested-file"]').waitFor();
      await poll(async () => (await page.locator('.crumbs').innerText()).includes('资料'));
      await page.getByRole('button', { name: '上一级（Backspace）' }).click();
      await page.locator('.ft-row[data-id="file-7"]').waitFor();
    });
    await test('wire u64 and disabled unsupported file operations', async () => {
      await page.locator('.ft-row[data-id="file-7"]').dblclick();
      await page.locator('.side-island').waitFor();
      await poll(async () => /18,446,744,073,709,551,615/.test(await page.locator('.side-island').innerText()));
      await page.getByRole('button', { name: '关闭面板（Esc）' }).click();
      await page.locator('.ft-row[data-id="file-1"]').click();
      assert.equal(await page.getByRole('button', { name: '重命名（F2）' }).getAttribute('aria-disabled'), 'true');
    });
    await test('UI creates persistent Rust task, pauses, reloads and resumes', async () => {
      const before = (await wire('transfers_list')).items.length;
      await page.locator('.pf-commands').getByRole('button', { name: '下载', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.length === before + 1);
      const task = (await wire('transfers_list')).items.at(-1);
      assert.equal(task.simulated, true);
      await route('#/transfers');
      const row = page.locator(`.task[data-task="${task.id}"]`);
      await row.getByRole('button', { name: '暂停', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.find(item => item.id === task.id).state === 'paused');
      await page.reload();
      await row.getByRole('button', { name: '继续', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.find(item => item.id === task.id).state === 'completed');
      await poll(async () => (await row.innerText()).includes('已完成'));
      await page.screenshot({ path: 'docs/screenshots/desktop-rust-transfers.png' });
    });
    await test('UI cloud submission reaches Rust queue', async () => {
      await route('#/files');
      await page.getByRole('button', { name: '添加链接', exact: true }).click();
      await page.locator('#cloud-src').fill('https://example.com/desktop-fixture.bin');
      await page.getByRole('button', { name: '提交云下载', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.some(item => item.kind === 'cloud_download'));
      await page.getByRole('button', { name: '关闭面板（Esc）' }).click();
    });
  } else {
    await test('logged-out CLI disables operations without fake files', async () => {
      assert.equal((await wire('auth_status')).authenticated, false);
      await page.locator('.banner').first().waitFor();
      assert.equal(await page.getByRole('button', { name: '添加链接', exact: true }).getAttribute('aria-disabled'), 'true');
      assert.equal(await page.locator('.ft-row').count(), 0);
    });
  }
  await test('sync, mount and backend settings remain explicitly disabled', async () => {
    await route('#/sync');
    assert.equal(await page.getByRole('button', { name: '新建同步' }).getAttribute('aria-disabled'), 'true');
    await route('#/mount');
    assert.equal(await page.getByRole('button', { name: '只读挂载为 P:' }).getAttribute('aria-disabled'), 'true');
    await route('#/settings/transfers');
    assert.equal(await page.getByRole('button', { name: '保存', exact: true }).getAttribute('aria-disabled'), 'true');
    await route('#/settings/account');
    assert.equal(await page.getByRole('button', { name: '在浏览器中授权' }).getAttribute('aria-disabled'), 'true');
    assert.equal(await page.getByRole('button', { name: '退出登录' }).getAttribute('aria-disabled'), 'true');
  });
  await test('theme preference persists locally without a settings backend', async () => {
    await route('#/settings/general');
    await page.getByRole('button', { name: '深色', exact: true }).click();
    await poll(async () => await page.locator('html').getAttribute('data-theme') === 'dark');
    await page.reload();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    await route('#/files');
    await page.locator('.banner').first().waitFor();
    await page.screenshot({ path: `docs/screenshots/desktop-${cli ? 'cli-unauthorized' : 'rust-files'}-dark.png` });
  });
  assert.deepEqual(errors, []);
  writeFileSync(`docs/evidence/desktop-${cli ? 'cli' : 'mock'}-ui-e2e.json`, JSON.stringify({ mode: status.mode, transport: 'real Tauri invoke/events in WebView2', results, errors, date: new Date().toISOString() }, null, 2));
} catch (error) {
  await page.screenshot({ path: 'docs/screenshots/desktop-failure.png' });
  console.error(await page.locator('body').innerText());
  throw error;
} finally { await browser.close(); }
