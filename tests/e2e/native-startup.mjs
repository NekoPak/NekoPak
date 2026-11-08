// Isolated, hidden development app. Tests real Rust IPC + WebView2, never real login.
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const executable = join(root, 'target', 'debug', 'nekopak.exe');
if (!existsSync(executable)) throw new Error('Build the debug desktop app first.');
const data = join(root, 'target', `native-login-e2e-${randomUUID()}`);
mkdirSync(data, { recursive: true });
const reserve = createServer();
await new Promise((ok, reject) => { reserve.once('error', reject); reserve.listen(0, '127.0.0.1', ok); });
const port = reserve.address().port;
await new Promise(ok => reserve.close(ok));
const endpoint = `http://127.0.0.1:${port}`;
const env = { ...process.env, NEKOPAK_BACKEND: 'rust_compat', NEKOPAK_TEST_DATA_DIR: data,
  NEKOPAK_TEST_HIDE_WINDOW: '1', WEBVIEW2_USER_DATA_FOLDER: join(data, 'webview'),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` };
delete env.NEKOPAK_API_CLIENT_ID;
delete env.NEKOPAK_API_CLIENT_SECRET;
const child = spawn(executable, [], { cwd: root, env, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
let failure;
child.once('error', error => { failure = error; });
let stderr = '';
child.stderr.on('data', bytes => { stderr = (stderr + bytes.toString()).slice(-4096); });
try {
  const until = Date.now() + 40_000;
  let ready = false;
  while (Date.now() < until) {
    if (failure) throw failure;
    if (child.exitCode !== null) throw new Error(`Isolated app exited: ${child.exitCode}; ${stderr}`);
    try {
      const response = await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(1000) });
      const pages = await response.json();
      if (pages.some(page => page.url?.startsWith('http://tauri.localhost'))) { ready = true; break; }
    } catch { /* The fresh WebView2 profile is still starting. */ }
    await new Promise(ok => setTimeout(ok, 200));
  }
  if (!ready) throw new Error('Isolated WebView2 did not start in time.');
  process.env.NEKOPAK_TEST_CDP = endpoint;
  await import('./desktop.mjs');
} finally {
  if (child.exitCode === null) child.kill();
}
