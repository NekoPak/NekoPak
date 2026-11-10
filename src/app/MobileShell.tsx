import type { ComponentChildren } from "preact";
import { useStore } from "../lib/store";
import { tr } from "../lib/i18n";
import { app, signedIn } from "../state/app";
import { navigate, route, type Route } from "../state/route";
import { Icon } from "../components/Icon";
import { prefs } from "../state/prefs";
import { formatBytes } from "../lib/format";
import { dialogs, menu } from "../state/ui";
import { palette } from "../state/palette";
import { useKeyboardVisible } from "../lib/mobile";

export function MobileShell({ children }: { children: ComponentChildren }) {
  const keyboard = useKeyboardVisible();
  const menuOpen=useStore(menu,s=>s.open), dialogOpen=useStore(dialogs,s=>s.stack.length>0), paletteOpen=useStore(palette,s=>s.open);
  const blocked=menuOpen||dialogOpen||paletteOpen;
  const page = useStore(route, s => s.current.page);
  const active = page === "transfers" || page === "cloud" ? "transfers" : ["me", "settings", "about", "recent", "starred", "shares", "trash", "references"].includes(page) ? "me" : "files";
  return <div class="mobile-app">
    <main id="main" class="mobile-workspace" inert={blocked||undefined}>{children}</main>
    <nav class="mobile-bottom-nav" aria-label={tr("主导航")} hidden={keyboard} inert={blocked||keyboard||undefined}>
      {[
        { id: "files", label: tr("文件"), icon: "folder", to: { page: "files", folder: "root" } as Route },
        { id: "transfers", label: tr("传输"), icon: "transfer", to: { page: "transfers", tab: "active" } as Route },
        { id: "me", label: tr("我的"), icon: "user", to: { page: "me" } as Route },
      ].map(item => <button key={item.id} type="button" class={active === item.id ? "active" : ""} aria-current={active === item.id ? "page" : undefined} onClick={() => navigate(item.to)}><span class="mobile-nav-icon"><Icon name={item.icon} filled={active === item.id} size={24}/></span><span>{item.label}</span></button>)}
    </nav>
  </div>;
}

export function MobileMePage() {
  const state = useStore(app, s => s);
  const pins = useStore(prefs, p => p.pins);
  const name = state.auth?.account?.display_name ?? tr("登录 PikPak");
  const quota = state.quota?.items.find(item => item.dimension === "storage");
  const row = (label: string, icon: string, to: Route, detail?: string) => <button type="button" class="mobile-setting-link" onClick={() => navigate(to)}><Icon name={icon}/><span>{label}</span>{detail ? <small>{detail}</small> : null}<Icon name="chevronRight" size={18}/></button>;
  return <section class="mobile-me">
    <header class="mobile-page-header"><h1>{tr("我的")}</h1></header>
    <div class="mobile-me-scroll">
      <button type="button" class="mobile-account" onClick={() => navigate({page:"settings",section:"account"})}>
        <span class="avatar"><Icon name="user" size={28}/></span><span><strong>{name}</strong><small>{signedIn(state) ? state.auth?.account?.tier === "premium" ? "Premium" : tr("账号与授权") : tr("登录后查看")}</small></span><Icon name="chevronRight"/>
      </button>
      {quota && quota.used ? <button class="mobile-quota" type="button" onClick={() => navigate({page:"settings",section:"quota"})}><span>{tr("空间")}</span><strong>{formatBytes(quota.used)}{quota.limit ? ` / ${formatBytes(quota.limit)}` : ""}</strong><Icon name="chevronRight" size={18}/></button> : null}
      <div class="mobile-settings-group" aria-label={tr("库")}>
        {row(tr("最近"),"clock",{page:"recent"})}{row(tr("星标"),"star",{page:"starred"})}{row(tr("我的分享"),"share",{page:"shares"})}{row(tr("回收站"),"trash",{page:"trash"})}
      </div>
      {pins.length ? <div class="mobile-settings-group" aria-label={tr("快速访问")}>{pins.map(pin => row(pin.name,"folder",{page:"files",folder:pin.id}))}</div> : null}
      <div class="mobile-settings-group" aria-label={tr("设置")}>
        {row(tr("外观"),"palette",{page:"settings",section:"general"})}{row(tr("界面语言"),"label",{page:"settings",section:"language"})}{row(tr("下载与传输"),"download",{page:"settings",section:"transfers"})}{row(tr("网络"),"network",{page:"settings",section:"network"})}{row(tr("配额"),"quota",{page:"settings",section:"quota"})}{row(tr("关于"),"info",{page:"settings",section:"about"})}
      </div>
      {state.app?.feature_flags.reference_archive ? <div class="mobile-settings-group" aria-label={tr("工具")}>{row(tr("引用归档"),"archive",{page:"references"})}</div> : null}
      <p class="mobile-version">NekoPak · {state.app?.version} · Pre-alpha</p>
    </div>
  </section>;
}
