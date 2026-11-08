import { afterEach, expect, it, vi } from "vitest";
import { render } from "preact-render-to-string";
import { SettingsPage } from "../../src/pages/Settings";
import { app, updateSettings } from "../../src/state/app";
import { api } from "../../src/lib/api";
import { MockTransport } from "../../src/lib/mock/backend";
import { setLanguage, tr, type Language } from "../../src/lib/i18n";

const original = app.get();
afterEach(() => { app.replace(original); setLanguage("zh-CN"); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("defaults the native access-domain switch off and renders a saved enabled value independently of network compatibility", async () => {
  const mock = new MockTransport("free", { latency_ms: 0 });
  try {
    const settings = await mock.invoke("settings_get", undefined);
    expect(settings.experimental_network.block_access_domains).toBe(false);
    for (const value of [undefined, false, true]) {
      app.set({ runtime: "tauri", settings: { ...settings, experimental_network: { ...settings.experimental_network, enabled: false, block_access_domains: value } } });
      const html = render(<SettingsPage section="network"/>);
      expect(html).toMatch(new RegExp(`role="switch"[^>]*aria-checked="${value === true}"[^>]*aria-label="屏蔽 access 域名"`));
      expect(html).toContain("不修改系统 Hosts");
      expect(html).toContain("当前直连路径不调用这些域名");
      for (const domain of ["access.mypikpak.com", "access.mypikpak.net", "access.pikpak.me", "access.pikpakdrive.com"]) expect(html).toContain(`<code>${domain}</code>`);
    }
  } finally { mock.dispose(); }
});

it("passes the toggle to settings_update while enabled remains false and restores the stored value", async () => {
  const mock = new MockTransport("free", { latency_ms: 0 });
  vi.stubGlobal("document", { documentElement: { lang: "", setAttribute: vi.fn(), removeAttribute: vi.fn() } });
  const update = vi.spyOn(api, "settingsUpdate").mockImplementation(patch => mock.invoke("settings_update", { patch }));
  try {
    const initial = await mock.invoke("settings_get", undefined);
    app.set({ runtime: "tauri", settings: initial });
    for (const block_access_domains of [true, false]) {
      await updateSettings({ experimental_network: { enabled: false, block_access_domains } });
      expect(update).toHaveBeenLastCalledWith({ experimental_network: { enabled: false, block_access_domains } });
      const restored = await mock.invoke("settings_get", undefined);
      expect(restored.experimental_network.enabled).toBe(false);
      expect(restored.experimental_network.block_access_domains).toBe(block_access_domains);
      expect(app.get().settings?.experimental_network.block_access_domains).toBe(block_access_domains);
    }
  } finally { mock.dispose(); }
});

it("translates all five language labels without translating domain names", async () => {
  const mock = new MockTransport("free", { latency_ms: 0 });
  try {
    app.set({ runtime: "tauri", settings: await mock.invoke("settings_get", undefined) });
    for (const locale of ["zh-CN", "zh-TW", "en", "ja-JP", "ko-KR"] as Language[]) {
      setLanguage(locale);
      const html = render(<SettingsPage section="network"/>);
      expect(html).toContain(tr("屏蔽 access 域名"));
      expect(html).toContain(tr("当前直连路径不调用这些域名；此开关仅在出现相关请求时阻止连接。"));
      for (const domain of ["access.mypikpak.com", "access.mypikpak.net", "access.pikpak.me", "access.pikpakdrive.com"]) expect(html).toContain(`<code>${domain}</code>`);
    }
  } finally { mock.dispose(); }
});
