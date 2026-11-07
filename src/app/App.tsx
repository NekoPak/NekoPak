import { useEffect, useState } from "preact/hooks";
import { useStore } from "../lib/store";
import { app, boot } from "../state/app";
import { initRouter, navigate, route } from "../state/route";
import { loadSnapshot } from "../state/transfers";
import { dialogs, focusRequests, menu } from "../state/ui";
import { applyPrefs, prefs, setPref } from "../state/prefs";
import { DialogHost, MenuHost, ToastHost } from "../components/hosts";
import { LogoMark } from "../components/Brand";
import { ErrorPanel } from "../components/ui";
import { showShortcuts } from "../components/dialogs";
import { NAV_KEYS, Sidebar } from "./Shell";
import { FilesPage } from "../pages/Files";
import { RecentPage, SearchPage, TrashPage } from "../pages/Lists";
import { TransfersPage } from "../pages/Transfers";
import { SyncPage } from "../pages/Sync";
import { MountPage } from "../pages/Mount";
import { SettingsPage } from "../pages/Settings";
import { Onboarding } from "../pages/Onboarding";
import { requestRefresh } from "../pages/common";

function isTyping(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (el as HTMLElement).isContentEditable;
}

function useGlobalShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialogs.get().stack.length || menu.get().open) return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && !e.shiftKey && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        const pg = route.get().current.page;
        if (pg !== "files" && pg !== "search") navigate({ page: "files", folder: "root" });
        // Let the files page mount before asking it to open its search field.
        setTimeout(() => focusRequests.set((s) => ({ search: s.search + 1 })), pg === "files" || pg === "search" ? 0 : 60);
        return;
      }
      if (ctrl && !e.shiftKey && /^[1-6]$/.test(e.key)) {
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
        history.back();
        return;
      }
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        history.forward();
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
      if (e.button === 3) history.back();
      if (e.button === 4) history.forward();
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
    case "settings": return <SettingsPage section={r.section} />;
    case "about": return <SettingsPage section="about" />;
  }
}

export function App() {
  const s = useStore(app, (x) => ({ boot: x.boot, err: x.bootError, accepted: x.disclaimerAccepted, auth: x.auth?.state }));
  const collapsedPref = useStore(prefs, (p) => p.sidebarCollapsed);
  const [narrow, setNarrow] = useState(() => typeof window !== "undefined" && window.innerWidth < 1000);
  // Decided once when boot completes: first run, or signed out at launch.
  const [obActive, setObActive] = useState<boolean | null>(null);
  useGlobalShortcuts();

  useEffect(() => {
    applyPrefs();
    initRouter();
    void boot();
    const mq = window.matchMedia("(max-width: 999px)");
    const onChange = () => setNarrow(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    if (s.boot === "ready" && obActive === null) setObActive(!s.accepted || s.auth === "signed_out");
  }, [s.boot, s.auth]);

  if (s.boot === "loading") {
    return (
      <div class="boot" role="status">
        <LogoMark size={40} />
        <span>正在启动 NekoPak…</span>
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
  const collapsed = narrow || collapsedPref;

  return (
    <div class={`app${collapsed ? " rail" : ""}`}>
      <a class="skip-link" href="#main">跳到主要内容</a>
      <Sidebar collapsed={collapsed} />
      <main id="main" class="workspace" tabIndex={-1}>
        <Page />
      </main>
      {showOnboarding ? <Onboarding onClose={() => setObActive(false)} /> : null}
      <DialogHost />
      <MenuHost />
      <ToastHost />
    </div>
  );
}
