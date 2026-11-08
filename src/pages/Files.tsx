import { tr, tx } from "../lib/i18n";
import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { formatBytes, addU64, groupDigits } from "../lib/format";
import { fileKind, KIND_LABEL, type FileKind } from "../lib/names";
import { useStore } from "../lib/store";
import { app, fileGate, signedIn } from "../state/app";
import { activeTab, navigate, navigateHistory, setTabTitle } from "../state/route";
import { focusRequests, openMenu, toast } from "../state/ui";
import { prefs, setPref, zoomView } from "../state/prefs";
import { clip } from "../state/clipboard";
import { onPageCommand, setAvailableCommands } from "../state/bus";
import { dropTarget, FileTable, sortEntries, useSelection, type SortKey, type SortSpec } from "../components/FileTable";
import { Icon } from "../components/Icon";
import { Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { PageFrame } from "../components/Frame";
import { newCloudTask, promptName, showProperties } from "../components/dialogs";
import {
  copyPaths, cutOrCopy, downloadEntries, entryMenu, moveOrCopy, mutate, openArchive, pasteInto, pathOf, renameEntry,
  sendTo, setStar, trashEntries, type ActionContext,
} from "./fileActions";
import { openBatchRename, openPlayer, openShareDialog } from "./lazy";
import { isStarred } from "../lib/contract-proposals";
import { useRefreshKey } from "./common";
import { onSessionReset } from "../state/session";
import { noteCreated } from "../state/transfers";

export interface Crumb { id: string; name: string }

const crumbCache = new Map<string, Crumb[]>();
const ROOT_CRUMB: Crumb = { id: ROOT_ID, get name() { return tr("网盘"); } };
const PAGE = 500;
/** Recently visited folders for the address-bar dropdown (session only). */
const recentFolders: Crumb[] = [];
onSessionReset(() => { crumbCache.clear(); recentFolders.length = 0; });

export function useDensity(): [number, "comfortable" | "compact", (d: "comfortable" | "compact") => void] {
  const d = useStore(prefs, (p) => p.density);
  return [d === "compact" ? 36 : 44, d, (x) => setPref("density", x)];
}

interface FolderState {
  items: RemoteEntry[];
  next?: string;
  phase: "loading" | "ready" | "error";
  error?: ClientError;
  loadingMore: boolean;
  moreError?: ClientError;
}

function useFolder(folder: string, epoch: number, refresh: number) {
  const [st, setSt] = useState<FolderState>({ items: [], phase: "loading", loadingMore: false });
  const token = useRef(0);
  const prevFolder = useRef(folder);
  const prevEpoch = useRef(epoch);

  const load = useCallback(async (keepCount = 0) => {
    const my = ++token.current;
    setSt((s) => ({ ...s, phase: keepCount ? s.phase : "loading", error: undefined, moreError: undefined, ...(keepCount ? {} : { items: [], next: undefined }) }));
    try {
      let page = await api.filesList(folder, undefined, PAGE);
      const items = [...page.items];
      while (page.next_cursor && items.length < keepCount) {
        page = await api.filesList(folder, page.next_cursor, PAGE);
        items.push(...page.items);
      }
      if (my !== token.current) return;
      setSt({ items, next: page.next_cursor, phase: "ready", loadingMore: false });
    } catch (e) {
      if (my !== token.current) return;
      setSt((s) => ({ ...s, phase: "error", error: errorOf(e), loadingMore: false }));
    }
  }, [folder]);

  useEffect(() => {
    const same = prevFolder.current === folder && prevEpoch.current === epoch;
    prevFolder.current = folder;
    prevEpoch.current = epoch;
    void load(same ? st.items.length : 0);
  }, [folder, epoch, refresh]);

  const loadMore = useCallback(async () => {
    const cursor = st.next;
    if (!cursor || st.loadingMore) return;
    const my = token.current;
    setSt((s) => ({ ...s, loadingMore: true, moreError: undefined }));
    try {
      const page = await api.filesList(folder, cursor, PAGE);
      if (my !== token.current) return;
      setSt((s) => ({ ...s, items: [...s.items, ...page.items], next: page.next_cursor, loadingMore: false }));
    } catch (e) {
      if (my !== token.current) return;
      setSt((s) => ({ ...s, loadingMore: false, moreError: errorOf(e) }));
    }
  }, [folder, st.next, st.loadingMore]);

  const loadAll = useCallback(async () => {
    let cursor = st.next;
    if (!cursor || st.loadingMore) return;
    const my = token.current;
    setSt((s) => ({ ...s, loadingMore: true }));
    const acc: RemoteEntry[] = [];
    const seen = new Set<string>();
    try {
      while (cursor) {
        if (seen.has(cursor)) throw { code: "CONFLICT", message: tr("服务返回重复分页，请刷新目录。"), retryable: false };
        seen.add(cursor);
        const page = await api.filesList(folder, cursor, PAGE);
        if (my !== token.current) return;
        acc.push(...page.items);
        cursor = page.next_cursor;
      }
      setSt((s) => ({ ...s, items: [...s.items, ...acc], next: undefined, loadingMore: false }));
    } catch (e) {
      if (my !== token.current) return;
      setSt((s) => ({ ...s, items: [...s.items, ...acc], next: cursor, loadingMore: false, moreError: errorOf(e) }));
    }
  }, [folder, st.next, st.loadingMore]);

  return { st, reload: () => void load(st.items.length), retry: () => void load(0), loadMore, loadAll };
}

function useCrumbs(folder: string, epoch: number): Crumb[] {
  const [crumbs, setCrumbs] = useState<Crumb[]>(() => crumbCache.get(folder) ?? [ROOT_CRUMB]);
  useEffect(() => {
    if (folder === ROOT_ID) {
      setCrumbs([ROOT_CRUMB]);
      return;
    }
    const cached = crumbCache.get(folder);
    if (cached) setCrumbs(cached);
    let alive = true;
    api.filesPath(folder).then(
      (chain) => {
        if (!alive) return;
        const c = [ROOT_CRUMB, ...chain.map((e) => ({ id: e.id, name: e.name }))];
        crumbCache.set(folder, c);
        setCrumbs(c);
      },
      () => {
        if (alive && !cached) setCrumbs([ROOT_CRUMB, { id: folder, name: "…" }]);
      },
    );
    return () => {
      alive = false;
    };
  }, [folder, epoch]);
  return crumbs;
}


/* ------------------------------------------------------------------ */
/* Caption: history buttons, address bar, inline search                */
/* ------------------------------------------------------------------ */

export function NavButtons({ up }: { up?: () => void }) {
  return (
    <div class="nav-btns">
      <Btn tone="quiet" iconOnly icon="arrowBack" label={tr("后退（Alt+←）")} onClick={() => navigateHistory(-1)} />
      <Btn tone="quiet" iconOnly icon="arrowForward" label={tr("前进（Alt+→）")} onClick={() => navigateHistory(1)} />
      <Btn tone="quiet" iconOnly icon="arrowUp" label={tr("上一级（Backspace）")} disabled={!up} reason={tr("已在根目录")} onClick={() => up?.()} />
    </div>
  );
}

function AddressBar({ crumbs, onSearch, onDrop }: { crumbs: Crumb[]; onSearch: () => void; onDrop?: (op: "move" | "copy", entries: RemoteEntry[], dest: string) => void }) {
  return (
    <div class="address">
      <nav class="crumbs" aria-label={tr("路径")}>
        {crumbs.map((c, i) => (
          <>
            {i > 0 ? <Icon name="chevronRight" size={18} class="crumb-sep" /> : null}
            {i === crumbs.length - 1 ? (
              <h1 id="page-title" class="crumb current" dir="auto" aria-current="location" title={c.name}>
                {i === 0 ? <Icon name="cloud" size={18} /> : null}
                <span>{c.name}</span>
              </h1>
            ) : (
              <a class="crumb" href={`#/files/${encodeURIComponent(c.id)}`} dir="auto" title={c.name}
                {...(onDrop ? dropTarget(c.id, onDrop) : {})}
                onClick={(e) => { e.preventDefault(); navigate({ page: "files", folder: c.id }); }}>
                {i === 0 ? <Icon name="cloud" size={18} /> : null}
                <span>{c.name}</span>
              </a>
            )}
          </>
        ))}
      </nav>
      <button type="button" class="address-drop" aria-label={tr("最近访问的位置")} title={tr("最近访问的位置")}
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).closest(".address")!.getBoundingClientRect();
          const list = recentFolders.filter((f) => f.id !== crumbs[crumbs.length - 1]?.id).slice(0, 8);
          openMenu(r.right - 280, r.bottom + 4, list.length
            ? list.map((f) => ({ id: f.id, label: f.name, icon: f.id === ROOT_ID ? "cloud" : "folder", run: () => navigate({ page: "files", folder: f.id }) }))
            : [{ id: "none", label: tr("还没有其他最近访问的位置"), disabled: true }], tr("最近访问"));
        }}>
        <Icon name="chevronDown" size={20} />
      </button>
      <span class="address-split" />
      <button type="button" class="address-search" aria-label={tr("搜索（Ctrl+F）")} title={tr("搜索（Ctrl+F）")} onClick={onSearch}>
        <Icon name="search" size={20} />
      </button>
    </div>
  );
}

export function CaptionSearch({ initial = "", onClose }: { initial?: string; onClose: () => void }) {
  const [q, setQ] = useState(initial);
  const input = useRef<HTMLInputElement>(null);
  const req = useStore(focusRequests, (s) => s.search);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [req]);
  return (
    <form class="address search-field" role="search" onSubmit={(e) => { e.preventDefault(); const v = q.trim(); if (v) navigate({ page: "search", query: v }); }}>
      <Icon name="search" size={20} class="search-lead" />
      <input ref={input} type="search" value={q} placeholder={tr("搜索全部云盘文件")} aria-label={tr("搜索云盘文件")}
        onInput={(e) => setQ((e.target as HTMLInputElement).value)}
        onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); onClose(); } }} />
      {/* Always present: clears text, or closes search when empty (touch-friendly). */}
      <button type="button" class="address-search" aria-label={q ? tr("清除") : tr("关闭搜索")} title={q ? tr("清除") : tr("关闭搜索（Esc）")}
        onClick={() => (q ? (setQ(""), input.current?.focus()) : onClose())}>
        <Icon name="x" size={20} />
      </button>
    </form>
  );
}

/* ------------------------------------------------------------------ */
/* Page                                                                */
/* ------------------------------------------------------------------ */

type KindFilter = "all" | FileKind;

const SORT_LABEL: Record<SortKey, string> = { get name() { return tr("名称"); }, get size() { return tr("大小"); }, get modified() { return tr("修改时间"); }, get kind() { return tr("类型"); } };

export function FilesPage({ folder }: { folder: string }) {
  const s = useStore(app, (x) => x);
  const gate = (op: Parameters<typeof fileGate>[0]) => fileGate(op, folder, s);
  const view = useStore(prefs, (p) => p.view);
  const gridSize = useStore(prefs, (p) => p.gridSize);
  const foldersFirst = useStore(prefs, (p) => p.foldersFirst);
  const clipCount = useStore(clip, (c) => (c.mode ? c.entries.length : 0));
  const clipMode = useStore(clip, (c) => c.mode);
  const refresh = useRefreshKey();
  const { st, reload, retry, loadMore, loadAll } = useFolder(folder, s.epoch, refresh);
  const crumbs = useCrumbs(folder, s.epoch);
  useEffect(() => { const last = crumbs[crumbs.length - 1]; if (folder !== ROOT_ID && last?.id === folder) setTabTitle({ page: "files", folder }, last.name); }, [crumbs, folder]);
  const sort = useStore(prefs, p => p.sort);
  const [kind, setKind] = useState<KindFilter>("all");
  const [rowH] = useDensity();
  const [searching, setSearching] = useState(false);
  const [uploading, setUploading] = useState(false);
  const searchReq = useStore(focusRequests, (x) => x.search);
  const firstReq = useRef(searchReq);

  useEffect(() => {
    setKind("all");
    const cur = crumbs[crumbs.length - 1];
    if (cur && cur.id === folder) {
      const i = recentFolders.findIndex((f) => f.id === cur.id);
      if (i >= 0) recentFolders.splice(i, 1);
      recentFolders.unshift(cur);
      recentFolders.length = Math.min(recentFolders.length, 12);
    }
  }, [folder, crumbs]);
  useEffect(() => {
    if (searchReq !== firstReq.current) setSearching(true);
  }, [searchReq]);

  const viewItems = useMemo(() => {
    const filtered = kind === "all" ? st.items : st.items.filter((e) => fileKind(e.name, e.kind, e.mime_type) === kind);
    return sortEntries(filtered, sort, foldersFirst);
  }, [st.items, sort, kind, foldersFirst]);

  const viewKey = `${activeTab().id}:${folder}`;
  const { sel, setSel, selected } = useSelection(viewItems, folder, viewKey, st.phase === "ready");
  const crumbPath = "/" + crumbs.slice(1).map((c) => c.name).join("/");

  const ctx: ActionContext = {
    pathFor: (e) => pathOf(crumbPath, e.name),
    siblings: () => st.items.map((e) => e.name),
    reload,
    folder,
    open: (e) => {
      if (e.kind === "folder") {
        crumbCache.set(e.id, [...crumbs, { id: e.id, name: e.name }]);
        navigate({ page: "files", folder: e.id });
      } else if (s.app?.feature_flags.media_session && ["video","audio","image"].includes(fileKind(e.name,e.kind,e.mime_type))) {
        openPlayer(e);
      } else if (/\.(zip|rar|7z)$/i.test(e.name) && gate("archive").ok) {
        openArchive(e);
      } else {
        void showProperties(e.id, pathOf(crumbPath, e.name), fileGate("download", e.id).ok ? (x) => void downloadEntries([x]) : undefined);
      }
    },
  };
  const onDropInto = gate("move").ok ? (op: "move" | "copy", entries: RemoteEntry[], dest: string) => void sendTo(op, entries, dest, reload) : undefined;
  const paste = () => void pasteInto(folder, reload);

  const goUp = crumbs.length > 1 ? () => navigate({ page: "files", folder: crumbs[crumbs.length - 2].id }) : undefined;

  const newFolder = async () => {
    const g = gate("mkdir");
    if (!g.ok) return void toast(g.reason ?? tr("不能新建文件夹"), "info");
    const name = await promptName(tr("新建文件夹"), { initial: tr("新建文件夹"), siblings: st.items.map((e) => e.name), confirmLabel: tr("新建") });
    if (name !== undefined) await mutate("mkdir", [], { parent_id: folder, name }, reload);
  };
  const upload = async (directory = false) => {
    const permission = gate("upload"); if (!permission.ok) return void toast(permission.reason ?? tr("暂不支持上传"), "info");
    if (uploading) return;
    if (directory && !s.app?.feature_flags.upload_directory) return void toast(tr("当前版本尚未支持目录上传"), "info");
    setUploading(true);
    try { const result = directory ? await api.uploadDirectoryPickAndStart(folder) : await api.uploadPickAndStart(folder); if (result) { if (result.task_ids.length) { noteCreated(); toast(tx("已加入上传队列 {0} 项", result.task_ids.length), "success"); } if (result.skipped.length) toast(result.skipped[0].reason, "warning"); if (directory) reload(); } }
    catch(e) { toast(errorOf(e).message, "error"); }
    finally { setUploading(false); }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === "N" || e.key === "n")) {
        e.preventDefault();
        void newFolder();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Palette / global shortcuts reach this page through the command bus.
  useEffect(() => {
    setAvailableCommands(folder.startsWith("local:") ? ["select-all", "refresh"] : ["new-folder", "add-link", "paste", "select-all", "batch-rename", "find-duplicates", "space", "refresh", "download-selection"]);
    const off = onPageCommand((cmd) => {
      switch (cmd) {
        case "new-folder": void newFolder(); return true;
        case "add-link": void newCloudTask(folder, crumbPath); return true;
        case "paste": paste(); return true;
        case "select-all": setSel((p) => ({ ...p, ids: new Set(viewItems.map((i) => i.id)) })); return true;
        case "batch-rename": openBatchRename(selected.length > 1 ? selected : viewItems, ctx); return true;
        case "find-duplicates": if (gate("duplicates").ok) navigate({ page: "duplicates", folder }); return true;
        case "space": if (gate("space_summary").ok) navigate({ page: "space", folder }); return true;
        case "refresh": reload(); return true;
        case "download-selection": if (selected.length) void downloadEntries(selected); else toast(tr("请先选择要下载的项目"), "info"); return true;
        default: return false;
      }
    });
    return () => {
      off();
      setAvailableCommands([]);
    };
  });

  const onSort = (key: SortKey) => {
    const next: SortSpec = sort.key === key ? { key, dir: sort.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "name" || key === "kind" ? "asc" : "desc" };
    setPref("sort", next);
  };

  const gMk = gate("mkdir");
  const gUp = gate("upload");
  const directoryUpload = gUp.ok && !!s.app?.feature_flags.upload_directory;
  const directoryUploadReason = !gUp.ok ? gUp.reason : tr("当前版本尚未支持目录上传");
  const gCloud = gate("cloud_task");
  const gDl = gate("download");
  const selFiles = selected.filter((e) => e.kind === "file");
  const selSize = addU64(selFiles.map((e) => e.size_bytes));
  const local = folder.startsWith("local:");
  const signed = local || signedIn(s);
  const menuAt = (e: MouseEvent) => (e.currentTarget as HTMLElement).getBoundingClientRect();

  const commandsLeft: ComponentChildren = selected.length === 0 ? (
    <div key="create" class="cmd-group">
      <Btn tone="quiet" iconOnly icon="home" label={tr("回到网盘根目录")} disabled={folder === ROOT_ID} reason={tr("已在根目录")} onClick={() => navigate({ page: "files", folder: ROOT_ID })} />
      <Btn tone="ghost" icon="plus" label={tr("新建")} trail="chevronDown" disabled={!gMk.ok && !gUp.ok} reason={gMk.reason} busy={uploading}
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, [
            { id: "mk", label: tr("新建文件夹"), icon: "folderPlus", shortcut: "Ctrl+Shift+N", disabled: !gMk.ok, reason: gMk.reason, run: () => void newFolder() },
            { id: "up", label: tr("上传文件"), icon: "upload", disabled: !gUp.ok || uploading, reason: gUp.reason, run: () => void upload() },
            { id: "up-dir", label: tr("上传文件夹"), icon: "folderPlus", disabled: !directoryUpload || uploading, reason: directoryUploadReason, run: () => void upload(true) },
          ], tr("新建"));
        }} />
      <span class="cmd-sep" />
      <Btn tone="ghost" icon="image" label={tr("媒体库")} disabled={!s.app?.feature_flags.media_session} reason={tr("当前宿主未提供媒体播放")} onClick={()=>navigate({page:"gallery",folder,mode:"images"})}/>
      <Btn tone="ghost" icon="sort" label={tx("按{0}", SORT_LABEL[sort.key])} trail={sort.dir === "asc" ? "arrowUp" : "chevronDown"} title={tr("排序方式")}
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, (Object.keys(SORT_LABEL) as SortKey[]).map((k) => ({
            id: k, label: `${SORT_LABEL[k]}${sort.key === k ? (sort.dir === "asc" ? tr("（升序）") : tr("（降序）")) : ""}`, icon: sort.key === k ? "check" : undefined, run: () => onSort(k),
          })), tr("排序"));
        }} />
      <Btn tone="ghost" icon="filter" label={kind === "all" ? tr("全部类型") : KIND_LABEL[kind]} trail="chevronDown" title={tr("按类型筛选")}
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, [{ id: "all", label: tr("全部类型"), icon: kind === "all" ? "check" : undefined, run: () => setKind("all") },
            ...(Object.keys(KIND_LABEL) as FileKind[]).map((k) => ({ id: k, label: KIND_LABEL[k], icon: kind === k ? "check" : undefined, run: () => setKind(k) }))], tr("类型"));
        }} />
      <Btn tone="ghost" icon="space" label={tr("整理")} trail="chevronDown" title={tr("查找重复、空间分布")}
        onClick={(e) => {
          const r = menuAt(e);
          const dup = gate("duplicates");
          const sp = gate("space_summary");
          openMenu(r.left, r.bottom + 4, [
            { id: "dups", label: tr("在此文件夹中查找重复"), icon: "duplicates", disabled: !dup.ok, reason: dup.reason, run: () => navigate({ page: "duplicates", folder }) },
            { id: "space", label: tr("查看空间分布"), icon: "space", disabled: !sp.ok, reason: sp.reason, run: () => navigate({ page: "space", folder }) },
            { id: "batch", label: tr("批量重命名本文件夹…"), icon: "findReplace", disabled: viewItems.length < 2 || !gate("rename").ok, reason: viewItems.length < 2 ? tr("至少需要两个项目") : gate("rename").reason, run: () => openBatchRename(viewItems, ctx) },
          ], tr("整理"));
        }} />
      <Btn tone="quiet" iconOnly icon="selectAll" label={tr("全选（Ctrl+A）")} disabled={!viewItems.length} reason={tr("没有可选择的项目")}
        onClick={() => setSel((p) => ({ ...p, ids: new Set(viewItems.map((i) => i.id)) }))} />
      {clipCount ? (
        <>
          <span class="cmd-sep" />
          <Btn tone="tonal" icon="paste" label={tx("粘贴 {0} 项", clipCount)} title={clipMode === "cut" ? tr("移动到这里（Ctrl+V）") : tr("复制到这里（Ctrl+V）")} disabled={!gate(clipMode === "cut" ? "move" : "copy").ok} reason={gate(clipMode === "cut" ? "move" : "copy").reason} onClick={paste} />
        </>
      ) : null}
    </div>
  ) : (
    <div key="selection" class="cmd-group cmd-selection" role="group" aria-label={tr("所选项目")}>
      <Btn tone="quiet" iconOnly icon="x" label={tr("退出多选（Esc）")} onClick={() => setSel((p) => ({ ...p, ids: new Set() }))} />
      <span class="sel-count" title={selFiles.length ? tx("所选文件共 {0}", formatBytes(selSize)) : undefined} aria-live="polite">{tr("已选")} {selected.length} {tr("项")}</span>
      <span class="cmd-sep" />
      <Btn tone="tonal" icon="download" label={selected.length > 1 ? tx("下载 {0} 项", selected.length) : tr("下载")}
        disabled={!gDl.ok || (selFiles.length === 0 && !gate("folder_download").ok)} reason={!gDl.ok ? gDl.reason : gate("folder_download").reason}
        onClick={() => void downloadEntries(selected)} />
      <span class="cmd-sep" />
      <Btn tone="quiet" iconOnly icon="folderMove" label={tr("移动到…")} disabled={!gate("move").ok} reason={gate("move").reason} onClick={() => void moveOrCopy("move", selected, ctx, folder)} />
      <Btn tone="quiet" iconOnly icon="folderCopy" label={tr("复制到…")} disabled={!gate("copy").ok} reason={gate("copy").reason} onClick={() => void moveOrCopy("copy", selected, ctx, folder)} />
      <Btn tone="quiet" iconOnly icon="pencil" label={selected.length > 1 ? tr("批量重命名（F2）") : tr("重命名（F2）")} disabled={!gate("rename").ok} reason={gate("rename").reason}
        onClick={() => (selected.length > 1 ? openBatchRename(selected, ctx) : void renameEntry(selected[0], ctx))} />
      <Btn tone="quiet" iconOnly icon="star" label={selected.every(isStarred) ? tr("取消星标（Ctrl+D）") : tr("添加星标（Ctrl+D）")} pressed={selected.every(isStarred)} disabled={!gate("star").ok} reason={gate("star").reason}
        onClick={() => void setStar(selected, !selected.every(isStarred), reload)} />
      <Btn tone="quiet" iconOnly icon="share" label={tr("分享…")} disabled={!gate("share").ok} reason={gate("share").reason} onClick={() => openShareDialog(selected)} />
      <Btn tone="quiet" iconOnly icon="link" label={tr("复制路径（Ctrl+Shift+C）")} onClick={() => void copyPaths(selected, ctx)} />
      <Btn tone="quiet" iconOnly icon="trash" label={tr("移至回收站（Delete）")} disabled={!gate("trash").ok} reason={gate("trash").reason} onClick={() => void trashEntries(selected, ctx)} />
    </div>
  );

  const commands = (
    <>
      {commandsLeft}
      <div class="cmd-group cmd-end">
        {st.phase === "ready" ? (
          <span class="cmd-meta" aria-live="polite">{st.next ? tx("已加载 {0} 项，还有更多", groupDigits(String(st.items.length))) : tx("{0} 项", groupDigits(String(st.items.length)))}</span>
        ) : null}
        <Btn tone="quiet" iconOnly icon="refresh" label={tr("刷新（F5）")} busy={st.phase === "loading"} onClick={reload} />
        <div class="view-toggle" role="group" aria-label={tr("视图")}>
          <Btn tone="quiet" iconOnly icon="rows" label={tr("列表视图")} pressed={view === "list"} onClick={() => setPref("view", "list")} />
          <Btn tone="quiet" iconOnly icon="grid" label={tr("网格视图")} pressed={view === "grid"} onClick={() => setPref("view", "grid")} />
        </div>
        <Btn tone="primary" icon="bolt" label={tr("添加链接")} disabled={!gCloud.ok} reason={gCloud.reason} onClick={() => void newCloudTask(folder, crumbPath)} />
      </div>
    </>
  );

  return (
    <PageFrame
      label={tr("文件")}
      side
      caption={<>
        <NavButtons up={goUp} />
        {searching ? <CaptionSearch onClose={() => setSearching(false)} /> : <AddressBar crumbs={crumbs} onSearch={() => setSearching(true)} onDrop={onDropInto} />}
      </>}
      commands={commands}
    >
      {st.next && st.phase === "ready" && (sort.key !== "name" || sort.dir !== "asc" || kind !== "all") ? (
        <div class="inline-note" role="note">
          <Icon name="info" size={18} />
          <span>{tr("排序与筛选只作用于已加载的")}{groupDigits(String(st.items.length))} {tr("项。")}</span>
          <button type="button" class="link-btn" onClick={() => void loadAll()} disabled={st.loadingMore}>{st.loadingMore ? tr("正在加载…") : tr("加载全部")}</button>
        </div>
      ) : null}
      {!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED" ? (
        <Empty title={s.auth?.state === "expired" ? tr("登录已过期") : tr("登录后浏览云盘")} awake action={<Btn tone="primary" icon="login" label={s.auth?.state === "expired" ? tr("重新登录") : tr("登录")} onClick={() => navigate({ page: "settings", section: "account" })} />} />
      ) : st.phase === "error" && st.error ? (
        <div class="canvas-pad">
          <ErrorPanel error={st.error} onRetry={retry} context={`files_list parent=${folder}`} />
          {st.error.code === "NOT_FOUND" && goUp ? <p class="pad-top"><Btn tone="tonal" icon="arrowUp" label={tr("返回上一级")} onClick={goUp} /></p> : null}
        </div>
      ) : st.phase === "loading" ? (
        <SkeletonRows rowH={rowH} />
      ) : st.items.length === 0 ? (
        <Empty title={tr("这个文件夹是空的")} action={!local ? <>
          <Btn tone="tonal" icon="folderPlus" label={tr("新建文件夹")} disabled={!gMk.ok} reason={gMk.reason} onClick={() => void newFolder()} />
          <Btn tone="primary" icon="bolt" label={tr("添加链接")} disabled={!gCloud.ok} reason={gCloud.reason} onClick={() => void newCloudTask(folder, crumbPath)} />
        </> : undefined}>{local ? tr("只读浏览你选择的本地文件夹。不会上传、同步或修改原文件。") : tr("新建文件夹，或粘贴磁力链接、http(s) 链接让服务端保存到这里。")}</Empty>
      ) : viewItems.length === 0 ? (
        <Empty title={tr("没有这种类型的项目")} action={<Btn tone="tonal" label={tr("显示全部类型")} onClick={() => setKind("all")} />} />
      ) : (
        <FileTable
          scrollKey={viewKey}
          id="files-grid"
          label={tx("{0} 中的文件", crumbs[crumbs.length - 1]?.name ?? tr("文件"))}
          items={viewItems}
          mode={view}
          gridSize={gridSize}
          onZoom={zoomView}
          onDropInto={onDropInto}
          columns={["name", "size", "modified", "kind"]}
          sort={sort}
          onSort={onSort}
          selection={sel}
          setSelection={setSel}
          rowHeight={rowH}
          onOpen={ctx.open}
          onMenu={(entries, x, y) => {
            if (!entries.length) {
              openMenu(x, y, [
                { id: "paste", label: clipCount ? tx("粘贴 {0} 项", clipCount) : tr("粘贴"), icon: "paste", shortcut: "Ctrl+V", disabled: !clipCount || !gate(clipMode === "cut" ? "move" : "copy").ok, reason: !clipCount ? tr("先在其他位置剪切或复制项目") : gate(clipMode === "cut" ? "move" : "copy").reason, run: paste },
                { id: "mk", label: tr("新建文件夹"), icon: "folderPlus", shortcut: "Ctrl+Shift+N", separatorBefore: true, disabled: !gMk.ok, reason: gMk.reason, run: () => void newFolder() },
                { id: "cloud", label: tr("添加链接"), icon: "bolt", disabled: !gCloud.ok, reason: gCloud.reason, run: () => void newCloudTask(folder, crumbPath) },
                { id: "up", label: tr("上传文件"), icon: "upload", disabled: !gUp.ok || uploading, reason: gUp.reason, run: () => void upload() },
                { id: "up-dir", label: tr("上传文件夹"), icon: "folderPlus", disabled: !directoryUpload || uploading, reason: directoryUploadReason, run: () => void upload(true) },
                { id: "dups", label: tr("在此文件夹中查找重复"), icon: "duplicates", separatorBefore: true, disabled: !gate("duplicates").ok, reason: gate("duplicates").reason, run: () => navigate({ page: "duplicates", folder }) },
                { id: "space", label: tr("查看空间分布"), icon: "space", disabled: !gate("space_summary").ok, reason: gate("space_summary").reason, run: () => navigate({ page: "space", folder }) },
                { id: "refresh", label: tr("刷新"), icon: "refresh", shortcut: "F5", separatorBefore: true, run: reload },
              ], tr("文件夹"));
              return;
            }
            openMenu(x, y, entryMenu(entries, ctx), entries.length > 1 ? tx("{0} 个项目", entries.length) : entries[0].name);
          }}
          onKeyAction={(a, entries) => {
            if (a === "parent") goUp?.();
            else if (a === "rename" && entries.length === 1) void renameEntry(entries[0], ctx);
            else if (a === "rename" && entries.length > 1) openBatchRename(entries, ctx);
            else if (a === "trash") void trashEntries(entries, ctx);
            else if (a === "copy-path") void copyPaths(entries, ctx);
            else if (a === "copy" || a === "cut") cutOrCopy(a, entries);
            else if (a === "paste") paste();
            else if (a === "star" && entries.length) void setStar(entries, !entries.every(isStarred), reload);
            else if (a === "properties" && entries[0]) void showProperties(entries[0].id, ctx.pathFor(entries[0]), fileGate("download", entries[0].id).ok ? (x) => void downloadEntries([x]) : undefined);
          }}
          hasMore={!!st.next}
          loadingMore={st.loadingMore}
          onLoadMore={() => void loadMore()}
          footer={st.next || st.moreError ? (
            <div class="ft-foot">
              {st.moreError ? <ErrorPanel error={st.moreError} compact onRetry={() => void loadMore()} />
                : st.loadingMore ? <Spinner label={tr("正在加载更多")} /> : <span class="muted">{tr("继续滚动以加载更多")}</span>}
            </div>
          ) : null}
        />
      )}
    </PageFrame>
  );
}

export function SkeletonRows({ rowH, n = 12 }: { rowH: number; n?: number }) {
  return (
    <div class="skeleton" aria-busy="true" aria-label={tr("正在加载列表")}>
      {Array.from({ length: n }, (_, i) => (
        <div class="sk-row" style={{ height: `${rowH}px` }}>
          <span class="sk-box" />
          <span class="sk-line" style={{ width: `${38 + ((i * 37) % 40)}%` }} />
          <span class="sk-line short" />
        </div>
      ))}
    </div>
  );
}
