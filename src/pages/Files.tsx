import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { formatBytes, addU64, groupDigits } from "../lib/format";
import { fileKind, KIND_LABEL, type FileKind } from "../lib/names";
import { useStore } from "../lib/store";
import { app, gate, signedIn } from "../state/app";
import { navigate } from "../state/route";
import { focusRequests, openMenu, toast } from "../state/ui";
import { prefs, setPref } from "../state/prefs";
import { FileTable, sortEntries, useSelection, type SortKey, type SortSpec } from "../components/FileTable";
import { Icon } from "../components/Icon";
import { Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { PageFrame } from "../components/Frame";
import { newCloudTask, promptName, showProperties } from "../components/dialogs";
import {
  copyPaths, downloadEntries, entryMenu, moveOrCopy, mutate, pathOf, renameEntry, trashEntries,
  type ActionContext,
} from "./fileActions";
import { useRefreshKey } from "./common";

export interface Crumb { id: string; name: string }

const crumbCache = new Map<string, Crumb[]>();
const ROOT_CRUMB: Crumb = { id: ROOT_ID, name: "网盘" };
const PAGE = 500;
/** Recently visited folders for the address-bar dropdown (session only). */
const recentFolders: Crumb[] = [];

function readPref<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}
function writePref(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* per-viewer convenience only */
  }
}

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
    const same = prevFolder.current === folder;
    prevFolder.current = folder;
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
    const my = token.current;
    setSt((s) => ({ ...s, loadingMore: true }));
    const acc: RemoteEntry[] = [];
    try {
      while (cursor) {
        const page = await api.filesList(folder, cursor, 500);
        if (my !== token.current) return;
        acc.push(...page.items);
        cursor = page.next_cursor;
      }
      setSt((s) => ({ ...s, items: [...s.items, ...acc], next: undefined, loadingMore: false }));
    } catch (e) {
      setSt((s) => ({ ...s, items: [...s.items, ...acc], next: cursor, loadingMore: false, moreError: errorOf(e) }));
    }
  }, [folder, st.next]);

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
      <Btn tone="quiet" iconOnly icon="arrowBack" label="后退（Alt+←）" onClick={() => history.back()} />
      <Btn tone="quiet" iconOnly icon="arrowForward" label="前进（Alt+→）" onClick={() => history.forward()} />
      <Btn tone="quiet" iconOnly icon="arrowUp" label="上一级（Backspace）" disabled={!up} reason="已在根目录" onClick={() => up?.()} />
    </div>
  );
}

function AddressBar({ crumbs, onSearch }: { crumbs: Crumb[]; onSearch: () => void }) {
  return (
    <div class="address">
      <nav class="crumbs" aria-label="路径">
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
                onClick={(e) => { e.preventDefault(); navigate({ page: "files", folder: c.id }); }}>
                {i === 0 ? <Icon name="cloud" size={18} /> : null}
                <span>{c.name}</span>
              </a>
            )}
          </>
        ))}
      </nav>
      <button type="button" class="address-drop" aria-label="最近访问的位置" title="最近访问的位置"
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).closest(".address")!.getBoundingClientRect();
          const list = recentFolders.filter((f) => f.id !== crumbs[crumbs.length - 1]?.id).slice(0, 8);
          openMenu(r.right - 280, r.bottom + 4, list.length
            ? list.map((f) => ({ id: f.id, label: f.name, icon: f.id === ROOT_ID ? "cloud" : "folder", run: () => navigate({ page: "files", folder: f.id }) }))
            : [{ id: "none", label: "还没有其他最近访问的位置", disabled: true }], "最近访问");
        }}>
        <Icon name="chevronDown" size={20} />
      </button>
      <span class="address-split" />
      <button type="button" class="address-search" aria-label="搜索（Ctrl+F）" title="搜索（Ctrl+F）" onClick={onSearch}>
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
      <input ref={input} type="search" value={q} placeholder="搜索全部云盘文件" aria-label="搜索云盘文件"
        onInput={(e) => setQ((e.target as HTMLInputElement).value)}
        onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); onClose(); } }} />
      {/* Always present: clears text, or closes search when empty (touch-friendly). */}
      <button type="button" class="address-search" aria-label={q ? "清除" : "关闭搜索"} title={q ? "清除" : "关闭搜索（Esc）"}
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

const SORT_LABEL: Record<SortKey, string> = { name: "名称", size: "大小", modified: "修改时间", kind: "类型" };

export function FilesPage({ folder }: { folder: string }) {
  const s = useStore(app, (x) => x);
  const view = useStore(prefs, (p) => p.view);
  const refresh = useRefreshKey();
  const { st, reload, retry, loadMore, loadAll } = useFolder(folder, s.epoch, refresh);
  const crumbs = useCrumbs(folder, s.epoch);
  const [sort, setSort] = useState<SortSpec>(() => readPref("nekopak.sort", { key: "name", dir: "asc" } as SortSpec));
  const [kind, setKind] = useState<KindFilter>("all");
  const [rowH] = useDensity();
  const [searching, setSearching] = useState(false);
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
    return sortEntries(filtered, sort);
  }, [st.items, sort, kind]);

  const { sel, setSel, selected } = useSelection(viewItems, folder);
  const crumbPath = "/" + crumbs.slice(1).map((c) => c.name).join("/");

  const ctx: ActionContext = {
    pathFor: (e) => pathOf(crumbPath, e.name),
    siblings: () => st.items.map((e) => e.name),
    reload,
    open: (e) => {
      if (e.kind === "folder") {
        crumbCache.set(e.id, [...crumbs, { id: e.id, name: e.name }]);
        navigate({ page: "files", folder: e.id });
      } else {
        void showProperties(e.id, pathOf(crumbPath, e.name), (x) => void downloadEntries([x]));
      }
    },
  };

  const goUp = crumbs.length > 1 ? () => navigate({ page: "files", folder: crumbs[crumbs.length - 2].id }) : undefined;

  const newFolder = async () => {
    const g = gate("mkdir");
    if (!g.ok) return void toast(g.reason ?? "不能新建文件夹", "info");
    const name = await promptName("新建文件夹", { initial: "新建文件夹", siblings: st.items.map((e) => e.name), confirmLabel: "新建" });
    if (name !== undefined) await mutate("mkdir", [], { parent_id: folder, name }, reload);
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

  const onSort = (key: SortKey) => {
    const next: SortSpec = sort.key === key ? { key, dir: sort.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "name" || key === "kind" ? "asc" : "desc" };
    setSort(next);
    writePref("nekopak.sort", next);
  };

  const gMk = gate("mkdir");
  const gUp = gate("upload");
  const gCloud = gate("cloud_task");
  const gDl = gate("download");
  const selFiles = selected.filter((e) => e.kind === "file");
  const selSize = addU64(selFiles.map((e) => e.size_bytes));
  const signed = signedIn(s);
  const menuAt = (e: MouseEvent) => (e.currentTarget as HTMLElement).getBoundingClientRect();

  const commandsLeft: ComponentChildren = selected.length === 0 ? (
    <div key="create" class="cmd-group">
      <Btn tone="quiet" iconOnly icon="home" label="回到网盘根目录" disabled={folder === ROOT_ID} reason="已在根目录" onClick={() => navigate({ page: "files", folder: ROOT_ID })} />
      <Btn tone="ghost" icon="plus" label="新建" trail="chevronDown" disabled={!gMk.ok} reason={gMk.reason}
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, [
            { id: "mk", label: "新建文件夹", icon: "folderPlus", shortcut: "Ctrl+Shift+N", disabled: !gMk.ok, reason: gMk.reason, run: () => void newFolder() },
            { id: "up", label: "上传文件", icon: "upload", disabled: true, reason: gUp.ok ? "上传在 M1 接入" : gUp.reason },
          ], "新建");
        }} />
      <span class="cmd-sep" />
      <Btn tone="ghost" icon="sort" label={`按${SORT_LABEL[sort.key]}`} trail={sort.dir === "asc" ? "arrowUp" : "chevronDown"} title="排序方式"
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, (Object.keys(SORT_LABEL) as SortKey[]).map((k) => ({
            id: k, label: `${SORT_LABEL[k]}${sort.key === k ? (sort.dir === "asc" ? "（升序）" : "（降序）") : ""}`, icon: sort.key === k ? "check" : undefined, run: () => onSort(k),
          })), "排序");
        }} />
      <Btn tone="ghost" icon="filter" label={kind === "all" ? "全部类型" : KIND_LABEL[kind]} trail="chevronDown" title="按类型筛选"
        onClick={(e) => {
          const r = menuAt(e);
          openMenu(r.left, r.bottom + 4, [{ id: "all", label: "全部类型", icon: kind === "all" ? "check" : undefined, run: () => setKind("all") },
            ...(Object.keys(KIND_LABEL) as FileKind[]).map((k) => ({ id: k, label: KIND_LABEL[k], icon: kind === k ? "check" : undefined, run: () => setKind(k) }))], "类型");
        }} />
      <Btn tone="quiet" iconOnly icon="selectAll" label="全选（Ctrl+A）" disabled={!viewItems.length} reason="没有可选择的项目"
        onClick={() => setSel((p) => ({ ...p, ids: new Set(viewItems.map((i) => i.id)) }))} />
    </div>
  ) : (
    <div key="selection" class="cmd-group cmd-selection" role="group" aria-label="所选项目">
      <Btn tone="quiet" iconOnly icon="x" label="退出多选（Esc）" onClick={() => setSel((p) => ({ ...p, ids: new Set() }))} />
      <span class="sel-count" title={selFiles.length ? `所选文件共 ${formatBytes(selSize)}` : undefined} aria-live="polite">已选 {selected.length} 项</span>
      <span class="cmd-sep" />
      <Btn tone="tonal" icon="download" label={selFiles.length > 1 ? `下载 ${selFiles.length} 项` : "下载"}
        disabled={!gDl.ok || selFiles.length === 0} reason={!gDl.ok ? gDl.reason : "M0 暂不支持下载文件夹"}
        onClick={() => void downloadEntries(selected)} />
      <Btn tone="quiet" iconOnly icon="folderMove" label="移动到…" disabled={!gate("move").ok} reason={gate("move").reason} onClick={() => void moveOrCopy("move", selected, ctx, folder)} />
      <Btn tone="quiet" iconOnly icon="copy" label="复制到…" disabled={!gate("copy").ok} reason={gate("copy").reason} onClick={() => void moveOrCopy("copy", selected, ctx, folder)} />
      <Btn tone="quiet" iconOnly icon="pencil" label="重命名（F2）" disabled={selected.length !== 1 || !gate("rename").ok} reason={selected.length !== 1 ? "一次只能重命名一个项目" : gate("rename").reason} onClick={() => void renameEntry(selected[0], ctx)} />
      <Btn tone="quiet" iconOnly icon="link" label="复制路径（Ctrl+C）" onClick={() => void copyPaths(selected, ctx)} />
      <Btn tone="quiet" iconOnly icon="trash" label="移至回收站（Delete）" disabled={!gate("trash").ok} reason={gate("trash").reason} onClick={() => void trashEntries(selected, ctx)} />
    </div>
  );

  const commands = (
    <>
      {commandsLeft}
      <div class="cmd-group cmd-end">
        {st.phase === "ready" ? (
          <span class="cmd-meta" aria-live="polite">{st.next ? `已加载 ${groupDigits(String(st.items.length))} 项，还有更多` : `${groupDigits(String(st.items.length))} 项`}</span>
        ) : null}
        <Btn tone="quiet" iconOnly icon="refresh" label="刷新（F5）" busy={st.phase === "loading"} onClick={reload} />
        <div class="view-toggle" role="group" aria-label="视图">
          <Btn tone="quiet" iconOnly icon="rows" label="列表视图" pressed={view === "list"} onClick={() => setPref("view", "list")} />
          <Btn tone="quiet" iconOnly icon="grid" label="网格视图" pressed={view === "grid"} onClick={() => setPref("view", "grid")} />
        </div>
        <Btn tone="primary" icon="bolt" label="添加链接" disabled={!gCloud.ok} reason={gCloud.reason} onClick={() => void newCloudTask(folder, crumbPath)} />
      </div>
    </>
  );

  return (
    <PageFrame
      label="文件"
      side
      caption={<>
        <NavButtons up={goUp} />
        {searching ? <CaptionSearch onClose={() => setSearching(false)} /> : <AddressBar crumbs={crumbs} onSearch={() => setSearching(true)} />}
      </>}
      commands={commands}
    >
      {st.next && st.phase === "ready" && (sort.key !== "name" || sort.dir !== "asc" || kind !== "all") ? (
        <div class="inline-note" role="note">
          <Icon name="info" size={18} />
          <span>排序与筛选只作用于已加载的 {groupDigits(String(st.items.length))} 项。</span>
          <button type="button" class="link-btn" onClick={() => void loadAll()} disabled={st.loadingMore}>{st.loadingMore ? "正在加载…" : "加载全部"}</button>
        </div>
      ) : null}
      {!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED" ? (
        <Empty title={s.auth?.state === "expired" ? "登录已过期" : "登录后浏览云盘"} awake action={<Btn tone="primary" icon="user" label={s.auth?.state === "expired" ? "重新授权" : "连接账号"} onClick={() => navigate({ page: "settings", section: "account" })} />}>
          NekoPak 通过已授权的后端访问你的 PikPak 云盘，不会要求你在此输入密码。
        </Empty>
      ) : st.phase === "error" && st.error ? (
        <div class="canvas-pad">
          <ErrorPanel error={st.error} onRetry={retry} context={`files_list parent=${folder}`} />
          {st.error.code === "NOT_FOUND" && goUp ? <p class="pad-top"><Btn tone="tonal" icon="arrowUp" label="返回上一级" onClick={goUp} /></p> : null}
        </div>
      ) : st.phase === "loading" ? (
        <SkeletonRows rowH={rowH} />
      ) : st.items.length === 0 ? (
        <Empty title="这个文件夹是空的" action={<>
          <Btn tone="tonal" icon="folderPlus" label="新建文件夹" disabled={!gMk.ok} reason={gMk.reason} onClick={() => void newFolder()} />
          <Btn tone="primary" icon="bolt" label="添加链接" disabled={!gCloud.ok} reason={gCloud.reason} onClick={() => void newCloudTask(folder, crumbPath)} />
        </>}>新建文件夹，或粘贴磁力链接、http(s) 链接让服务端保存到这里。</Empty>
      ) : viewItems.length === 0 ? (
        <Empty title="没有这种类型的项目" action={<Btn tone="tonal" label="显示全部类型" onClick={() => setKind("all")} />} />
      ) : (
        <FileTable
          id="files-grid"
          label={`${crumbs[crumbs.length - 1]?.name ?? "文件"} 中的文件`}
          items={viewItems}
          mode={view}
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
                { id: "mk", label: "新建文件夹", icon: "folderPlus", shortcut: "Ctrl+Shift+N", disabled: !gMk.ok, reason: gMk.reason, run: () => void newFolder() },
                { id: "cloud", label: "添加链接", icon: "bolt", disabled: !gCloud.ok, reason: gCloud.reason, run: () => void newCloudTask(folder, crumbPath) },
                { id: "up", label: "上传文件", icon: "upload", disabled: true, reason: "上传在 M1 接入" },
                { id: "refresh", label: "刷新", icon: "refresh", shortcut: "F5", separatorBefore: true, run: reload },
              ], "文件夹");
              return;
            }
            openMenu(x, y, entryMenu(entries, ctx), entries.length > 1 ? `${entries.length} 个项目` : entries[0].name);
          }}
          onKeyAction={(a, entries) => {
            if (a === "parent") goUp?.();
            else if (a === "rename" && entries.length === 1) void renameEntry(entries[0], ctx);
            else if (a === "rename") toast("一次只能重命名一个项目", "info");
            else if (a === "trash") void trashEntries(entries, ctx);
            else if (a === "copy") void copyPaths(entries, ctx);
            else if (a === "properties" && entries[0]) void showProperties(entries[0].id, ctx.pathFor(entries[0]), (x) => void downloadEntries([x]));
          }}
          hasMore={!!st.next}
          loadingMore={st.loadingMore}
          onLoadMore={() => void loadMore()}
          footer={st.next || st.moreError ? (
            <div class="ft-foot">
              {st.moreError ? <ErrorPanel error={st.moreError} compact onRetry={() => void loadMore()} />
                : st.loadingMore ? <Spinner label="正在加载更多" /> : <span class="muted">继续滚动以加载更多</span>}
            </div>
          ) : null}
        />
      )}
    </PageFrame>
  );
}

export function SkeletonRows({ rowH, n = 12 }: { rowH: number; n?: number }) {
  return (
    <div class="skeleton" aria-busy="true" aria-label="正在加载列表">
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
