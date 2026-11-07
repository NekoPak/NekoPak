// Render without a browser or desktop window (safe while the user is using other apps).
import { render } from 'preact-render-to-string';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Sidebar } from '../../src/app/Shell';
import { SettingsPage } from '../../src/pages/Settings';
import { app } from '../../src/state/app';
import { statusForUi } from '../../src/lib/transport/rust-adapter';
import type { AppStatus, CapabilityReport } from '../../src/lib/api-types';
import free from '../fixtures/free.json';

const original = app.get();
beforeEach(() => app.set({
  boot: 'ready', runtime: 'tauri', app: statusForUi(free.app_status as AppStatus),
  auth: { state: 'demo', backend: 'mock', checked_at: '2026-11-07T00:00:00Z' },
  caps: [...free.backend_discover as CapabilityReport[], {
    backend: 'official_cli', status: 'not_installed', operations: {}, checked_at: '2026-11-07T00:00:00Z',
    limitations: ['No reviewed executable/profile is configured; PATH is not searched or executed.'],
  }],
}));
afterEach(() => app.replace(original));

describe('integrated UI product state and requested navigation', () => {
  it('places tasks in an independent sidebar item', () => {
    const html = render(<Sidebar collapsed={false} />);
    expect(html).toContain('aria-label="任务"');
    expect(html).toContain('aria-label="网盘"');
    expect(html).toContain('sb-primary');
    expect(html).not.toContain('mode-switch');
    expect(html).toContain('未连接 PikPak');
  });
  it('connection page does not present mock capabilities as real installed services', () => {
    const html = render(<SettingsPage section="backends" />);
    expect(html).toContain('PikPak 连接');
    expect(html).toContain('官方 CLI');
    expect(html).toContain('未配置');
    expect(html).not.toContain('未安装');
    expect(html).not.toContain('mock-0.1');
    expect(html).not.toContain('OFFLINE SIMULATION');
    expect(html).not.toContain('No reviewed executable');
    expect(html).not.toContain('op-chips');
  });
  it('keeps technical mock diagnostics explicitly labeled on the developer page', () => {
    const html = render(<SettingsPage section="developer" />);
    expect(html).toContain('开发测试与连接诊断');
    expect(html).toContain('仅离线模拟，不是实际账号或服务能力');
    expect(html).toContain('可模拟');
  });
});
