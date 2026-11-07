import { useStore } from "../lib/store";
import { ACTIVE_STATES, ROOT_ID } from "../lib/contract-proposals";
import { formatBytes, formatSpeed } from "../lib/format";
import { app, signedIn } from "../state/app";
import { navigate, route, type Route } from "../state/route";
import { transfers } from "../state/transfers";
import { openMenu, toast } from "../state/ui";
import { prefs, setPref, togglePin } from "../state/prefs";
import { LogoMark } from "../components/Brand";
import { Icon } from "../components/Icon";
import { openMockPanel } from "../pages/DevPanel";

interface NavDef {
  id: string;
  label: string;
  icon: string;
  to: Route;
  key?: string;
}

const LIBRARY: NavDef[] = [
  { id: "recent", label: "最近", icon: "clock", to: { page: "recent" }, key: "3" },
  { id: "trash", label: "回收站", icon: "trash", to: { page: "trash" }, key: "4" },
];
const TOOLS: NavDef[] = [
  { id: "sync", label: "自动同步", icon: "sync", to: { page: "sync" }, key: "5" },
  { id: "mount", label: "挂载", icon: "drive", to: { page: "mount" }, key: "6" },
];

/** Ctrl+<n> targets, shared with the global shortcut handler. */
export const NAV_KEYS: { key: string; to: Route }[] = [
  { key: "1", to: { page: "files", folder: ROOT_ID } },
  { key: "2", to: { page: "transfers", tab: "active" } },
  ...[...LIBRARY, ...TOOLS].map((n) => ({ key: n.key!, to: n.to })),
];

function NavItem({ label, icon, active, onClick, collapsed, title, onContext, badge, subtitle }: { label: string; icon: string; active: boolean; onClick: () => void; collapsed: boolean; title?: string; onContext?: (e: MouseEvent) => void; badge?: number; subtitle?: string }) {
  return (
    <li>
      <a
        href="#"
        class={`nav-item${active ? " active" : ""}${subtitle ? " has-subtitle" : ""}`}
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
        <span class="nav-label" dir="auto">{label}{subtitle ? <span class="nav-subtitle" aria-hidden="true">{subtitle}</span> : null}</span>
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
    return `${n}|${Math.round(down / 4096) * 4096}|${Math.round(up / 4096) * 4096}|${failed}`;
  });
}

export function Sidebar({ collapsed }: { collapsed: boolean }) {
  const r = useStore(route, (s) => s.current);
  const s = useStore(app, (x) => x);
  const pins = useStore(prefs, (p) => p.pins);
  const [n, down, , failed] = useTransferActivity().split("|").map(Number);
  const limited = (s.settings?.bandwidth_limit_kib_s ?? 0) > 0;

  const currentFolder = r.page === "files" ? r.folder : null;
  const pinActive = currentFolder !== null && pins.some((p) => p.id === currentFolder);
  const second = down > 0 ? `↓ ${formatSpeed(down).replace(" ", "")}` : failed ? `${failed} 项需处理` : n ? `${n} 项进行中` : limited ? "已限速" : "";

  const daily = s.quota?.items.find((i) => i.dimension === "downstream_daily");
  const monthly = s.quota?.items.find((i) => i.dimension === "downstream_monthly");
  const q = daily ?? monthly;
  const acctLine = s.auth?.state === "demo" ? "离线模拟 · 无真实账号" : !signedIn(s)
    ? s.auth?.state === "expired" ? "登录已过期" : "未登录"
    : q && q.remaining && q.exactness !== "unavailable"
      ? `${q.dimension === "downstream_daily" ? "今日" : "本月"}剩余 ${formatBytes(q.remaining)}`
      : s.auth?.account?.tier === "premium" ? "Premium" : s.auth?.account?.tier === "free" ? "Free" : "账号类型未提供";
  const name = s.auth?.state === "demo" ? "未连接 PikPak" : s.auth?.account?.display_name ?? (s.auth?.state === "expired" ? "登录已过期" : "未登录");

  return (
    <nav class={`sidebar${collapsed ? " collapsed" : ""}`} aria-label="主导航">
      <div class="sb-head" data-tauri-drag-region>
        <LogoMark size={28} />
        <span class="sb-brand">NekoPak</span>
        <button type="button" class="sb-flavor" disabled={s.runtime !== "mock"} onClick={() => void openMockPanel()} title={s.runtime === "mock" ? "Mock 场景与故障注入" : "桌面模式由 Rust 核心提供状态"}>
          {s.runtime === "mock" ? "Mock" : "预览版"}
        </button>
        <button type="button" class="btn btn-quiet btn-icon sb-toggle" aria-label={collapsed ? "展开侧栏（Ctrl+B）" : "收起侧栏（Ctrl+B）"} title={collapsed ? "展开侧栏（Ctrl+B）" : "收起侧栏（Ctrl+B）"}
          onClick={() => setPref("sidebarCollapsed", !collapsed)}>
          <Icon name={collapsed ? "menu" : "menuOpen"} />
        </button>
      </div>

      <div class="sb-scroll">
        <ul class="sb-primary" aria-label="主要页面">
          <NavItem label="网盘" icon="cloud" collapsed={collapsed} title="网盘（Ctrl+1）"
            active={(r.page === "files" && !pinActive) || r.page === "search"}
            onClick={() => navigate({ page: "files", folder: ROOT_ID })} />
          <NavItem label="任务" icon="transfer" collapsed={collapsed} title={`任务（Ctrl+2）${second ? ` · ${second}` : ""}`}
            active={r.page === "transfers" || r.page === "cloud"} subtitle={second || undefined} badge={failed || n || undefined}
            onClick={() => navigate({ page: "transfers", tab: "active" })} />
        </ul>
        <p class="sb-section">快速访问</p>
        <ul>
          {pins.map((p) => (
            <NavItem key={p.id} label={p.name} icon="folder" collapsed={collapsed}
              active={currentFolder === p.id}
              onClick={() => navigate({ page: "files", folder: p.id })}
              onContext={(e) => openMenu(e.clientX, e.clientY, [
                { id: "open", label: "打开", icon: "folder", run: () => navigate({ page: "files", folder: p.id }) },
                { id: "unpin", label: "取消固定", icon: "unpin", separatorBefore: true, run: () => { togglePin(p); toast(`已从快速访问移除“${p.name}”`, "success"); } },
              ], p.name)} />
          ))}
        </ul>
        {pins.length === 0 && !collapsed ? <p class="sb-hint">在文件夹右键菜单中选择“固定到快速访问”。</p> : null}

        <p class="sb-section">库</p>
        <ul>
          {LIBRARY.map((d) => <NavItem key={d.id} label={d.label} icon={d.icon} collapsed={collapsed} title={`Ctrl+${d.key}`} active={r.page === d.to.page} onClick={() => navigate(d.to)} />)}
        </ul>
        <p class="sb-section">工具</p>
        <ul>
          {TOOLS.map((d) => <NavItem key={d.id} label={d.label} icon={d.icon} collapsed={collapsed} title={`Ctrl+${d.key}`} active={r.page === d.to.page} onClick={() => navigate(d.to)} />)}
        </ul>
      </div>

      <div class="sb-account">
        <button type="button" class={`acct${r.page === "settings" && r.section === "account" ? " active" : ""}`} onClick={() => navigate({ page: "settings", section: "account" })} title="账号与授权">
          <span class={`avatar state-${s.auth?.state ?? "unknown"}`} aria-hidden="true">
            {signedIn(s) ? (name.trim()[0] ?? "N").toUpperCase() : <Icon name="user" size={20} />}
          </span>
          <span class="acct-text">
            <span class="acct-name" dir="auto">{name}</span>
            <span class={`acct-sub${s.quota?.items.some((i) => i.state === "exhausted") ? " warn" : ""}`}>{acctLine}</span>
          </span>
        </button>
        <button type="button" class={`btn btn-quiet btn-icon${r.page === "settings" || r.page === "about" ? " on" : ""}`} aria-label="设置（Ctrl+,）" title="设置（Ctrl+,）"
          onClick={() => navigate({ page: "settings", section: "general" })}>
          <Icon name="settings" filled={r.page === "settings" || r.page === "about"} />
        </button>
      </div>
    </nav>
  );
}
