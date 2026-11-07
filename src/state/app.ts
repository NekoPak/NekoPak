import type { BackendId, CapabilityReport, ClientError, Operation } from "../lib/contract";
import { EVENTS } from "../lib/contract";
import type { AppStatus, AuthFlow, AuthState, QuotaReport, Settings, SettingsPatch } from "../lib/contract-proposals";
import { PROPOSED_EVENTS } from "../lib/contract-proposals";
import { api, errorOf, on, transportKind, writeLocal } from "../lib/api";
import { Store } from "../lib/store";
import { scheduleReload, startTransfers } from "./transfers";

export interface AppState {
  boot: "loading" | "ready" | "fatal";
  bootError?: ClientError;
  runtime: "mock" | "tauri";
  app?: AppStatus;
  caps: CapabilityReport[];
  capsError?: ClientError;
  capsLoading: boolean;
  auth?: AuthState;
  authFlow: AuthFlow | null;
  authError?: ClientError;
  authBusy: boolean;
  quota?: QuotaReport;
  quotaError?: ClientError;
  quotaLoading: boolean;
  settings?: Settings;
  settingsError?: ClientError;
  disclaimerAccepted: boolean;
  /** Bumped when the remote tree may have changed (scenario switch, re-auth). */
  epoch: number;
}

const DISCLAIMER_KEY = "nekopak.disclaimer.v1";
const THEME_KEY = "nekopak.theme";

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

export const app = new Store<AppState>({
  boot: "loading",
  runtime: "mock",
  caps: [],
  capsLoading: false,
  authFlow: null,
  authBusy: false,
  quotaLoading: false,
  disclaimerAccepted: readFlag(DISCLAIMER_KEY),
  epoch: 0,
});

/* ------------------------------------------------------------------ */
/* Theme                                                               */
/* ------------------------------------------------------------------ */

export type ThemePref = Settings["theme"];

export function applyTheme(pref: ThemePref): void {
  const root = document.documentElement;
  if (pref === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", pref);
  try {
    localStorage.setItem(THEME_KEY, pref);
  } catch {
    /* non-critical */
  }
}

export function initialTheme(): ThemePref {
  try {
    const v = localStorage.getItem(THEME_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    /* ignore */
  }
  return "system";
}

/* ------------------------------------------------------------------ */
/* Capabilities                                                        */
/* ------------------------------------------------------------------ */

export function activeBackend(s: AppState = app.get()): CapabilityReport | undefined {
  if (s.runtime === "tauri") {
    const selected = s.app?.mode === "official_cli_preview" ? "official_cli" : "mock";
    return s.caps.find(c => c.backend === selected);
  }
  if (s.auth?.backend) {
    const hit = s.caps.find((c) => c.backend === s.auth!.backend);
    if (hit) return hit;
  }
  return s.caps.find((c) => c.status === "ready") ?? s.caps.find((c) => c.backend === "mock");
}

export function signedIn(s: AppState = app.get()): boolean {
  return s.auth?.state === "signed_in" || s.auth?.state === "demo";
}

export interface Gate {
  ok: boolean;
  reason?: string;
}

const OP_LABEL: Record<Operation, string> = {
  list: "列目录", stat: "读取元数据", search: "搜索", mkdir: "新建文件夹", rename: "重命名",
  move: "移动", copy: "复制", trash: "移至回收站", upload: "上传", download: "下载",
  cloud_task: "云下载", stream_url: "播放链接", read_range: "随机读取", quota: "配额查询",
  segment_download: "视频指定片段下载",
};

/** Whether `op` may be offered right now, with a human reason when not. */
export function gate(op: Operation, s: AppState = app.get()): Gate {
  if (s.boot !== "ready") return { ok: false, reason: "正在启动" };
  if (!signedIn(s)) {
    return { ok: false, reason: s.auth?.state === "expired" ? "登录已过期，请重新授权" : "需要先登录" };
  }
  const b = activeBackend(s);
  if (!b) return { ok: false, reason: "没有可用的后端" };
  if (b.status !== "ready") return { ok: false, reason: `后端 ${backendLabel(b.backend)} 当前不可用` };
  if (!b.operations[op]) return { ok: false, reason: `当前后端未提供“${OP_LABEL[op]}”能力` };
  if (op === "download" || op === "cloud_task") {
    const dim = op === "download" ? ["downstream_daily", "downstream_monthly"] : ["cloud_download_monthly"];
    const hit = s.quota?.items.find((i) => dim.includes(i.dimension) && i.state === "exhausted");
    if (hit) return { ok: false, reason: "配额已用尽，切换后端不会恢复配额" };
  }
  return { ok: true };
}

export function backendLabel(id: BackendId | undefined): string {
  switch (id) {
    case "official_cli": return "官方 CLI";
    case "rust_compat": return "社区兼容层";
    case "webdav": return "WebDAV";
    case "rclone": return "Rclone";
    case "mock": return "Mock";
    default: return "未知";
  }
}

/* ------------------------------------------------------------------ */
/* Actions                                                             */
/* ------------------------------------------------------------------ */

export async function refreshCaps(): Promise<void> {
  app.set({ capsLoading: true });
  try {
    const caps = await api.backendDiscover();
    app.set({ caps, capsError: undefined, capsLoading: false });
  } catch (e) {
    app.set({ capsError: errorOf(e), capsLoading: false });
  }
}

export async function refreshAuth(): Promise<void> {
  try {
    const auth = await api.authStatus();
    // A status read racing with auth/changed must not erase a just-reported login failure.
    app.set({ auth });
  } catch (e) {
    app.set({ authError: errorOf(e) });
  }
}

export async function refreshQuota(): Promise<void> {
  if (!signedIn()) {
    app.set({ quota: undefined, quotaError: undefined });
    return;
  }
  app.set({ quotaLoading: true });
  try {
    const quota = await api.quotaGet();
    app.set({ quota, quotaError: undefined, quotaLoading: false });
  } catch (e) {
    app.set({ quotaError: errorOf(e), quotaLoading: false });
  }
}

export async function beginAuth(backend: BackendId): Promise<void> {
  app.set({ authBusy: true, authError: undefined });
  try {
    const flow = await api.authBegin(backend);
    app.set({ authFlow: flow, authBusy: false });
    await refreshAuth();
  } catch (e) {
    app.set({ authError: errorOf(e), authBusy: false, authFlow: null });
  }
}

export async function cancelAuth(): Promise<void> {
  const flow = app.get().authFlow;
  if (!flow) return;
  try {
    await api.authCancel(flow.flow_id);
  } catch (e) {
    app.set({ authError: errorOf(e) });
  }
  app.set({ authFlow: null });
  await refreshAuth();
}

export async function logout(): Promise<void> {
  const b = app.get().auth?.backend ?? activeBackend()?.backend;
  if (!b) return;
  await api.authLogout(b);
  await Promise.all([refreshAuth(), refreshCaps()]);
  app.set({ quota: undefined, epoch: app.get().epoch + 1 });
}

export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  if (app.get().runtime === "tauri" && Object.keys(patch).every(key => key === "theme")) {
    const settings = { ...app.get().settings!, theme: patch.theme ?? initialTheme() };
    applyTheme(settings.theme); app.set({ settings }); return settings;
  }
  const settings = await api.settingsUpdate(patch);
  app.set({ settings, settingsError: undefined });
  applyTheme(settings.theme);
  return settings;
}

export function acceptDisclaimer(): void {
  try {
    localStorage.setItem(DISCLAIMER_KEY, "1");
  } catch {
    /* non-critical: will be asked again next launch */
  }
  app.set({ disclaimerAccepted: true });
}

export function resetDisclaimerForDemo(): void {
  writeLocal(DISCLAIMER_KEY, "0");
  app.set({ disclaimerAccepted: false });
}

/** Re-read everything after a scenario switch or reconnect. */
export async function resync(): Promise<void> {
  await Promise.all([refreshCaps(), refreshAuth()]);
  await refreshQuota();
  scheduleReload(0);
  app.set({ epoch: app.get().epoch + 1 });
}

let booted = false;

export async function boot(): Promise<void> {
  if (booted) return;
  booted = true;
  applyTheme(initialTheme());
  try {
    const runtime = await transportKind();
    app.set({ runtime });
    const status = await api.appStatus();
    app.set({ app: status });

    await on(EVENTS.backendStatus, () => void refreshCaps());
    await on(EVENTS.quotaChanged, (ev) => {
      const q = ev.quota as QuotaReport | undefined;
      if (q && Array.isArray(q.items)) app.set({ quota: q, quotaError: undefined });
    });
    await on(PROPOSED_EVENTS.authChanged, (ev) => {
      const patch: Partial<AppState> = {};
      if (ev.state !== "pending") patch.authFlow = null;
      patch.authError = ev.error;
      app.set(patch);
      void refreshAuth().then(() => {
        if (ev.state === "signed_in") {
          void refreshQuota();
          app.set({ epoch: app.get().epoch + 1 });
        }
      });
    });

    await Promise.all([refreshCaps(), refreshAuth()]);
    try {
      const settings = await api.settingsGet();
      app.set({ settings });
      applyTheme(settings.theme);
    } catch (e) {
      // These are UI display defaults only. Backend-affecting controls stay disabled.
      app.set({ settingsError: errorOf(e), settings: {
        schema_version: 1, theme: initialTheme(), download_dir: "", max_concurrent_transfers: 0,
        bandwidth_limit_kib_s: 0, cache: { dir: "", max_bytes: "0" }, telemetry: false,
        experimental_network: { enabled: false, proxy_mode: "system", user_proxy_configured: false, custom_dns_mode: false, connection_diagnostics: false },
      } });
    }
    await startTransfers();
    await refreshQuota();
    app.set({ boot: "ready" });
  } catch (e) {
    app.set({ boot: "fatal", bootError: errorOf(e) });
  }
}
