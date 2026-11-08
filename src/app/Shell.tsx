import { tr, tx } from "../lib/i18n";
import { useStore } from "../lib/store";
import { ACTIVE_STATES, ROOT_ID } from "../lib/contract-proposals";
import { formatBytes, formatSpeed } from "../lib/format";
import { app, gate, signedIn } from "../state/app";
import { dropTarget } from "../components/FileTable";
import { sendTo } from "../pages/fileActions";
import { requestRefresh } from "../pages/common";
import { navigate, route, type Route } from "../state/route";
import { transfers } from "../state/transfers";
import { openMenu, toast } from "../state/ui";
import { prefs, setPref, togglePin } from "../state/prefs";
import { LogoMark } from "../components/Brand";
import { Icon } from "../components/Icon";
import { openMockPanel } from "../pages/lazy";

interface NavDef {
  id: string;
  label: string;
  icon: string;
  to: Route;
  key?: string;
}

const LIBRARY: NavDef[] = [
  { id: "recent", get label() { return tr("最近"); }, icon: "clock", to: { page: "recent" }, key: "3" },
  { id: "starred", get label() { return tr("星标"); }, icon: "star", to: { page: "starred" }, key: "4" },
  { id: "shares", get label() { return tr("我的分享"); }, icon: "share", to: { page: "shares" }, key: "5" },
  { id: "trash", get label() { return tr("回收站"); }, icon: "trash", to: { page: "trash" }, key: "6" },
];
const TOOLS: NavDef[] = [
  { id: "sync", get label() { return tr("自动同步"); }, icon: "sync", to: { page: "sync" }, key: "7" },
  { id: "mount", get label() { return tr("挂载"); }, icon: "drive", to: { page: "mount" }, key: "8" },
];

/** Ctrl+<n> targets, shared with the global shortcut handler. */
export const NAV_KEYS: { key: string; to: Route }[] = [
  { key: "1", to: { page: "files", folder: ROOT_ID } },
  { key: "2", to: { page: "transfers", tab: "active" } },
  ...[...LIBRARY, ...TOOLS].map((n) => ({ key: n.key!, to: n.to })),
];

function NavItem({ label, icon, active, onClick, collapsed, title, onContext, badge, dropInto }: { label: string; icon: string; active: boolean; onClick: () => void; collapsed: boolean; title?: string; onContext?: (e: MouseEvent) => void; badge?: number; dropInto?: string }) {
  return (
    <li>
      <a
        {...(dropInto && gate("move").ok ? dropTarget(dropInto, (op, entries, dest) => void sendTo(op, entries, dest, requestRefresh)) : {})}
        href="#"
        class={`nav-item${active ? " active" : ""}`}
        aria-label={label}
        aria-current={active ? "page" : undefined}
        title={collapsed ? label : title}
        onClick={(e) => {
          e.preventDefault();
          onClick();
        }}
        onContextMenu={onContext ? (e) => { e.preventDefault(); onContext(e as unknown as MouseEvent); } : undefined}
      >
        <Icon name={icon} filled={active} />
        <span class="nav-label" dir="auto">{label}</span>
        {badge ? <span class="nav-badge">{badge}</span> : null}
      </a>
    </li>
  );
}

function useTransferActivity() {
  return useStore(transfers, (t) => {
    let n = 0;
    let down = 0;
    let up = 0;
    let failed = 0;
    for (const task of t.tasks.values()) {
      if (task.state === "failed") failed++;
      if (!ACTIVE_STATES.has(task.state)) continue;
      n++;
      if (task.state === "running" && typeof task.speed_bytes_per_sec === "number") {
        if (task.kind === "upload") up += task.speed_bytes_per_sec;
        else if (task.kind === "download") down += task.speed_bytes_per_sec;
      }
    }
    return `${n}|${Math.round(down)}|${Math.round(up)}|${failed}`;
  });
}

export function Sidebar({ collapsed }: { collapsed: boolean }) {
  const r = useStore(route, (s) => s.current);
  const s = useStore(app, (x) => x);
  const pins = useStore(prefs, (p) => p.pins);
  const [n, down, , failed] = useTransferActivity().split("|").map(Number);
  const limited = (s.settings?.bandwidth_limit_kib_s ?? 0) > 0;

  const onTransfers = r.page === "transfers" || r.page === "cloud";
  const currentFolder = r.page === "files" ? r.folder : null;
  const pinActive = currentFolder !== null && pins.some((p) => p.id === currentFolder);
  const sub = down > 0 ? `↓ ${formatSpeed(down).replace(" ", "")}` : failed ? tx("{0} 项需处理", failed) : n ? tx("{0} 项进行中", n) : limited ? tr("已限速") : "";

  const daily = s.quota?.items.find((i) => i.dimension === "downstream_daily");
  const monthly = s.quota?.items.find((i) => i.dimension === "downstream_monthly");
  const q = daily ?? monthly;
  const acctLine = s.auth?.state === "demo" ? tr("离线模拟 · 无真实账号") : !signedIn(s)
    ? s.auth?.state === "expired" ? tr("登录已过期") : tr("未登录")
    : q && q.remaining && q.exactness !== "unavailable"
      ? tx("{0}剩余 {1}", q.dimension === "downstream_daily" ? tr("今日") : tr("本月"), formatBytes(q.remaining))
      : s.auth?.account?.tier === "premium" ? "Premium" : s.auth?.account?.tier === "free" ? "Free" : tr("账号类型未提供");
  const name = s.auth?.state === "demo" ? tr("未连接 PikPak") : s.auth?.account?.display_name ?? (s.auth?.state === "expired" ? tr("登录已过期") : tr("未登录"));
  const demo = s.runtime === "mock";

  return (
    <nav class={`sidebar${collapsed ? " collapsed" : ""}`} aria-label={tr("主导航")}>
      <div class="sb-head" data-tauri-drag-region>
        <LogoMark size={28} />
        <span class="sb-brand">NekoPak</span>
        {demo ? (
          <button type="button" class="sb-flavor" onClick={() => void openMockPanel()} title={tr("演示模式：使用本地样例数据，不连接 PikPak。点击切换演示场景。")}>{tr("演示")}</button>
        ) : null}
        <button type="button" class="btn btn-quiet btn-icon sb-toggle" aria-label={collapsed ? tr("展开侧栏（Ctrl+B）") : tr("收起侧栏（Ctrl+B）")} title={collapsed ? tr("展开侧栏（Ctrl+B）") : tr("收起侧栏（Ctrl+B）")}
          onClick={() => setPref("sidebarCollapsed", !collapsed)}>
          <Icon name={collapsed ? "menu" : "menuOpen"} />
        </button>
      </div>

      <button type="button" class={`dl-btn${onTransfers ? " on" : ""}${failed && !down ? " alert" : ""}${limited ? " limited" : ""}`}
        aria-current={onTransfers ? "page" : undefined}
        aria-label={tx("下载{0}", sub ? `，${sub}` : "")}
        title={tx("下载（Ctrl+2）{0}", limited ? "，已开启限速" : "")}
        onClick={() => navigate({ page: "transfers", tab: "active" })}>
        <Icon name="download" filled={onTransfers} size={20} />
        <span class="dl-text">
          <span class="dl-label">{tr("下载")}</span>
          {sub ? <span class="dl-sub tabular" aria-live="off">{sub}</span> : null}
        </span>
        {n ? <span class="dl-count" aria-label={tx("{0} 项进行中", n)}>{n > 99 ? "99+" : n}</span> : null}
      </button>

      <div class="sb-scroll">
        <p class="sb-section">{tr("快速访问")}</p>
        <ul>
          <NavItem label={tr("网盘")} icon="cloud" collapsed={collapsed} title="Ctrl+1" dropInto={ROOT_ID}
            active={(r.page === "files" && !pinActive) || r.page === "search"}
            onClick={() => navigate({ page: "files", folder: ROOT_ID })} />
          {pins.map((p) => (
            <NavItem key={p.id} label={p.name} icon="folder" collapsed={collapsed} dropInto={p.id}
              active={currentFolder === p.id}
              onClick={() => navigate({ page: "files", folder: p.id })}
              onContext={(e) => openMenu(e.clientX, e.clientY, [
                { id: "open", label: tr("打开"), icon: "folder", run: () => navigate({ page: "files", folder: p.id }) },
                { id: "unpin", label: tr("取消固定"), icon: "unpin", separatorBefore: true, run: () => { togglePin(p); toast(tx("已从快速访问移除“{0}”", p.name), "success"); } },
              ], p.name)} />
          ))}
        </ul>

        <p class="sb-section">{tr("库")}</p>
        <ul>
          {LIBRARY.map((d) => <NavItem key={d.id} label={d.label} icon={d.icon} collapsed={collapsed} title={`Ctrl+${d.key}`} active={r.page === d.to.page} onClick={() => navigate(d.to)} />)}
        </ul>
        <p class="sb-section">{tr("工具")}</p>
        <ul>
          {TOOLS.map((d) => <NavItem key={d.id} label={d.label} icon={d.icon} collapsed={collapsed} title={`Ctrl+${d.key}`} active={r.page === d.to.page} onClick={() => navigate(d.to)} />)}
          <NavItem label={tr("本地文件夹")} icon="folder" collapsed={collapsed} active={r.page === "local"} onClick={() => navigate({page:"local"})}/>
          <NavItem label={tr("引用归档")} icon="archive" collapsed={collapsed} active={r.page === "references"} onClick={() => navigate({page:"references"})}/>
        </ul>
      </div>

      <div class="sb-account">
        <button type="button" class={`acct${r.page === "settings" && r.section === "account" ? " active" : ""}`} onClick={() => navigate({ page: "settings", section: "account" })} title={tr("账号与授权")}>
          <span class={`avatar state-${s.auth?.state ?? "unknown"}`} aria-hidden="true">
            {signedIn(s) ? (name.trim()[0] ?? "N").toUpperCase() : <Icon name="user" size={20} />}
          </span>
          <span class="acct-text">
            <span class="acct-name" dir="auto">{name}</span>
            <span class={`acct-sub${s.quota?.items.some((i) => i.state === "exhausted") ? " warn" : ""}`}>{acctLine}</span>
          </span>
        </button>
        <button type="button" class={`btn btn-quiet btn-icon${r.page === "settings" || r.page === "about" ? " on" : ""}`} aria-label={tr("设置（Ctrl+,）")} title={tr("设置（Ctrl+,）")}
          onClick={() => navigate({ page: "settings", section: "general" })}>
          <Icon name="settings" filled={r.page === "settings" || r.page === "about"} />
        </button>
      </div>
    </nav>
  );
}
