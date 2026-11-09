import { language, tr, tx } from "../lib/i18n";
import { useEffect, useRef, useState } from "preact/hooks";
import { useStore } from "../lib/store";
import { app, boot, isMobileHost } from "../state/app";
import { activeTab, closeTab, cycleTab, initRouter, navigate, navigateHistory, openTab, route, workspaceTabs } from "../state/route";
import { WorkspaceTabs } from "../components/WorkspaceTabs";
import { loadSnapshot } from "../state/transfers";
import { closeDialog, closeMenu, dialogs, focusRequests, menu, toast } from "../state/ui";
import { applyPrefs, closeSide, prefs, setPref, side } from "../state/prefs";
import { clip } from "../state/clipboard";
import { runPageCommand } from "../state/bus";
import { startJobs, watchJobs } from "../state/jobs";
import { undo } from "../state/undo";
import { DialogHost, MenuHost, ToastHost } from "../components/hosts";
import { openPalette, palette } from "../state/palette";
import { lazyPage } from "./lazyPage";
import { newCloudTask } from "../components/dialogs";
import { LogoMark } from "../components/Brand";
import { Icon } from "../components/Icon";
import { ErrorPanel } from "../components/ui";
import { showShortcuts } from "../components/dialogs";
import { NAV_KEYS, Sidebar } from "./Shell";
import { FilesPage } from "../pages/Files";
import { RecentPage, SearchPage, StarredPage, TrashPage } from "../pages/Lists";

import { TransfersPage } from "../pages/Transfers";

import { SettingsPage } from "../pages/Settings";
import { Onboarding } from "../pages/Onboarding";
import { requestRefresh } from "../pages/common";
import { on } from "../lib/api";

const SharesPage = lazyPage(() => import("../pages/Shares").then((m) => m.SharesPage));
const ReferencesPage = lazyPage<{ manifest?: string }>(() => import("../pages/References").then(m => m.ReferencesPage));
const DuplicatesPage = lazyPage<{ folder: string }>(() => import("../pages/Duplicates").then((m) => m.DuplicatesPage));
const SpacePage = lazyPage<{ folder: string }>(() => import("../pages/Space").then((m) => m.SpacePage));
const ArchivePage = lazyPage<{ file: string; path: string }>(() => import("../pages/Archive").then((m) => m.ArchivePage));
const SyncPage = lazyPage<{ config?: string }>(() => import("../pages/Sync").then((m) => m.SyncPage));
const MountPage = lazyPage(() => import("../pages/Mount").then((m) => m.MountPage));
const LocalSourcesPage = lazyPage(() => import("../pages/LocalSources").then(m => m.LocalSourcesPage));
const GalleryPage = lazyPage<{folder:string;mode:"images"|"videos"}>(() => import("../pages/Gallery").then(m => m.GalleryPage));
const PaletteHost = lazyPage(() => import("../components/Palette").then((m) => m.PaletteHost));

function PaletteSlot() {
  const open = useStore(palette, (p) => p.open);
  return open ? <PaletteHost /> : null;
}

function isTyping(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (el as HTMLElement).isContentEditable;
}

function useGlobalShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialogs.get().stack.length || menu.get().open || palette.get().open) return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && e.key.toLowerCase() === "t") { e.preventDefault(); openTab(); return; }
      if (ctrl && e.key.toLowerCase() === "w") { e.preventDefault(); closeTab(); return; }
      if (ctrl && e.key === "Tab") { e.preventDefault(); cycleTab(e.shiftKey ? -1 : 1); return; }
      if (ctrl && ((!e.shiftKey && (e.key === "k" || e.key === "K")) || (e.shiftKey && (e.key === "p" || e.key === "P")))) {
        e.preventDefault();
        openPalette();
        return;
      }
      if (ctrl && !e.shiftKey && (e.key === "z" || e.key === "Z") && !isTyping(document.activeElement)) {
        e.preventDefault();
        void undo().then((ok) => { if (!ok) toast(tr("没有可以撤销的操作"), "info"); });
        return;
      }
      if (ctrl && !e.shiftKey && (e.key === "v" || e.key === "V") && !isTyping(document.activeElement) && clip.get().mode) {
        e.preventDefault();
        runPageCommand("paste");
        return;
      }
      if (ctrl && !e.shiftKey && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        const pg = route.get().current.page;
        if (pg !== "files" && pg !== "search") navigate({ page: "files", folder: "root" });
        // Let the files page mount before asking it to open its search field.
        setTimeout(() => focusRequests.set((s) => ({ search: s.search + 1 })), pg === "files" || pg === "search" ? 0 : 60);
        return;
      }
      if (ctrl && !e.shiftKey && /^[1-8]$/.test(e.key)) {
        e.preventDefault();
        const n = NAV_KEYS.find((x) => x.key === e.key);
        if (n) navigate(n.to);
        return;
      }
      if (ctrl && (e.key === "b" || e.key === "B")) {
        e.preventDefault();
        setPref("sidebarCollapsed", !prefs.get().sidebarCollapsed);
        return;
      }
      if (ctrl && e.key === ",") {
        e.preventDefault();
        navigate({ page: "settings", section: "general" });
        return;
      }
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        navigateHistory(-1);
        return;
      }
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        navigateHistory(1);
        return;
      }
      if (e.key === "F5" || (ctrl && (e.key === "r" || e.key === "R"))) {
        e.preventDefault();
        requestRefresh();
        if (route.get().current.page === "transfers") void loadSnapshot();
        return;
      }
      if (e.key === "F1" || (e.key === "?" && !isTyping(document.activeElement))) {
        e.preventDefault();
        void showShortcuts();
      }
    };
    const onMouse = (e: MouseEvent) => {
      if (e.button === 3) navigateHistory(-1);
      if (e.button === 4) navigateHistory(1);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mouseup", onMouse);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mouseup", onMouse);
    };
  }, []);
}

function Page() {
  const r = useStore(route, (s) => s.current);
  switch (r.page) {
    case "files": return <FilesPage folder={r.folder} />;
    case "search": return <SearchPage query={r.query} />;
    case "recent": return <RecentPage />;
    case "transfers": return <TransfersPage tab={r.tab} />;
    case "cloud": return <TransfersPage tab="active" kind="cloud_download" />;
    case "sync": return <SyncPage config={r.config} />;
    case "mount": return <MountPage />;
    case "trash": return <TrashPage />;
    case "starred": return <StarredPage />;
    case "shares": return <SharesPage />;
    case "references": return <ReferencesPage manifest={r.manifest}/>;
    case "local": return <LocalSourcesPage />;
    case "gallery": return <GalleryPage folder={r.folder} mode={r.mode}/>;
    case "duplicates": return <DuplicatesPage folder={r.folder} />;
    case "space": return <SpacePage folder={r.folder} />;
    case "archive": return <ArchivePage file={r.file} path={r.path} />;
    case "settings": return <SettingsPage section={r.section} />;
    case "about": return <SettingsPage section="about" />;
  }
}

export function App() {
  // Rerender translated labels without remounting the tree or losing forms.
  useStore(language, state => state.preference);
  const tabId = useStore(workspaceTabs, state => state.activeId);
  const s = useStore(app, (x) => ({ boot: x.boot, err: x.bootError, accepted: x.disclaimerAccepted, auth: x.auth?.state, sessionEpoch: x.sessionEpoch }));
  const collapsedPref = useStore(prefs, (p) => p.sidebarCollapsed);
  const mobile = useStore(app, isMobileHost);
  const currentRoute = useStore(route, state => state.current);
  const canBack = useStore(workspaceTabs, () => activeTab().at > 0);
  const [narrow, setNarrow] = useState(() => typeof window !== "undefined" && window.innerWidth < 1000);
  const [phone, setPhone] = useState(() => typeof window !== "undefined" && window.innerWidth <= 600);
  const [navOpen, setNavOpen] = useState(false);
  const drawer = useRef<HTMLDivElement>(null);
  const menuTrigger = useRef<HTMLButtonElement>(null);
  // Decided once when boot completes: first run, or signed out at launch.
  const [obActive, setObActive] = useState<boolean | null>(null);
  useGlobalShortcuts();

  useEffect(() => {
    applyPrefs();
    initRouter();
    void boot();
    const mq = window.matchMedia("(max-width: 999px)");
    const phoneQuery = window.matchMedia("(max-width: 600px)");
    const onChange = () => { setNarrow(mq.matches); setPhone(phoneQuery.matches); };
    mq.addEventListener("change", onChange);
    phoneQuery.addEventListener("change", onChange);
    return () => { mq.removeEventListener("change", onChange); phoneQuery.removeEventListener("change", onChange); };
  }, []);
  useEffect(() => setNavOpen(false), [currentRoute, phone]);
  useEffect(() => {
    if (!phone || !navOpen) return;
    drawer.current?.querySelector<HTMLElement>("button, a[href]")?.focus();
    return () => menuTrigger.current?.focus();
  }, [navOpen, phone]);
  useEffect(() => {
    if (!mobile) return;
    // The Android activity sends this event for system-back. Frontend routing
    // remains authoritative; opening the drawer never adds fake history.
    const back = (event: Event) => {
      const stack = dialogs.get().stack;
      if (menu.get().open) { event.preventDefault(); closeMenu(); }
      else if (palette.get().open) { event.preventDefault(); palette.set({ open: false }); }
      else if (stack.length) {
        event.preventDefault();
        const top = stack[stack.length - 1];
        if (top.dismissable !== false) closeDialog(top.id);
      }
      else if (side.get().panel) { event.preventDefault(); void closeSide(); }
      else if (navOpen) { event.preventDefault(); setNavOpen(false); }
      else if (activeTab().at > 0) { event.preventDefault(); navigateHistory(-1); }
    };
    window.addEventListener("nekopak:back", back);
    return () => window.removeEventListener("nekopak:back", back);
  }, [mobile, navOpen]);
  useEffect(()=>{
    let stop:(()=>void)|undefined;let disposed=false;
    void on("nekopak://desktop/navigate",()=>navigate({page:"transfers",tab:"active"})).then(unlisten=>{if(disposed)unlisten();else stop=unlisten;});
    return ()=>{disposed=true;stop?.();};
  },[]);

  useEffect(() => {
    if (s.boot === "ready" && obActive === null) setObActive(!s.accepted || s.auth === "signed_out");
    if (s.boot === "ready") void startJobs();
  }, [s.boot, s.auth]);

  // Server-side jobs (cloud extraction) announce when they finish.
  useEffect(() => watchJobs((job, prev) => {
    if (prev?.state === job.state) return;
    if (job.state === "completed") toast(tx("{0}已完成", job.title), "success", job.target_parent_id ? { action: { label: tr("打开"), run: () => navigate({ page: "files", folder: job.target_parent_id! }) } } : {});
    if (job.state === "failed") toast(tx("{0}失败：{1}", job.title, job.error?.message ?? "未知原因"), "error", job.error ? { error: job.error } : {});
  }), []);

  // Optional: offer a link found on the clipboard when the window regains focus.
  useEffect(() => {
    let last = "";
    const onFocus = async () => {
      if (!prefs.get().clipboardWatch || !navigator.clipboard?.readText) return;
      try {
        const text = (await navigator.clipboard.readText()).trim();
        if (!text || text === last || !/^(magnet:\?xt=urn:btih:|https?:\/\/)\S+$/i.test(text)) return;
        last = text;
        toast(tr("剪贴板中有一个链接"), "info", { action: { label: tr("添加"), run: () => void newCloudTask(route.get().current.page === "files" ? (route.get().current as { folder: string }).folder : "root", "/", text) } });
      } catch {
        /* clipboard permission denied: stay quiet */
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  if (s.boot === "loading") {
    return (
      <div class="boot" role="status">
        <LogoMark size={40} />
        <span>{tr("正在启动 NekoPak…")}</span>
      </div>
    );
  }
  if (s.boot === "fatal" && s.err) {
    return (
      <div class="boot">
        <LogoMark size={40} />
        <div class="boot-error"><ErrorPanel error={s.err} onRetry={() => location.reload()} context="boot" /></div>
      </div>
    );
  }

  const showOnboarding = !s.accepted || obActive === true;
  // Narrow windows always use the rail; the user preference applies to wide windows.
  const collapsed = !phone && (narrow || collapsedPref);

  return (
    <div class={`app${collapsed ? " rail" : ""}${mobile ? " mobile-host" : ""}${phone && navOpen ? " nav-open" : ""}`}>
      <a class="skip-link" href="#main">{tr("跳到主要内容")}</a>
      {phone ? <header class="mobile-topbar" inert={navOpen || showOnboarding || undefined}>
        <button ref={menuTrigger} type="button" class="btn btn-quiet btn-icon" aria-label={tr("打开导航")} aria-expanded={navOpen} aria-controls="app-navigation" onClick={() => setNavOpen(true)}><Icon name="menu" /></button>
        <LogoMark size={24} /><span class="mobile-brand">NekoPak</span>
        <button type="button" class="btn btn-quiet btn-icon" aria-label={tr("返回上一页")} disabled={!canBack} onClick={() => navigateHistory(-1)}><Icon name="arrowBack" /></button>
      </header> : null}
      {phone && navOpen ? <button type="button" class="mobile-scrim" aria-label={tr("关闭导航")} tabIndex={-1} onClick={() => setNavOpen(false)} /> : null}
      <div ref={drawer} id="app-navigation" class="navigation-shell" role={phone && navOpen ? "dialog" : undefined} aria-modal={phone && navOpen ? "true" : undefined} aria-label={phone ? tr("主导航") : undefined} inert={phone && !navOpen || undefined}
        onKeyDown={e => {
          if (!phone || !navOpen) return;
          if (e.key === "Escape") { e.stopPropagation(); setNavOpen(false); }
          if (e.key === "Tab") {
            const items = [...drawer.current!.querySelectorAll<HTMLElement>('button:not([disabled]), a[href]')].filter(el => el.offsetParent !== null);
            const first = items[0], last = items[items.length - 1];
            if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
            else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
          }
        }}>
        {phone ? <button type="button" class="btn btn-quiet mobile-nav-close" onClick={() => setNavOpen(false)}><Icon name="x" />{tr("关闭导航")}</button> : null}
        <Sidebar collapsed={collapsed} />
      </div>
      <main id="main" class="workspace" tabIndex={-1} inert={phone && navOpen || undefined}>
        <WorkspaceTabs />
        <div class="workspace-panel" id="workspace-panel" role="tabpanel" aria-labelledby={`workspace-tab-${tabId}`}>
          <Page key={`${s.sessionEpoch}:${tabId}`} />
        </div>
      </main>
      {showOnboarding ? <Onboarding onClose={() => setObActive(false)} /> : null}
      <DialogHost />
      <PaletteSlot />
      <MenuHost />
      <ToastHost />
    </div>
  );
}
