import { tr, tx } from "../lib/i18n";
/**
 * Command palette (Ctrl+K / Ctrl+Shift+P): jump to any page or folder and
 * run any command by typing a few letters. Folder hits come from search.
 */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { RemoteEntry } from "../lib/contract";
import { ROOT_ID } from "../lib/contract-proposals";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { palette } from "../state/palette";
import { score } from "../lib/fuzzy";
import { app, applyTheme, gate, signedIn, updateSettings } from "../state/app";
import { isCommandAvailable, runPageCommand, type PageCommand } from "../state/bus";
import { navigate, type Route, type SettingsSection } from "../state/route";
import { prefs, setPref } from "../state/prefs";
import { canUndo, undo, undoLog } from "../state/undo";
import { Icon } from "./Icon";
import { showShortcuts } from "./dialogs";


interface Item {
  id: string;
  group: string;
  label: string;
  icon: string;
  keys?: string;
  hint?: string;
  shortcut?: string;
  run: () => void;
}

const NAV: { label: string; icon: string; to: Route; keys: string; shortcut?: string }[] = [
  { get label() { return tr("网盘"); }, icon: "cloud", to: { page: "files", folder: ROOT_ID }, keys: "files drive wangpan", shortcut: "Ctrl+1" },
  { get label() { return tr("下载"); }, icon: "download", to: { page: "transfers", tab: "active" }, keys: "transfers tasks xiazai renwu", shortcut: "Ctrl+2" },
  { get label() { return tr("最近"); }, icon: "clock", to: { page: "recent" }, keys: "recent zuijin", shortcut: "Ctrl+3" },
  { get label() { return tr("星标"); }, icon: "star", to: { page: "starred" }, keys: "starred xingbiao", shortcut: "Ctrl+4" },
  { get label() { return tr("我的分享"); }, icon: "share", to: { page: "shares" }, keys: "shares fenxiang", shortcut: "Ctrl+5" },
  { get label() { return tr("回收站"); }, icon: "trash", to: { page: "trash" }, keys: "trash huishouzhan", shortcut: "Ctrl+6" },
  { get label() { return tr("自动同步"); }, icon: "sync", to: { page: "sync" }, keys: "sync tongbu", shortcut: "Ctrl+7" },
  { get label() { return tr("挂载"); }, icon: "drive", to: { page: "mount" }, keys: "mount guazai", shortcut: "Ctrl+8" },
];

const SETTINGS: { label: string; section: SettingsSection; icon: string; keys: string }[] = [
  { get label() { return tr("账号"); }, section: "account", icon: "user", keys: "account login zhanghao" },
  { get label() { return tr("外观"); }, section: "general", icon: "palette", keys: "appearance theme waiguan" },
  { get label() { return tr("下载与传输"); }, section: "transfers", icon: "download", keys: "download quality xiazai" },
  { get label() { return tr("网络"); }, section: "network", icon: "network", keys: "network proxy wangluo" },
  { get label() { return tr("配额"); }, section: "quota", icon: "quota", keys: "quota peie" },
  { get label() { return tr("快捷键"); }, section: "shortcuts", icon: "keyboard", keys: "shortcuts kuaijiejian" },
  { get label() { return tr("关于"); }, section: "about", icon: "info", keys: "about guanyu" },
];

const ACTIONS: { cmd: PageCommand; label: string; icon: string; keys: string; shortcut?: string }[] = [
  { cmd: "new-folder", get label() { return tr("新建文件夹"); }, icon: "folderPlus", keys: "mkdir new folder xinjian", shortcut: "Ctrl+Shift+N" },
  { cmd: "add-link", get label() { return tr("添加链接（云下载）"); }, icon: "bolt", keys: "magnet link cloud tianjia" },
  { cmd: "paste", get label() { return tr("粘贴"); }, icon: "paste", keys: "paste zhantie", shortcut: "Ctrl+V" },
  { cmd: "select-all", get label() { return tr("全选"); }, icon: "selectAll", keys: "select all quanxuan", shortcut: "Ctrl+A" },
  { cmd: "download-selection", get label() { return tr("下载所选"); }, icon: "download", keys: "download xiazai" },
  { cmd: "batch-rename", get label() { return tr("批量重命名"); }, icon: "findReplace", keys: "rename batch chongmingming" },
  { cmd: "find-duplicates", get label() { return tr("在当前文件夹查找重复"); }, icon: "duplicates", keys: "duplicates dedupe chongfu" },
  { cmd: "space", get label() { return tr("当前文件夹的空间分布"); }, icon: "space", keys: "space usage kongjian" },
  { cmd: "refresh", get label() { return tr("刷新"); }, icon: "refresh", keys: "refresh reload shuaxin", shortcut: "F5" },
];

function buildItems(): Item[] {
  const out: Item[] = [];
  for (const n of NAV) out.push({ id: `nav-${n.label}`, group: tr("跳转"), label: n.label, icon: n.icon, keys: n.keys, shortcut: n.shortcut, run: () => navigate(n.to) });
  for (const p of prefs.get().pins) out.push({ id: `pin-${p.id}`, group: tr("跳转"), label: p.name, icon: "folder", hint: tr("快速访问"), keys: "pin", run: () => navigate({ page: "files", folder: p.id }) });
  for (const a of ACTIONS) {
    if (!isCommandAvailable(a.cmd)) continue;
    out.push({ id: `cmd-${a.cmd}`, group: tr("操作"), label: a.label, icon: a.icon, keys: a.keys, shortcut: a.shortcut, run: () => void runPageCommand(a.cmd) });
  }
  const u = undoLog.get().entries[undoLog.get().entries.length - 1];
  if (u && canUndo()) out.push({ id: "undo", group: tr("操作"), label: tx("撤销{0}", u.label), icon: "undo", keys: "undo chexiao", shortcut: "Ctrl+Z", run: () => void undo() });
  const theme = app.get().settings?.theme ?? "system";
  const dark = theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  out.push(
    { id: "theme", group: tr("操作"), label: dark ? tr("切换到浅色主题") : tr("切换到深色主题"), icon: dark ? "sun" : "moon", keys: "theme dark light zhuti", run: () => { const t = dark ? "light" : "dark"; applyTheme(t); void updateSettings({ theme: t }).catch(() => undefined); } },
    { id: "view", group: tr("操作"), label: prefs.get().view === "list" ? tr("切换到网格视图") : tr("切换到列表视图"), icon: prefs.get().view === "list" ? "grid" : "rows", keys: "view grid list shitu", run: () => setPref("view", prefs.get().view === "list" ? "grid" : "list") },
    { id: "density", group: tr("操作"), label: prefs.get().density === "compact" ? tr("使用舒适行距") : tr("使用紧凑行距"), icon: "rows", keys: "density compact hangju", run: () => setPref("density", prefs.get().density === "compact" ? "comfortable" : "compact") },
    { id: "sidebar", group: tr("操作"), label: prefs.get().sidebarCollapsed ? tr("展开侧栏") : tr("收起侧栏"), icon: "menuOpen", keys: "sidebar cebian", shortcut: "Ctrl+B", run: () => setPref("sidebarCollapsed", !prefs.get().sidebarCollapsed) },
    { id: "keys", group: tr("操作"), label: tr("查看全部快捷键"), icon: "keyboard", keys: "help shortcuts", shortcut: "F1", run: () => void showShortcuts() },
  );
  for (const x of SETTINGS) out.push({ id: `set-${x.section}`, group: tr("设置"), label: tx("设置：{0}", x.label), icon: x.icon, keys: `settings shezhi ${x.keys}`, run: () => navigate({ page: "settings", section: x.section }) });
  return out;
}

export function PaletteHost() {
  const st = useStore(palette, (s) => s);
  if (!st.open) return null;
  return <Palette seed={st.seed} close={() => palette.set({ open: false, seed: "" })} />;
}

function Palette({ seed, close }: { seed: string; close: () => void }) {
  const [q, setQ] = useState(seed);
  const [active, setActive] = useState(0);
  const [folders, setFolders] = useState<RemoteEntry[]>([]);
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const restore = useRef<Element | null>(document.activeElement);
  const base = useMemo(buildItems, []);

  useEffect(() => {
    input.current?.focus();
    return () => (restore.current as HTMLElement | null)?.focus?.();
  }, []);

  // Folder search (debounced; only when signed in and search is available).
  useEffect(() => {
    const text = q.trim();
    if (text.length < 1 || !signedIn() || !gate("search").ok) return setFolders([]);
    let alive = true;
    const t = setTimeout(() => {
      api.filesSearch(text).then((p) => alive && setFolders(p.items.filter((e) => e.kind === "folder").slice(0, 6)), () => alive && setFolders([]));
    }, 180);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [q]);

  const items = useMemo(() => {
    const text = q.trim();
    const ranked = base
      .map((it) => ({ it, s: Math.max(score(it.label, text), score(it.keys ?? "", text) * 0.8) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => (text ? b.s - a.s : 0))
      .map((x) => x.it);
    const folderItems: Item[] = folders.map((f) => ({ id: `f-${f.id}`, group: tr("文件夹"), label: f.name, icon: "folder", run: () => navigate({ page: "files", folder: f.id }) }));
    const searchItem: Item[] = text && gate("search").ok
      ? [{ id: "search", group: tr("文件夹"), label: tx("在云盘中搜索“{0}”", text), icon: "search", shortcut: "Enter", run: () => navigate({ page: "search", query: text }) }]
      : [];
    const all = text ? [...ranked, ...folderItems, ...searchItem] : ranked;
    const order: Item["group"][] = [tr("跳转"), tr("操作"), tr("文件夹"), tr("设置")];
    return text ? all : all.sort((a, b) => order.indexOf(a.group) - order.indexOf(b.group));
  }, [q, base, folders]);

  useEffect(() => setActive(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = (it?: Item) => {
    if (!it) return;
    close();
    it.run();
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(items.length - 1, a + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(0, a - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); choose(items[active]); }
    else if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "Tab") e.preventDefault();
  };

  let lastGroup = "";
  return (
    <div class="palette-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div class="palette" role="dialog" aria-modal="true" aria-label={tr("命令面板")}>
        <div class="palette-input">
          <Icon name="search" size={20} />
          <input ref={input} value={q} placeholder={tr("输入命令、页面或文件夹名")} aria-label={tr("搜索命令")} role="combobox"
            aria-expanded="true" aria-controls="palette-list" aria-activedescendant={items[active] ? `pi-${active}` : undefined}
            onInput={(e) => setQ((e.target as HTMLInputElement).value)} onKeyDown={(e) => onKey(e as unknown as KeyboardEvent)} />
          <kbd class="kbd">Esc</kbd>
        </div>
        <ul class="palette-list" id="palette-list" role="listbox" ref={listRef}>
          {items.length === 0 ? <li class="palette-empty">{tr("没有匹配的命令")}</li> : items.map((it, i) => {
            const head = it.group !== lastGroup ? it.group : null;
            lastGroup = it.group;
            return (
              <>
                {head ? <li class="palette-group" role="presentation">{head}</li> : null}
                <li id={`pi-${i}`} data-i={i} role="option" aria-selected={i === active ? "true" : "false"} class={`palette-item${i === active ? " on" : ""}`}
                  onMouseMove={() => setActive(i)} onClick={() => choose(it)}>
                  <Icon name={it.icon} size={20} />
                  <span class="ellipsis" dir="auto">{it.label}</span>
                  {it.hint ? <span class="dim">{it.hint}</span> : null}
                  {it.shortcut ? <kbd class="kbd">{it.shortcut}</kbd> : null}
                </li>
              </>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
