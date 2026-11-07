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
import { app } from "../state/app";
import { navigate } from "../state/route";
import { closeSide, side } from "../state/prefs";
import { Icon } from "./Icon";
import { Btn, openOfficial } from "./ui";
import { toast } from "../state/ui";

type WinApi = { minimize(): Promise<void>; toggleMaximize(): Promise<void>; close(): Promise<void>; isMaximized(): Promise<boolean> };

async function currentWindow(): Promise<WinApi | null> {
  if (!inTauri()) return null;
  const mod = await import("@tauri-apps/api/window");
  return mod.getCurrentWindow() as unknown as WinApi;
}

/** Min / max / close. Only functional inside the desktop app; in a browser preview they are disabled. */
export function WindowControls() {
  const desktop = inTauri();
  const [max, setMax] = useState(false);
  useEffect(() => {
    if (!desktop) return;
    void currentWindow().then((w) => w?.isMaximized().then(setMax).catch(() => undefined));
  }, [desktop]);
  const reason = "浏览器预览中不可用，桌面应用中可用";
  const act = (fn: (w: WinApi) => Promise<unknown>) => () => void currentWindow().then((w) => w && fn(w)).catch(() => toast("窗口操作失败，请重试。", "error"));
  return (
    <div class="win-controls" role="group" aria-label="窗口">
      <button type="button" class="win-btn" aria-label="最小化" title={desktop ? "最小化" : reason} aria-disabled={desktop ? undefined : "true"} onClick={desktop ? act((w) => w.minimize()) : undefined}>
        <Icon name="minimize" size={18} />
      </button>
      <button type="button" class="win-btn" aria-label={max ? "还原" : "最大化"} title={desktop ? (max ? "还原" : "最大化") : reason} aria-disabled={desktop ? undefined : "true"}
        onClick={desktop ? act(async (w) => { await w.toggleMaximize(); setMax(await w.isMaximized()); }) : undefined}>
        <Icon name="maximize" size={16} />
      </button>
      <button type="button" class="win-btn win-close" aria-label="关闭" title={desktop ? "关闭" : reason} aria-disabled={desktop ? undefined : "true"} onClick={desktop ? act((w) => w.close()) : undefined}>
        <Icon name="x" size={18} />
      </button>
    </div>
  );
}

function Banners() {
  const s = useStore(app, (x) => x);
  const out: preact.JSX.Element[] = [];
  out.push(<div class="banner" role="status"><Icon name="info" /><span>{s.runtime === "mock" ? "浏览器界面演示：文件、账号和任务均为测试样本，未连接 PikPak。" : s.app?.mode === "mock" ? "离线界面预览：尚未连接 PikPak。文件与任务均为演示数据，不会下载文件。" : "官方 CLI 只读预览：仅文件浏览和元数据可用；下载、片段、同步与挂载尚未启用。"}</span></div>);
  if (s.auth?.state === "expired") {
    out.push(
      <div class="banner banner-warn" role="alert">
        <Icon name="user" />
        <span>登录已过期。文件列表与任务历史已保留，重新授权后继续。</span>
        <Btn size="sm" tone="primary" label="重新授权" onClick={() => navigate({ page: "settings", section: "account" })} />
      </div>,
    );
  }
  const exhausted = s.quota?.items.filter((i) => i.state === "exhausted") ?? [];
  if (exhausted.length) {
    out.push(
      <div class="banner banner-error" role="alert">
        <Icon name="quota" />
        <span>
          Connected Apps 配额已用尽：{exhausted.map((q) => (q.dimension === "downstream_daily" ? "每日下行" : q.dimension === "cloud_download_monthly" ? "每月云下载" : q.dimension === "upload_monthly" ? "每月上传" : "每月下行")).join("、")}。
          新的下载会被拒绝，文件列表与任务历史仍可查看。切换后端不会恢复配额。
        </span>
        <Btn size="sm" tone="tonal" label="查看配额" onClick={() => navigate({ page: "settings", section: "quota" })} />
        <Btn size="sm" tone="ghost" icon="external" label="官方应用" onClick={() => void openOfficial()} />
      </div>,
    );
  }
  if (s.quotaError?.code === "TRANSIENT_NETWORK") {
    out.push(
      <div class="banner banner-warn" role="status">
        <Icon name="cloud" />
        <span>无法连接远端服务：{s.quotaError.message}。进行中的传输会按退避策略重试。</span>
      </div>,
    );
  }
  if (s.app && s.app.storage.status !== "ok") {
    out.push(
      <div class="banner banner-error" role="alert">
        <Icon name="alertCircle" />
        <span>任务数据库{s.app.storage.status === "corrupt_readonly" ? "已损坏，已切换为只读，未覆盖任何记录" : "状态异常"}。{s.app.storage.message ?? ""}</span>
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
      if (e.key === "Escape" && document.activeElement?.closest(".side-island")) closeSide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panel?.key]);
  if (!panel) return null;
  return (
    <aside class="side-island" aria-labelledby="side-title" key={panel.key}>
      <header class="side-head">
        <h2 id="side-title">{panel.title}</h2>
        <Btn tone="quiet" iconOnly icon="x" label="关闭面板（Esc）" onClick={closeSide} />
      </header>
      <div class="side-body">{panel.render(closeSide)}</div>
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
  return (
    <section class={`pf${p.class ? ` ${p.class}` : ""}`} aria-labelledby="page-title">
      <header class="pf-caption" data-tauri-drag-region>
        <div class="pf-caption-main">{p.caption}</div>
        <div class="pf-drag" data-tauri-drag-region />
        {p.captionEnd ? <div class="pf-caption-end">{p.captionEnd}</div> : null}
        <WindowControls />
      </header>
      {p.commands ? <div class="pf-commands" role="toolbar" aria-label={`${p.label}操作`}>{p.commands}</div> : null}
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
