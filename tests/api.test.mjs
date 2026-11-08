import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/lib/ipc-client.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { applyTransferEvent, createApi } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('duplicate and out of order events cannot regress a reliable snapshot', () => {
  const task = { id: 't', state: 'running', transferred_bytes: '9007199254740993', revision: '9007199254740993' };
  for (const revision of ['9007199254740992', '9007199254740993']) {
    assert.equal(applyTransferEvent(task, { task_id: 't', state: 'queued', revision }), task);
  }
  assert.equal(applyTransferEvent(task, { task_id: 'other', revision: '9007199254740994' }), task);
  assert.equal(applyTransferEvent(task, { task_id: 't', state: 'paused', revision: '9007199254740994' }).state, 'paused');
});

test('IPC preserves snake_case arguments and propagates structured failures', async () => {
  const calls = [];
  const api = createApi(async (command, args) => { calls.push({ command, args }); return { items: [] }; });
  await api('files_list', { parent_id: 'root', limit: 25 });
  assert.deepEqual(calls, [{ command: 'files_list', args: { parent_id: 'root', limit: 25 } }]);
  const error = { code: 'QUOTA_EXCEEDED', message: 'Mock quota', retryable: false };
  await assert.rejects(createApi(async () => { throw error; })('quota_get', {}), (actual) => actual === error);
});

test('Rust-produced fixtures preserve large u64 values and unavailable capabilities', () => {
  const fixture = JSON.parse(readFileSync(new URL('fixtures/free.json', import.meta.url), 'utf8'));
  assert.equal(fixture.files_list.ok.items.find((item) => item.id === 'file-7').size_bytes, '18446744073709551615');
  assert.equal(fixture.backend_discover[0].operations.read_range, false);
  assert.equal(fixture.app_status.features.real_providers, false);
  const unauthorized = JSON.parse(readFileSync(new URL('fixtures/unauthorized.json', import.meta.url), 'utf8'));
  assert.equal(unauthorized.files_list.error.code, 'AUTH_EXPIRED');
});

test('Rust fault fixtures include observable task errors, collisions and reordered events', () => {
  const load = (name) => JSON.parse(readFileSync(new URL(`fixtures/${name}.json`, import.meta.url), 'utf8'));
  for (const [scenario, code] of [['disconnect_at37', 'TRANSIENT_NETWORK'], ['disk_full', 'DISK_FULL'], ['integrity_failure', 'INTEGRITY_FAILED']]) {
    const fixture = load(scenario);
    assert.equal(fixture.transfers_list.items[0].state, 'failed');
    assert.equal(fixture.transfers_list.items[0].error.code, code);
    let task = fixture.transfer_controls[0].snapshot;
    for (const event of fixture.out_of_order_events) task = applyTransferEvent(task, event);
    assert.equal(task.revision, fixture.transfers_list.items[0].revision);
    assert.equal(task.state, 'failed');
  }
  const entries = load('name_conflict').file_pages.flatMap((page) => page.items);
  assert.ok(entries.length > new Set(entries.map((entry) => entry.name)).size);
});

test('unconfigured native adapter never exposes an unauthorized live capability', async () => {
  const fixture = JSON.parse(readFileSync(new URL('fixtures/native_unconfigured.json', import.meta.url), 'utf8'));
  assert.equal(fixture.auth.authenticated, false);
  assert.equal(fixture.auth.credential_owner, 'none');
  assert.equal(fixture.backend_discover.length, 1);
  assert.equal(fixture.backend_discover[0].backend, 'rust_compat');
  assert.equal(fixture.backend_discover[0].status, 'unavailable');
  assert.ok(Object.values(fixture.backend_discover[0].operations).every((value) => value === false));
  const calls = [];
  const api = createApi(async (command, args) => { calls.push({ command, args }); return fixture.auth; });
  await api('auth_status', { request_id: 'status-1' });
  assert.deepEqual(calls, [{ command: 'auth_status', args: { request_id: 'status-1' } }]);
});

test('segment time contract is lossless and not advertised as executable', () => {
  const fixture = JSON.parse(readFileSync(new URL('fixtures/segment_contract.json', import.meta.url), 'utf8'));
  assert.equal(fixture.request.start_ms, '5000');
  assert.equal(fixture.request.end_ms, '8500');
  assert.equal(fixture.capability, false);
  assert.equal(fixture.executable_in_tauri, false);
  const mock = JSON.parse(readFileSync(new URL('fixtures/free.json', import.meta.url), 'utf8'));
  assert.equal(mock.backend_discover[0].operations.segment_download, false);
  assert.equal(mock.app_status.features.segment_download, false);
});

