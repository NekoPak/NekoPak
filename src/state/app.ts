import type { BackendId, CapabilityReport, ClientError } from "../lib/contract";
import { EVENTS } from "../lib/contract";
import type { AnyOperation, AppStatus, AuthFlow, AuthState, QuotaReport, Settings, SettingsPatch, SignInChallenge } from "../lib/contract-proposals";
import { PROPOSED_EVENTS } from "../lib/contract-proposals";
import { api, errorOf, on, transportKind, writeLocal } from "../lib/api";
import { Store } from "../lib/store";
import { scheduleReload, startTransfers } from "./transfers";
import { resetSessionUi, sessionGeneration } from "./session";
import { setLanguage, setSystemLanguageOverride, tr, tx } from "../lib/i18n";
import { entrySource } from "../lib/entry-source";
import { configureNativePreferences, loadAccountPreferences, pauseAccountPreferences } from "./prefs";

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
  authErrorSource?: "status" | "sign_in";
  authBusy: boolean;
  quota?: QuotaReport;
  quotaError?: ClientError;
  quotaLoading: boolean;
  settings?: Settings;
  settingsError?: ClientError;
  disclaimerAccepted: boolean;
  /** Bumped when the remote tree may have changed (scenario switch, re-auth). */
  epoch: number;
  sessionEpoch: number;
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
  sessionEpoch: 0,
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
    const selected = s.app?.mode === "mock" ? "mock" : "rust_compat";
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

export function canSignIn(s: AppState = app.get()): boolean {
  if (s.runtime === "mock") return true;
  const native = s.caps.find(c => c.backend === "rust_compat");
  return s.app?.feature_flags.auth_sign_in === true && !!native && (native.status === "needs_login" || native.status === "ready");
}

/** Host capabilities come from Rust. A narrow browser window is not an Android host. */
export function isMobileHost(s: AppState = app.get()): boolean {
  return s.app?.feature_flags.mobile === true;
}

export function hostSupports(feature: string, s: AppState = app.get()): boolean {
  const value = s.app?.feature_flags[feature];
  // Old desktop builds did not publish these additive flags.
  return value ?? !isMobileHost(s);
}

function clearAccountData(): void {
  resetSessionUi();
  app.set(s => ({ quota: undefined, quotaError: undefined, epoch: s.epoch + 1, sessionEpoch: s.sessionEpoch + 1 }));
  scheduleReload(0);
}

export async function recoverAuth(): Promise<void> {
  app.set({ authBusy: true, authError: undefined });
  try {
    if (app.get().runtime === "tauri") await api.networkReconnect();
    await resync();
  } finally { app.set({ authBusy: false }); }
}

export interface Gate {
  ok: boolean;
  reason?: string;
}

export const OP_LABEL: Record<AnyOperation, string> = {
  get list() { return tr("列目录"); }, get stat() { return tr("读取元数据"); }, get search() { return tr("搜索"); }, get mkdir() { return tr("新建文件夹"); }, get rename() { return tr("重命名"); },
  get move() { return tr("移动"); }, get copy() { return tr("复制"); }, get trash() { return tr("移至回收站"); }, get upload() { return tr("上传"); }, get download() { return tr("下载"); },
  get cloud_task() { return tr("云下载"); }, get stream_url() { return tr("播放链接"); }, get read_range() { return tr("随机读取"); }, get quota() { return tr("配额查询"); },
  get segment_download() { return tr("视频指定片段下载"); }, get restore() { return tr("从回收站还原"); },
  get delete_forever() { return tr("永久删除"); },
  get media_probe() { return tr("读取视频信息"); }, get clip_download() { return tr("下载片段"); }, get quality_download() { return tr("按画质下载"); },
  get folder_download() { return tr("下载文件夹"); }, get star() { return tr("星标"); }, get duplicates() { return tr("查找重复"); }, get space_summary() { return tr("空间分布"); },
  get archive() { return tr("压缩包浏览与解压"); }, get link_preview() { return tr("链接预览"); }, get external_player() { return tr("外部播放器"); }, get share() { return tr("分享"); },
};

/** Whether `op` may be offered right now, with a human reason when not. */
export function gate(op: AnyOperation, s: AppState = app.get()): Gate {
  if (s.boot !== "ready") return { ok: false, reason: tr("正在启动") };
  if ((op === "upload" && !hostSupports("upload", s)) || (op === "external_player" && !hostSupports("media_external", s))) {
    return { ok: false, reason: tr("Android 当前版本尚未支持这项操作") };
  }
  if (!signedIn(s)) {
    return { ok: false, reason: s.auth?.state === "expired" ? tr("登录已过期，请重新授权") : tr("需要先登录") };
  }
  const b = activeBackend(s);
  if (!b || b.status !== "ready") return { ok: false, reason: tr("暂时无法连接 PikPak") };
  if (!(b.operations as Partial<Record<AnyOperation, boolean>>)[op]) return { ok: false, reason: tx("暂不支持{0}", OP_LABEL[op]) };
  if (op === "download" || op === "cloud_task" || op === "clip_download" || op === "quality_download" || op === "folder_download") {
    const dim = op === "cloud_task" ? ["cloud_download_monthly"] : ["downstream_daily", "downstream_monthly"];
    const hit = s.quota?.items.find((i) => dim.includes(i.dimension) && i.state === "exhausted");
    if (hit) return { ok: false, reason: tr("配额已用尽，等待重置后才能继续") };
  }
  return { ok: true };
}

/** Permission belongs to the selected source, even after a tab or route changes. */
export function fileGate(op: AnyOperation, ids: string | string[], s: AppState = app.get()): Gate {
  if (op === "external_player" && !hostSupports("media_external", s)) return { ok: false, reason: tr("Android 当前版本尚未支持外部播放器") };
  const sources = (Array.isArray(ids) ? ids : [ids]).map(entrySource);
  if (sources.includes("shared")) return sources.every(source => source === "shared") ? sharedFileGate(op, s) : { ok: false, reason: tr("请先将分享文件转存到自己的云盘") };
  if (!sources.includes("local")) return gate(op, s);
  if (sources.some(source => source !== "local")) return { ok: false, reason: tr("本地映射为只读，不执行云端写操作") };
  if (s.boot !== "ready") return { ok: false, reason: tr("正在启动") };
  const features = s.app?.feature_flags;
  if ((op === "list" || op === "stat") && features?.local_sources) return { ok: true };
  if (op === "media_probe" && features?.local_media_probe) return { ok: true };
  if ((op === "stream_url" || op === "external_player") && features?.media_session) return { ok: true };
  if ((op === "clip_download" || op === "segment_download") && features?.local_clip) return { ok: true };
  return { ok: false, reason: tr("本地映射为只读，不执行云端写操作") };
}

/** Shared source reads require the backend's explicit shared-preview support. */
export function sharedFileGate(op: AnyOperation, s: AppState = app.get()): Gate {
  if (!["stat", "media_probe", "stream_url", "external_player", "download"].includes(op)) return { ok: false, reason: tr("请先将分享文件转存到自己的云盘") };
  const share = gate("share", s);
  if (!share.ok) return share;
  if (!s.app?.feature_flags.share_preview) return { ok: false, reason: tr("当前版本尚未支持这项操作。") };
  if (op === "stat") return { ok: true };
  if (op === "stream_url") return { ok: !!s.app.feature_flags.media_session, reason: tr("当前宿主未提供媒体播放") };
  return gate(op, s);
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

let authRead = 0;
export async function refreshAuth(): Promise<void> {
  const read = ++authRead;
  try {
    const auth = await api.authStatus();
    if (read === authRead) {
      const previous = app.get().auth?.account_id;
      if (previous && auth.account_id && previous !== auth.account_id) clearAccountData();
      app.set(s => ({ auth, ...(s.authErrorSource === "status" ? { authError: undefined, authErrorSource: undefined } : {}) }));
      if (auth.state === "signed_in") await loadAccountPreferences();
    }
  } catch (e) {
    if (read === authRead) app.set({ authError: errorOf(e), authErrorSource: "status" });
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

/**
 * Native sign-in (§P23). The password lives only in this call's arguments;
 * nothing here stores it. Returns the challenge when a code is needed.
 */
export async function signIn(username: string, password: string): Promise<SignInChallenge | null> {
  const generation = sessionGeneration();
  app.set({ authBusy: true, authError: undefined, authErrorSource: undefined });
  const resumePreferences = await pauseAccountPreferences();
  try {
    const r = await api.authSignIn(username, password);
    app.set({ authBusy: false });
    if (r.state === "challenge" && r.challenge) return r.challenge;
    if (generation === sessionGeneration()) clearAccountData();
    await resync();
    return null;
  } catch (e) {
    app.set({ authError: errorOf(e), authErrorSource: "sign_in", authBusy: false });
    throw e;
  } finally { resumePreferences(); }
}

export async function verifySignIn(challenge_id: string, code: string, password?: string): Promise<SignInChallenge | null> {
  const generation = sessionGeneration();
  app.set({ authBusy: true, authError: undefined, authErrorSource: undefined });
  const resumePreferences = await pauseAccountPreferences();
  try {
    const r = await api.authVerify(challenge_id, code, password);
    app.set({ authBusy: false });
    if (r.state === "challenge" && r.challenge) return r.challenge;
    if (generation === sessionGeneration()) clearAccountData();
    await resync();
    return null;
  } catch (e) {
    app.set({ authError: errorOf(e), authErrorSource: "sign_in", authBusy: false });
    throw e;
  } finally { resumePreferences(); }
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
  const resumePreferences = await pauseAccountPreferences();
  try {
    await api.authLogout(b);
    clearAccountData();
    app.set({ authError: undefined });
    await Promise.all([refreshAuth(), refreshCaps()]);
    app.set({ quota: undefined, epoch: app.get().epoch + 1 });
  } finally { resumePreferences(); }
}

export async function switchAccount(id: string): Promise<void> {
  app.set({ authBusy: true, authError: undefined });
  const generation = sessionGeneration();
  const resumePreferences = await pauseAccountPreferences();
  try {
    const auth = await api.accountSwitch(id);
    if (generation === sessionGeneration()) clearAccountData();
    app.set({ auth });
    await resync();
  } finally { resumePreferences(); app.set({ authBusy: false }); }
}

export async function removeAccount(id: string, active: boolean): Promise<void> {
  const resumePreferences = active ? await pauseAccountPreferences() : () => {};
  try {
    await api.accountRemove(id);
    if (active) {
      clearAccountData();
      app.set({ authError: undefined });
      await resync();
    }
  } finally { resumePreferences(); }
}

export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  if (app.get().runtime === "tauri" && !app.get().app?.feature_flags.settings && Object.keys(patch).every(key => key === "theme")) {
    const settings = { ...app.get().settings!, theme: patch.theme ?? initialTheme() };
    applyTheme(settings.theme); app.set({ settings }); return settings;
  }
  const settings = await api.settingsUpdate(patch);
  setLanguage(settings.language??"system");
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
    setSystemLanguageOverride(status.system_locale);
    configureNativePreferences(runtime === "tauri" && status.feature_flags.ui_prefs === true);
    // Legacy pins had no account identity. Keep them clear until Rust returns
    // preferences for the selected native session.
    if (runtime === "tauri") clearAccountData();

    // Rust restores the transport before exposing commands; also restore UI
    // preferences before discovery/auth events can start network work here.
    try {
      const settings = await api.settingsGet();
      setLanguage(settings.language ?? "system");
      app.set({ settings, settingsError: undefined });
      applyTheme(settings.theme);
    } catch (e) {
      app.set({ settingsError: errorOf(e), settings: {
        schema_version: 1, theme: initialTheme(), download_dir: "", max_concurrent_transfers: 0,
        bandwidth_limit_kib_s: 0, cache: { dir: "", max_bytes: "0" }, telemetry: false,
        experimental_network: { enabled: false, proxy_mode: "system", user_proxy_configured: false, custom_dns_mode: false, connection_diagnostics: false },
      } });
    }

    await on(EVENTS.backendStatus, () => void refreshCaps());
    await on(EVENTS.quotaChanged, (ev) => {
      const q = ev.quota as QuotaReport | undefined;
      if (q && Array.isArray(q.items)) app.set({ quota: q, quotaError: undefined });
    });
    await on(PROPOSED_EVENTS.authChanged, (ev) => {
      const patch: Partial<AppState> = {};
      if (ev.state !== "pending") patch.authFlow = null;
      patch.authError = ev.error;
      patch.authErrorSource = ev.error ? "status" : undefined;
      if (ev.state === "signed_out" || ev.state === "expired") {
        app.set({ auth: { state: ev.state, backend: app.get().auth?.backend ?? activeBackend()?.backend, checked_at: new Date().toISOString() } });
        clearAccountData();
      }
      app.set(patch);
      void Promise.all([refreshAuth(), refreshCaps()]).then(() => {
        if (ev.state === "signed_in") {
          void refreshQuota();
          app.set({ epoch: app.get().epoch + 1 });
        }
      });
    });

    await Promise.all([refreshCaps(), refreshAuth()]);
    await startTransfers();
    await refreshQuota();
    app.set({ boot: "ready" });
  } catch (e) {
    app.set({ boot: "fatal", bootError: errorOf(e) });
  }
}
