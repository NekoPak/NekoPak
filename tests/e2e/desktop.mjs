// Explicit isolated development WebView2 only. Never signs in or contacts a real account.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';

const endpoint = process.env.NEKOPAK_TEST_CDP;
if (!endpoint || !/^http:\/\/127\.0\.0\.1:\d+$/.test(endpoint)) throw new Error('Set an explicit loopback NEKOPAK_TEST_CDP for the isolated debug app.');
const browser = await chromium.connectOverCDP(endpoint);
const pages = browser.contexts().flatMap(context => context.pages()).filter(page => page.url().startsWith('http://tauri.localhost'));
assert.equal(pages.length, 1, 'Expected one NekoPak WebView');
const page = pages[0];
page.setDefaultTimeout(15_000);
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const results = [];
const shots = 'docs/screenshots/ui-v3';
mkdirSync(shots, { recursive: true });
async function poll(fn, timeout = 15_000) {
  const until = Date.now() + timeout;
  do { if (await fn()) return; await new Promise(r => setTimeout(r, 80)); } while (Date.now() < until);
  throw new Error('condition timed out');
}
const wire = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const route = hash => page.evaluate(hash => { location.hash = hash; }, hash);
async function test(name, run) { await run(); results.push({ name, passed: true }); console.log('PASS ' + name); }

try {
  const status = await wire('app_status');
  const native = status.mode === 'rust_compat_preview';
  assert.ok(['mock', 'rust_compat_preview'].includes(status.mode));
  // This harness must never run against an authorized account/profile.
  assert.equal((await wire('auth_status')).authenticated, false);
  await test('native wire contract and no retired backend', async () => {
    assert.equal(status.contract_version, '0.2');
    assert.equal(status.storage.status, 'ready');
    const backends = await wire('backend_discover');
    assert.equal(backends.length, 1);
    assert.equal(backends[0].backend, native ? 'rust_compat' : 'mock');
    assert.equal(status.features.real_transfers, native);
    await assert.rejects(wire('official_cli_probe'));
  });
  await page.locator('.app').waitFor();
  if (native) {
    await test('normal native build offers login without developer setup', async () => {
      assert.equal(status.features.auth_sign_in, true);
      const form = page.locator('.onboarding');
      await form.getByRole('heading', { name: '登录 PikPak' }).waitFor();
      assert.equal(await form.locator('input[name="username"]').isDisabled(), false);
      assert.equal(await form.locator('input[name="password"]').isDisabled(), false);
      assert.notEqual(await form.getByRole('button', { name: '登录', exact: true }).getAttribute('aria-disabled'), 'true');
      assert.ok(!(await form.innerText()).includes('CLI'));
      assert.ok(!(await form.innerText()).includes('尚不能连接'));
      // Empty submission validates locally; real account testing is left to the user.
      await form.getByRole('button', { name: '登录', exact: true }).click();
      await form.getByRole('alert').filter({ hasText: '请输入账号' }).waitFor();
      await page.screenshot({ path: `${shots}/desktop-native-sign-in.png` });
      await form.getByRole('button', { name: '先看看', exact: true }).click();
      assert.equal(await page.locator('.ft-row').count(), 0);
    });
  } else {
    await test('offline Rust data is labeled and remains distinct from browser demo', async () => {
      await poll(async () => !(await page.locator('.onboarding').count()));
      await page.locator('.ft-row[data-id="file-7"]').waitFor();
      assert.ok((await page.locator('.banner').allInnerTexts()).some(t => t.includes('离线界面预览')));
      assert.ok((await page.locator('.sidebar').innerText()).includes('未连接 PikPak'));
      assert.equal(await page.locator('.sb-flavor').count(), 0);
      assert.equal(await page.locator('.dl-btn').count(), 1);
    });
    await test('Rust directory ancestry and exact u64 metadata', async () => {
      await page.locator('.ft-row[data-id="documents"]').dblclick();
      await page.locator('.ft-row[data-id="nested-file"]').waitFor();
      await route('#/files');
      const entry = await wire('files_stat', { id: 'file-7' });
      assert.equal(entry.size_bytes, '18446744073709551615');
      await page.locator('.ft-row[data-id="file-7"]').dblclick();
      await poll(async () => /18,446,744,073,709,551,615/.test(await page.locator('.side-island').innerText()));
      await page.getByRole('button', { name: '关闭面板（Esc）' }).click();
    });
    await test('UI mock download creates a persistent Rust task and survives reload', async () => {
      await page.locator('.ft-row[data-id="file-1"]').click();
      const before = (await wire('transfers_list')).items.length;
      await page.locator('.pf-commands').getByRole('button', { name: '下载', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.length === before + 1);
      const task = (await wire('transfers_list')).items[0];
      assert.equal(task.simulated, true);
      await route('#/transfers');
      const row = page.locator(`.task[data-task="${task.id}"]`);
      await row.getByRole('button', { name: '暂停', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.find(t => t.id === task.id).state === 'paused');
      await page.reload();
      await route('#/transfers');
      await row.getByRole('button', { name: '继续', exact: true }).click();
      await poll(async () => (await wire('transfers_list')).items.find(t => t.id === task.id).state === 'completed');
    });
  }
  await test('native feature pages reflect implemented capabilities and network settings', async () => {
    await route('#/sync');
    if(native) await page.getByRole('heading', { name: '自动同步', exact: true }).waitFor();
    else assert.equal(await page.getByRole('button', { name: '新建同步' }).getAttribute('aria-disabled'), 'true');
    await route('#/settings/transfers');
    assert.notEqual(await page.getByRole('button', { name: '保存', exact: true }).getAttribute('aria-disabled'), 'true');
    await route('#/settings/network');
    await page.getByRole('radio', { name: '跟随系统', exact: true }).waitFor();
  });
  await test('local theme preferences persist without backend settings', async () => {
    await route('#/settings/general');
    await page.getByRole('radio', { name: '深色', exact: true }).click();
    await poll(async () => await page.locator('html').getAttribute('data-theme') === 'dark');
    await page.reload();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    const skip = page.getByRole('button', { name: '先看看', exact: true });
    if (await skip.isVisible()) await skip.click();
    await route('#/files');
    await page.screenshot({ path: `${shots}/desktop-${native ? 'native' : 'mock'}-dark.png` });
  });
  assert.deepEqual(errors, []);
  writeFileSync(`docs/evidence/desktop-${native ? 'native' : 'mock'}-v3.json`, JSON.stringify({ mode: status.mode, scope: 'Isolated real WebView2 with Rust IPC; no real account or service requests', results, errors, date: new Date().toISOString() }, null, 2));
} catch (error) {
  await page.screenshot({ path: `${shots}/desktop-failure.png` });
  throw error;
} finally { await browser.close(); }
