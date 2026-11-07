/** Search results, Recent and Trash — flat lists that reuse FileTable. */
import { useEffect, useMemo, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { groupDigits } from "../lib/format";
import { useStore } from "../lib/store";
import { app, gate, signedIn } from "../state/app";
import { navigate } from "../state/route";
import { openMenu, toast } from "../state/ui";
import { FileTable, sortEntries, useSelection, type SortKey, type SortSpec } from "../components/FileTable";
import { Btn, Empty, ErrorPanel } from "../components/ui";
import { confirmDialog, showProperties } from "../components/dialogs";
import { CaptionSearch, NavButtons, SkeletonRows, useDensity } from "./Files";
import { PageFrame, PageTitle } from "../components/Frame";
import { copyPaths, downloadEntries, entryMenu, mutate, renameEntry, trashEntries, type ActionContext } from "./fileActions";
import { useRefreshKey } from "./common";

/** Resolve "/a/b" locations for result rows, cached per parent id. */
function useLocations(items: readonly RemoteEntry[], epoch: number) {
  const [map, setMap] = useState<Map<string, string>>(new Map([[ROOT_ID, "/"]]));
  useEffect(() => {
    const parents = [...new Set(items.map((e) => e.parent_id ?? ROOT_ID))].filter((p) => !map.has(p)).slice(0, 200);
    if (!parents.length) return;
    let alive = true;
    Promise.all(parents.map((p) => api.filesPath(p).then((chain) => [p, "/" + chain.map((c) => c.name).join("/")] as const, () => [p, "（位置不可用）"] as const)))
      .then((pairs) => {
        if (!alive) return;
        setMap((m) => {
          const n = new Map(m);
          for (const [k, v] of pairs) n.set(k, v);
          return n;
        });
      });
    return () => {
      alive = false;
    };
  }, [items, epoch]);
  return (e: RemoteEntry) => map.get(e.parent_id ?? ROOT_ID) ?? "…";
}

interface ListState {
  items: RemoteEntry[];
  phase: "loading" | "ready" | "error";
  error?: ClientError;
  next?: string;
}

function useFlatList(load: (cursor?: string) => Promise<{ items: RemoteEntry[]; next_cursor?: string }>, deps: unknown[]) {
  const [st, setSt] = useState<ListState>({ items: [], phase: "loading" });
  const [more, setMore] = useState(false);
  const run = () => {
    setSt({ items: [], phase: "loading" });
    load().then((p) => setSt({ items: p.items, phase: "ready", next: p.next_cursor }), (e) => setSt({ items: [], phase: "error", error: errorOf(e) }));
  };
  useEffect(run, deps);
  const loadMore = async () => {
    if (!st.next || more) return;
    setMore(true);
    try {
      const p = await load(st.next);
      setSt((s) => ({ ...s, items: [...s.items, ...p.items], next: p.next_cursor }));
    } catch (e) {
      toast(errorOf(e).message, "error", { error: errorOf(e) });
    } finally {
      setMore(false);
    }
  };
  return { st, run, loadMore, more };
}

function useSort(initial: SortSpec): [SortSpec, (k: SortKey) => void] {
  const [sort, setSort] = useState<SortSpec>(initial);
  return [sort, (key) => setSort((s) => (s.key === key ? { key, dir: s.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "name" ? "asc" : "desc" }))];
}

function ListShell({ title, meta, commands, caption, children }: { title: string; meta?: preact.ComponentChildren; commands?: preact.ComponentChildren; caption?: preact.ComponentChildren; children: preact.ComponentChildren }) {
  return (
    <PageFrame label={title} side caption={caption ?? <><NavButtons /><PageTitle>{title}</PageTitle></>}
      commands={commands || meta ? <>{commands ?? <span />}<div class="cmd-group cmd-end">{meta}</div></> : undefined}>
      {children}
    </PageFrame>
  );
}

function SignedOutEmpty() {
  const s = useStore(app, (x) => x.auth?.state);
  return (
    <Empty title={s === "expired" ? "登录已过期" : "登录后查看"} awake
      action={<Btn tone="primary" icon="user" label={s === "expired" ? "重新授权" : "连接账号"} onClick={() => navigate({ page: "settings", section: "account" })} />} />
  );
}

/* ------------------------------------------------------------------ */

function ResultList({ st, loadMore, more, run, emptyTitle, emptyBody, dateLabel, idPrefix, label, sortInit }: {
  st: ListState; loadMore: () => Promise<void>; more: boolean; run: () => void;
  emptyTitle: string; emptyBody?: string; dateLabel?: string; idPrefix: string; label: string; sortInit?: SortSpec;
}) {
  const epoch = useStore(app, (s) => s.epoch);
  const signed = useStore(app, (s) => signedIn(s));
  const [rowH] = useDensity();
  const [sort, onSort] = useSort(sortInit ?? { key: "name", dir: "asc" });
  const view = useMemo(() => sortEntries(st.items, sort, false), [st.items, sort]);
  const { sel, setSel } = useSelection(view, st.items);
  const locationOf = useLocations(st.items, epoch);
  const ctx: ActionContext = {
    pathFor: (e) => `${locationOf(e).replace(/\/$/, "")}/${e.name}`,
    siblings: () => [],
    reload: run,
    open: (e) => (e.kind === "folder" ? navigate({ page: "files", folder: e.id }) : void showProperties(e.id, `${locationOf(e).replace(/\/$/, "")}/${e.name}`, (x) => void downloadEntries([x]))),
  };

  if (!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED") return <SignedOutEmpty />;
  if (st.phase === "error" && st.error) return <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={run} /></div>;
  if (st.phase === "loading") return <SkeletonRows rowH={rowH} />;
  if (st.items.length === 0) return <Empty title={emptyTitle}>{emptyBody}</Empty>;
  return (
    <FileTable
      id={`${idPrefix}-grid`}
      label={label}
      items={view}
      columns={["name", "location", "size", "modified"]}
      sort={sort}
      onSort={onSort}
      selection={sel}
      setSelection={setSel}
      rowHeight={rowH}
      dateLabel={dateLabel}
      locationOf={locationOf}
      onOpen={ctx.open}
      onMenu={(entries, x, y) => {
        if (!entries.length) return;
        openMenu(x, y, [
          { id: "reveal", label: "打开所在文件夹", icon: "folder", disabled: entries.length !== 1, reason: "请选择一个项目", run: () => navigate({ page: "files", folder: entries[0].parent_id ?? ROOT_ID }) },
          ...entryMenu(entries, ctx).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
        ]);
      }}
      onKeyAction={(a, entries) => {
        if (a === "rename" && entries.length === 1) void renameEntry(entries[0], ctx);
        else if (a === "trash") void trashEntries(entries, ctx);
        else if (a === "copy") void copyPaths(entries, ctx);
        else if (a === "properties" && entries[0]) ctx.open(entries[0]);
      }}
      hasMore={!!st.next}
      loadingMore={more}
      onLoadMore={() => void loadMore()}
    />
  );
}

export function SearchPage({ query }: { query: string }) {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <ListShell title="搜索"><Empty title="全盘搜索尚未接入">后端搜索与目录索引仍在实现清单中；当前可使用文件页的类型筛选和排序。</Empty></ListShell>;
  return <BrowserSearchPage query={query} />;
}
function BrowserSearchPage({ query }: { query: string }) {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const g = gate("search");
  const { st, run, loadMore, more } = useFlatList((c) => api.filesSearch(query, c), [query, epoch, refresh]);
  return (
    <ListShell title={`搜索“${query}”`}
      caption={<><NavButtons /><h1 id="page-title" class="sr-only">{`搜索“${query}”`}</h1><CaptionSearch initial={query} onClose={() => navigate({ page: "files", folder: ROOT_ID })} /></>}
      meta={st.phase === "ready" ? <span class="cmd-meta">{groupDigits(String(st.items.length))}{st.next ? "+" : ""} 个结果</span> : null}>
      {!g.ok && signedIn() ? <div class="canvas-pad"><div class="inline-note warn">{g.reason}</div></div> : null}
      <ResultList st={st} run={run} loadMore={loadMore} more={more} idPrefix="search" label={`“${query}”的搜索结果`} emptyTitle="没有找到匹配的文件" emptyBody="换个关键词试试。搜索会匹配全部云盘中的文件名。" />
    </ListShell>
  );
}

export function RecentPage() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <ListShell title="最近"><Empty title="最近文件尚未接入">后端未提供最近文件索引；请从网盘目录浏览。</Empty></ListShell>;
  return <BrowserRecentPage />;
}
function BrowserRecentPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const { st, run, loadMore, more } = useFlatList(() => api.filesRecent(), [epoch, refresh]);
  return (
    <ListShell title="最近" meta={<span class="cmd-meta">按修改时间排列的最近文件</span>}>
      <ResultList st={st} run={run} loadMore={loadMore} more={more} idPrefix="recent" label="最近的文件" emptyTitle="还没有最近的文件" sortInit={{ key: "modified", dir: "desc" }} />
    </ListShell>
  );
}

export function TrashPage() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <ListShell title="回收站"><Empty title="回收站后端尚未接入">列出、还原及删除操作均未启用，不会发送未经实现的修改请求。</Empty></ListShell>;
  return <BrowserTrashPage />;
}
function BrowserTrashPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const signed = useStore(app, (s) => signedIn(s));
  const refresh = useRefreshKey();
  const [rowH] = useDensity();
  const { st, run } = useFlatList((c) => api.trashList(c), [epoch, refresh]);
  const [sort, onSort] = useSort({ key: "modified", dir: "desc" });
  const view = useMemo(() => sortEntries(st.items, sort, false), [st.items, sort]);
  const { sel, setSel, selected } = useSelection(view, st.items);
  const locationOf = useLocations(st.items, epoch);
  const g = gate("trash");

  const restore = (entries: RemoteEntry[]) => void mutate("restore", entries.map((e) => e.id), undefined, run);
  const purge = async (entries: RemoteEntry[]) => {
    if (!entries.length) return;
    const ok = await confirmDialog({
      title: `永久删除 ${entries.length} 项？`,
      body: <p>永久删除后无法从回收站还原。这与“移至回收站”不同。</p>,
      confirmLabel: "永久删除",
      danger: true,
      acknowledge: "我了解此操作无法撤销",
    });
    if (ok) await mutate("delete_forever", entries.map((e) => e.id), undefined, run);
  };

  return (
    <ListShell title="回收站" commands={
      <div class="cmd-group">
        <Btn tone="tonal" icon="restore" label="还原" disabled={!selected.length || !g.ok} reason={!g.ok ? g.reason : "先选择要还原的项目"} onClick={() => restore(selected)} />
        <Btn tone="danger" icon="trash" label="永久删除" disabled={!selected.length || !g.ok} reason={!g.ok ? g.reason : "先选择要删除的项目"} onClick={() => void purge(selected)} />
        {selected.length ? <span class="sel-count">已选 {selected.length} 项</span> : null}
      </div>
    } meta={<span class="cmd-meta">移至回收站的项目可以还原</span>}>
      {!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED" ? <SignedOutEmpty />
        : st.phase === "error" && st.error ? <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={run} /></div>
          : st.phase === "loading" ? <SkeletonRows rowH={rowH} />
            : st.items.length === 0 ? <Empty title="回收站是空的">移至回收站的项目会显示在这里，可以还原。</Empty>
              : (
                <FileTable
                  id="trash-grid"
                  label="回收站中的项目"
                  items={view}
                  columns={["name", "location", "size", "modified"]}
                  sort={sort}
                  onSort={onSort}
                  selection={sel}
                  setSelection={setSel}
                  rowHeight={rowH}
                  dateLabel="删除前修改"
                  locationOf={(e) => `原位置 ${locationOf(e)}`}
                  onOpen={(e) => void showProperties(e.id)}
                  onMenu={(entries, x, y) => entries.length && openMenu(x, y, [
                    { id: "restore", label: "还原", icon: "restore", disabled: !g.ok, reason: g.reason, run: () => restore(entries) },
                    { id: "props", label: "属性", icon: "info", disabled: entries.length !== 1, reason: "请选择一个项目", run: () => void showProperties(entries[0].id) },
                    { id: "purge", label: "永久删除…", icon: "trash", danger: true, separatorBefore: true, disabled: !g.ok, reason: g.reason, run: () => void purge(entries) },
                  ])}
                  onKeyAction={(a, entries) => {
                    if (a === "trash") void purge(entries);
                  }}
                />
              )}
    </ListShell>
  );
}
