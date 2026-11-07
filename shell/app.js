/* Minimal backend acceptance host. Product UI belongs in src/ and can replace this shell. */
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
let parent = 'root';
let cursor;
let renderRevision = 0;
let mockMode = true;
function fail(error) { $('error').textContent = `${error.code || 'INTERNAL'}: ${error.message || String(error)}`; }
function action(text, handler, enabled = true) {
  const button = document.createElement('button'); button.textContent = text; button.disabled = !enabled;
  button.addEventListener('click', () => handler().catch(fail)); return button;
}
async function files(append = false) {
  const page = await invoke('files_list', { parent_id: parent, cursor: append ? cursor : null, limit: 50 });
  if (!append) $('files').replaceChildren();
  for (const entry of page.items) {
    const row = document.createElement('li'); const label = document.createElement('span');
    label.textContent = `${entry.kind === 'folder' ? '📁' : '📄'} ${entry.name} · ${entry.size_bytes} bytes`; row.append(label);
    if (entry.kind === 'folder') row.append(action('打开', async () => { parent = entry.id; await files(); }));
    else row.append(action(mockMode ? '模拟下载' : '下载未启用', async () => { await invoke('transfer_start', { remote_id: entry.id, destination: `C:/NekoPak-Mock/${entry.id}.bin`, idempotency_key: crypto.randomUUID() }); await tasks(); }, mockMode));
    $('files').append(row);
  }
  cursor = page.next_cursor; $('more').disabled = !cursor; $('folder').textContent = parent;
}
async function tasks() {
  const generation = ++renderRevision;
  let after; const all = [];
  do { const page = await invoke('transfers_list', { cursor: after ?? null, limit: 500 }); all.push(...page.items); after = page.next_cursor; } while (after);
  if (generation !== renderRevision) return;
  $('tasks').replaceChildren();
  for (const task of all) {
    const row = document.createElement('li'); const label = document.createElement('span');
    label.textContent = `模拟 ${task.remote_id} · ${task.state} · ${task.transferred_bytes}/${task.total_bytes ?? '?'}${task.error ? ' · ' + task.error.message : ''}`; row.append(label);
    for (const [name, title, enabled] of [
      ['pause', '暂停', ['queued', 'running', 'retrying'].includes(task.state)],
      ['resume', '继续', task.state === 'paused'], ['retry', '重试', task.state === 'failed'],
      ['cancel', '取消', !['completed', 'cancelled'].includes(task.state)],
    ]) row.append(action(title, async () => { await invoke('transfer_control', { task_id: task.id, action: name }); await tasks(); }, enabled));
    $('tasks').append(row);
  }
}
async function refresh() {
  $('error').textContent = '';
  const status = await invoke('app_status');
  mockMode = status.mode === 'mock';
  $('mode').textContent = mockMode ? '离线 Mock 模式。文件、配额和进度均为模拟数据，不连接 PikPak，也不写入下载文件。' : '官方 CLI 只读预览。读取外部 CLI 的授权状态与文件元数据；下载、云任务写入、同步和挂载尚未启用。';
  $('cloud').hidden = !mockMode;
  for (const control of $('cloud').elements) control.disabled = !mockMode;
  const [backends, quota, auth] = await Promise.allSettled([invoke('backend_discover'), invoke('quota_get'), invoke('auth_status')]);
  $('status').textContent = JSON.stringify({ status, backends, quota, auth }, null, 2);
  if (status.storage.error) fail(status.storage.error);
  const results = await Promise.allSettled([files(), tasks()]);
  for (const result of results) if (result.status === 'rejected') fail(result.reason);
}
$('refresh').onclick = () => refresh().catch(fail);
$('root').onclick = () => { parent = 'root'; files().catch(fail); };
$('more').onclick = () => files(true).catch(fail);
$('cloud').onsubmit = async (event) => {
  event.preventDefault();
  try { await invoke('cloud_task_create', { source_url: $('url').value, parent_id: parent, idempotency_key: crypto.randomUUID() }); await tasks(); } catch (error) { fail(error); }
};
async function start() {
  try { await tasks(); } catch (error) { fail(error); }
  await listen('nekopak://transfer/changed', () => tasks().catch(fail));
  await listen('nekopak://transfer/resync-required', () => tasks().catch(fail));
  await listen('nekopak://storage/error', ({ payload }) => fail(payload));
  await refresh(); // Reconcile the gap between initial snapshot and event subscription.
}
start().catch(fail);
