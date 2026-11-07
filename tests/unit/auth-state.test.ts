import { afterEach, expect, it, vi } from 'vitest';
import { api } from '../../src/lib/api';
import { app, refreshAuth } from '../../src/state/app';

afterEach(() => { vi.restoreAllMocks(); app.set({ authError: undefined }); });

it('a racing status response cannot hide an authorization failure', async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.authStatus>>) => void;
  vi.spyOn(api, 'authStatus').mockReturnValue(new Promise(r => { resolve = r; }));
  const loading = refreshAuth();
  app.set({ authError: { code: 'TRANSIENT_NETWORK', message: 'authorization failed', retryable: true } });
  resolve({ state: 'signed_out', checked_at: '2026-11-07T00:00:00Z' });
  await loading;
  expect(app.get().authError?.code).toBe('TRANSIENT_NETWORK');
  expect(app.get().auth?.state).toBe('signed_out');
});
