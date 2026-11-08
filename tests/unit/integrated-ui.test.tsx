// Render without a browser or desktop window (safe while the user is using other apps).
import { render } from 'preact-render-to-string';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Sidebar } from '../../src/app/Shell';
import { SettingsPage } from '../../src/pages/Settings';
import { app, canSignIn, gate } from '../../src/state/app';
import { authForUi, statusForUi } from '../../src/lib/transport/rust-adapter';
import type { AppStatus, AuthState, CapabilityReport } from '../../src/lib/api-types';
import { AuthPanel } from '../../src/pages/Auth';
import { ErrorPanel } from '../../src/components/ui';
import free from '../fixtures/free.json';
import native from '../fixtures/native_default.json';

const original = app.get();
beforeEach(() => app.set({
  boot: 'ready', runtime: 'tauri', app: statusForUi(free.app_status as AppStatus),
  auth: { state: 'demo', backend: 'mock', checked_at: '2026-11-07T00:00:00Z' },
  caps: free.backend_discover as CapabilityReport[],
}));
afterEach(() => app.replace(original));

describe('integrated UI product state and requested navigation', () => {
  it('the normal Windows build offers password entry and login without environment setup', () => {
    app.set({ app: { ...app.get().app!, mode: 'rust_compat_preview', feature_flags: { auth_sign_in: native.auth_sign_in_enabled, auth_logout: true } },
      auth: authForUi(native.auth as AuthState), caps: native.backend_discover as CapabilityReport[], authError: undefined });
    expect(canSignIn()).toBe(true);
    const html = render(<AuthPanel />);
    expect(html).toContain('name="username"');
    expect(html).toContain('name="password"');
    expect(html).not.toContain('disabled');
    expect(html).not.toContain('尚不能连接');
  });
  it('a login-attempt error does not discard the form or a pending verification flow', () => {
    app.set({ app: { ...app.get().app!, mode: 'rust_compat_preview', feature_flags: { auth_sign_in: true } },
      auth: authForUi(native.auth as AuthState), caps: native.backend_discover as CapabilityReport[],
      authError: { code: 'CONFLICT', message: '等待验证完成', retryable: false }, authErrorSource: 'sign_in' });
    const html = render(<AuthPanel />);
    expect(html).toContain('name="password"');
    expect(html).not.toContain('清除本地会话');
  });
  it('places tasks in an independent sidebar item', () => {
    const html = render(<Sidebar collapsed={false} />);
    expect(html).toContain('aria-label="下载');
    expect(html).toContain('aria-label="网盘"');
    expect(html).toContain('dl-btn');
    expect(html).not.toContain('mode-switch');
    expect(html).toContain('未连接 PikPak');
  });
  it('connection page does not present mock capabilities as real installed services', () => {
    const html = render(<SettingsPage section="backends" />);
    expect(html).toContain('账号');
    expect(html).toContain('未连接 PikPak');
    expect(html).not.toContain('CLI');
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
  it('native login cannot collect credentials before the core enables sign-in', () => {
    app.set({ app: { ...app.get().app!, mode: 'rust_compat_preview' }, auth: { state: 'signed_out', backend: 'rust_compat', checked_at: '2026-11-08T00:00:00Z' }, caps: [{ backend: 'rust_compat', status: 'unavailable', operations: {}, limitations: [], checked_at: '2026-11-08T00:00:00Z' }] });
    const html = render(<AuthPanel />);
    expect(canSignIn()).toBe(false);
    expect(html).toContain('当前版本尚不能连接账号');
    expect(html).toContain('disabled');
    expect(html).not.toContain('CLI');
    expect(gate('download').ok).toBe(false);
  });
  it('vault failure disables sign-in even when the static feature is enabled', () => {
    app.set({ app: { ...app.get().app!, mode: 'rust_compat_preview', feature_flags: { auth_sign_in: true, auth_logout: true } }, auth: undefined,
      authError: { code: 'PERMISSION_DENIED', message: '凭据库不可用', retryable: false },
      caps: [{ backend: 'rust_compat', status: 'unavailable', operations: {}, limitations: [], checked_at: '2026-11-08T00:00:00Z' }] });
    expect(canSignIn()).toBe(false);
    const html = render(<AuthPanel />);
    expect(html).toContain('凭据库不可用');
    expect(html).toContain('重试连接');
    expect(html).toContain('清除本地会话');
    expect(html).not.toContain('name="password"');
  });
  it('a transient saved-session error exposes recovery instead of a blocked sign-in loop', () => {
    app.set({ app: { ...app.get().app!, mode: 'rust_compat_preview', feature_flags: { auth_sign_in: true, auth_logout: true } }, auth: undefined,
      authError: { code: 'TRANSIENT_NETWORK', message: '无法刷新会话', retryable: true } });
    const html = render(<AuthPanel />);
    expect(html).toContain('无法刷新会话');
    expect(html).toContain('重试连接');
    expect(html).toContain('清除本地会话');
  });
  it('unimplemented v3 media and sync capabilities remain unavailable in desktop mock', () => {
    for (const op of ['clip_download', 'quality_download', 'folder_download', 'share', 'archive'] as const) expect(gate(op).ok).toBe(false);
    expect(render(<SettingsPage section="transfers" />)).toContain('下载原始文件');
  });
  it('a drive challenge offers verification and continuation without suggesting an account upgrade', () => {
    const html=render(<ErrorPanel error={{code:'VERIFICATION_REQUIRED',message:'需要安全验证',retryable:false,details:{verification_id:'local-challenge'}}} onRetry={()=>{}} />);
    expect(html).toContain('完成安全验证');expect(html).toContain('已完成验证，继续');expect(html).not.toContain('Premium');expect(html).not.toContain('重新授权');
  });
  it('download settings provide a folder picker instead of requiring a typed path', () => {
    const html=render(<SettingsPage section="transfers" />);
    expect(html).toContain('选择文件夹');
    expect(html).toContain('native-download-dir');
  });
  it('does not present autostart as available without a native host capability', () => {
    app.set({app:{...app.get().app!,feature_flags:{...app.get().app!.feature_flags,autostart:false}}});
    const html = render(<SettingsPage section="general"/>);
    const control = html.match(/<button[^>]*aria-label="开机自动启动"[^>]*>/)?.[0];
    expect(control).toContain('role="switch"');
    expect(control).toContain('aria-checked="false"');
    expect(control).toContain('aria-disabled="true"');
    expect(html).toContain('当前平台尚未支持自动启动');
  });
});
