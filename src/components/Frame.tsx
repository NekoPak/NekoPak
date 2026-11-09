import { tr, resolvedLanguage, tx } from "../lib/i18n";
/**
 * Window frame: an integrated caption row (drag region + window controls),
 * an optional command row, the rounded content island ("canvas"), an
 * optional side island, and an optional footer row — all on the frame
 * colour. Lighter surfaces always sit on darker ones with rounded corners.
 */
import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { inTauri } from "../lib/transport/tauri";
import { useStore } from "../lib/store";
import { app, hostSupports, isMobileHost } from "../state/app";
import { navigate } from "../state/route";
import { closeSide, side } from "../state/prefs";
import { Icon } from "./Icon";
import { Btn } from "./ui";
import { toast } from "../state/ui";

type WinApi = { minimize(): Promise<void>; toggleMaximize(): Promise<void>; close(): Promise<void>; isMaximized(): Promise<boolean> };

async function currentWindow(): Promise<WinApi | null> {
  if (!inTauri()) return null;
  const mod = await import("@tauri-apps/api/window");
  return mod.getCurrentWindow() as unknown as WinApi;
}

/** Min / max / close. Only functional inside the desktop app; in a browser preview they are disabled. */
export function WindowControls() {
  const mobile = useStore(app, isMobileHost);
  const windowAvailable = useStore(app, s => hostSupports("desktop_window", s));
  const desktop = inTauri() && windowAvailable;
  const [max, setMax] = useState(false);
  useEffect(() => {
    if (!desktop) return;
    void currentWindow().then((w) => w?.isMaximized().then(setMax).catch(() => undefined));
  }, [desktop]);
  if (mobile) return null;
  const reason = tr("浏览器预览中不可用，桌面应用中可用");
  const act = (fn: (w: WinApi) => Promise<unknown>) => () => void currentWindow().then((w) => w && fn(w)).catch(() => toast(tr("窗口操作失败，请重试。"), "error"));
  return (
    <div class="win-controls" role="group" aria-label={tr("窗口")}>
      <button type="button" class="win-btn" aria-label={tr("最小化")} title={desktop ? tr("最小化") : reason} aria-disabled={desktop ? undefined : "true"} onClick={desktop ? act((w) => w.minimize()) : undefined}>
        <Icon name="minimize" size={18} />
      </button>
      <button type="button" class="win-btn" aria-label={max ? tr("还原") : tr("最大化")} title={desktop ? (max ? tr("还原") : tr("最大化")) : reason} aria-disabled={desktop ? undefined : "true"}
        onClick={desktop ? act(async (w) => { await w.toggleMaximize(); setMax(await w.isMaximized()); }) : undefined}>
        <Icon name="maximize" size={16} />
      </button>
      <button type="button" class="win-btn win-close" aria-label={tr("关闭")} title={desktop ? tr("关闭") : reason} aria-disabled={desktop ? undefined : "true"} onClick={desktop ? act((w) => w.close()) : undefined}>
        <Icon name="x" size={18} />
      </button>
    </div>
  );
}

function Banners() {
  const s = useStore(app, (x) => x);
  const out: preact.JSX.Element[] = [];
  if (s.runtime === "mock" || s.app?.mode === "mock") out.push(<div class="banner" role="status"><Icon name="info" /><span>{tr("离线界面预览：文件、账号和任务均为测试样本，未连接 PikPak。")}</span></div>);
  if (s.auth?.state === "expired") {
    out.push(
      <div class="banner banner-warn" role="alert">
        <Icon name="user" />
        <span>{tr("登录已过期，请重新登录。")}</span>
        <Btn size="sm" tone="primary" label={tr("重新登录")} onClick={() => navigate({ page: "settings", section: "account" })} />
      </div>,
    );
  }
  const exhausted = s.quota?.items.filter((i) => i.state === "exhausted") ?? [];
  if (exhausted.length) {
    out.push(
      <div class="banner banner-error" role="alert">
        <Icon name="quota" />
        <span>
          {resolvedLanguage()==="en" ? "Transfer quota exhausted. New downloads must wait until it resets." : <>{exhausted.map((q) => (q.dimension === "downstream_daily" ? tr("今日下载") : q.dimension === "cloud_download_monthly" ? tr("本月云下载") : q.dimension === "upload_monthly" ? tr("本月上传") : tr("本月下载"))).join("、")}{tr("额度已用完，重置前不能开始新的下载。")}</>}
        </span>
        <Btn size="sm" tone="tonal" label={tr("查看配额")} onClick={() => navigate({ page: "settings", section: "quota" })} />
      </div>,
    );
  }
  if (s.quotaError?.code === "TRANSIENT_NETWORK") {
    out.push(
      <div class="banner banner-warn" role="status">
        <Icon name="cloud" />
        <span>{tr("无法连接远端服务：")}{s.quotaError.message}{tr("。进行中的传输会按退避策略重试。")}</span>
      </div>,
    );
  }
  if (s.app && s.app.storage.status !== "ok") {
    out.push(
      <div class="banner banner-error" role="alert">
        <Icon name="alertCircle" />
        <span>{tr("任务数据库")}{s.app.storage.status === "corrupt_readonly" ? tr("已损坏，已切换为只读，未覆盖任何记录") : tr("状态异常")}。{s.app.storage.message ?? ""}</span>
      </div>,
    );
  }
  return out.length ? <div class="banners">{out}</div> : null;
}

function SideIsland() {
  const panel = useStore(side, (s) => s.panel);
  useEffect(() => {
    if (!panel) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && document.activeElement?.closest(".side-island")) void closeSide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panel?.key]);
  if (!panel) return null;
  return (
    <aside class="side-island" aria-labelledby="side-title" key={panel.key}>
      <header class="side-head">
        <h2 id="side-title">{panel.title}</h2>
        <Btn tone="quiet" iconOnly icon="x" label={tr("关闭面板（Esc）")} onClick={() => void closeSide()} />
      </header>
      <div class="side-body">{panel.render(() => void closeSide())}</div>
    </aside>
  );
}

export interface PageFrameProps {
  /** Left part of the caption row (title, address bar, tabs). */
  caption: ComponentChildren;
  /** Right part of the caption row before the window controls. */
  captionEnd?: ComponentChildren;
  /** Command row above the canvas. */
  commands?: ComponentChildren;
  footer?: ComponentChildren;
  /** Whether this page hosts the shared side island. */
  side?: boolean;
  label: string;
  class?: string;
  children: ComponentChildren;
}

export function PageFrame(p: PageFrameProps) {
  const mobile = useStore(app, isMobileHost);
  return (
    <section class={`pf${p.class ? ` ${p.class}` : ""}`} aria-labelledby="page-title">
      <header class="pf-caption" data-tauri-drag-region={mobile ? undefined : true}>
        <div class="pf-caption-main">{p.caption}</div>
        <div class="pf-drag" data-tauri-drag-region={mobile ? undefined : true} />
        {p.captionEnd ? <div class="pf-caption-end">{p.captionEnd}</div> : null}
        <WindowControls />
      </header>
      {p.commands ? <div class="pf-commands" role="toolbar" aria-label={tx("{0}操作", p.label)}>{p.commands}</div> : null}
      <div class="pf-body">
        <div class="canvas">
          <Banners />
          {p.children}
        </div>
        {p.side ? <SideIsland /> : null}
      </div>
      {p.footer ? <footer class="pf-foot">{p.footer}</footer> : null}
    </section>
  );
}

export function PageTitle({ children }: { children: ComponentChildren }) {
  return <h1 id="page-title" class="pf-title">{children}</h1>;
}
