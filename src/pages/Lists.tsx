import { tr, tx } from "../lib/i18n";
/** Search results, Recent and Trash — flat lists that reuse FileTable. */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
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
import { copyPaths, cutOrCopy, downloadEntries, entryMenu, mutate, renameEntry, setStar, trashEntries, type ActionContext } from "./fileActions";
import { openBatchRename } from "./lazy";
import { isStarred } from "../lib/contract-proposals";
import { useRefreshKey } from "./common";

/** Resolve "/a/b" locations for result rows, cached per parent id. */
function useLocations(items: readonly RemoteEntry[], epoch: number) {
  const [map, setMap] = useState<Map<string, string>>(new Map([[ROOT_ID, "/"]]));
  useEffect(() => {
    const parents = [...new Set(items.map((e) => e.parent_id ?? ROOT_ID))].filter((p) => !map.has(p)).slice(0, 200);
    if (!parents.length) return;
    let alive = true;
    Promise.all(parents.map((p) => api.filesPath(p).then((chain) => [p, "/" + chain.map((c) => c.name).join("/")] as const, () => [p, tr("（位置不可用）")] as const)))
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
  const generation = useRef(0);
  const loadingMore = useRef(false);
  const run = () => {
    const current=++generation.current;
    loadingMore.current=false; setMore(false);
    setSt({ items: [], phase: "loading" });
    load().then((p) => {if(current===generation.current)setSt({ items: p.items, phase: "ready", next: p.next_cursor });}, (e) => {if(current===generation.current)setSt({ items: [], phase: "error", error: errorOf(e) });});
  };
  useEffect(() => {run();return () => {generation.current++;};}, deps);
  const loadMore = async () => {
    if (!st.next || loadingMore.current) return;
    const current=generation.current;loadingMore.current=true;
    setMore(true);
    try {
      const p = await load(st.next);
      if(current!==generation.current)return;
      setSt((s) => { const seen=new Set(s.items.map(item=>item.id)); return { ...s, items: [...s.items, ...p.items.filter(item=>{if(seen.has(item.id)) return false; seen.add(item.id); return true;})], next: p.next_cursor }; });
    } catch (e) {
      if(current!==generation.current)return;
      toast(errorOf(e).message, "error", { error: errorOf(e) });
    } finally {
      if(current===generation.current){loadingMore.current=false;setMore(false);}
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
    <Empty title={s === "expired" ? tr("登录已过期") : tr("登录后查看")} awake
      action={<Btn tone="primary" icon="user" label={s === "expired" ? tr("重新授权") : tr("连接账号")} onClick={() => navigate({ page: "settings", section: "account" })} />} />
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
    // Items come from many folders; the core reports real name conflicts.
    siblings: () => [],
    reload: run,
    open: (e) => (e.kind === "folder" ? navigate({ page: "files", folder: e.id }) : void showProperties(e.id, `${locationOf(e).replace(/\/$/, "")}/${e.name}`, (x) => void downloadEntries([x]))),
  };

  if (!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED") return <SignedOutEmpty />;
  if (st.phase === "error" && st.error) return <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={run} /></div>;
  if (st.phase === "loading") return <SkeletonRows rowH={rowH} />;
  if (st.items.length === 0) return st.next
    ? <Empty title={tr("当前页没有匹配项目，仍有文件待读取")} action={<Btn label={tr("继续读取")} busy={more} onClick={() => void loadMore()} />}>{tr("继续读取后才能确认全部结果。")}</Empty>
    : <Empty title={emptyTitle}>{emptyBody}</Empty>;
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
        const items = entryMenu(entries, ctx);
        const firstList = items.findIndex((m) => !m.quick);
        openMenu(x, y, [
          ...items.slice(0, firstList),
          { id: "reveal", label: tr("打开所在文件夹"), icon: "folderOpen", disabled: entries.length !== 1, reason: tr("请选择一个项目"), run: () => navigate({ page: "files", folder: entries[0].parent_id ?? ROOT_ID }) },
          ...items.slice(firstList).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
        ], entries.length > 1 ? tx("{0} 个项目", entries.length) : entries[0].name);
      }}
      onKeyAction={(a, entries) => {
        if (a === "rename" && entries.length === 1) void renameEntry(entries[0], ctx);
        else if (a === "rename" && entries.length > 1) openBatchRename(entries, ctx);
        else if (a === "trash") void trashEntries(entries, ctx);
        else if (a === "copy-path") void copyPaths(entries, ctx);
        else if (a === "copy" || a === "cut") cutOrCopy(a, entries);
        else if (a === "star" && entries.length) void setStar(entries, !entries.every(isStarred), run);
        else if (a === "properties" && entries[0]) ctx.open(entries[0]);
      }}
      hasMore={!!st.next}
      loadingMore={more}
      onLoadMore={() => void loadMore()}
    />
  );
}

export function SearchPage({ query }: { query: string }) {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const g = gate("search");
  const { st, run, loadMore, more } = useFlatList((c) => api.filesSearch(query, c), [query, epoch, refresh]);
  return (
    <ListShell title={tx("搜索“{0}”", query)}
      caption={<><NavButtons /><h1 id="page-title" class="sr-only">{tx("搜索“{0}”", query)}</h1><CaptionSearch initial={query} onClose={() => navigate({ page: "files", folder: ROOT_ID })} /></>}
      meta={st.phase === "ready" ? <span class="cmd-meta">{groupDigits(String(st.items.length))}{st.next ? "+" : ""} {tr("个结果")}</span> : null}>
      {!g.ok && signedIn() ? <div class="canvas-pad"><div class="inline-note warn">{g.reason}</div></div> : null}
      <ResultList st={st} run={run} loadMore={loadMore} more={more} idPrefix="search" label={tx("“{0}”的搜索结果", query)} emptyTitle={tr("没有找到匹配的文件")} emptyBody={tr("换个关键词试试。搜索会匹配全部云盘中的文件名。")} />
    </ListShell>
  );
}

export function RecentPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const { st, run, loadMore, more } = useFlatList((c) => api.filesRecent(c), [epoch, refresh]);
  return (
    <ListShell title={tr("最近")} meta={<span class="cmd-meta">{tr("最近操作过的文件")}</span>}>
      <ResultList st={st} run={run} loadMore={loadMore} more={more} idPrefix="recent" label={tr("最近的文件")} emptyTitle={tr("还没有最近的文件")} sortInit={{ key: "modified", dir: "desc" }} />
    </ListShell>
  );
}

export function StarredPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const g = gate("star");
  const { st, run, loadMore, more } = useFlatList((c) => api.filesStarred(c), [epoch, refresh]);
  return (
    <ListShell title={tr("星标")} meta={st.phase === "ready" ? <span class="cmd-meta">{groupDigits(String(st.items.length))}{tr("项")}</span> : null}>
      {!g.ok && signedIn() ? <div class="canvas-pad"><div class="inline-note warn">{g.reason}</div></div> : null}
      <ResultList st={st} run={run} loadMore={loadMore} more={more} idPrefix="starred" label={tr("加星标的项目")} emptyTitle={tr("还没有星标")}
        emptyBody={tr("在文件或文件夹上右键选择“添加星标”，或选中后按 Ctrl+D。星标保存在 PikPak 账号中，换设备也能看到。")} sortInit={{ key: "modified", dir: "desc" }} />
    </ListShell>
  );
}

export function TrashPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const signed = useStore(app, (s) => signedIn(s));
  const refresh = useRefreshKey();
  const [rowH] = useDensity();
  const { st, run, loadMore, more } = useFlatList((c) => api.trashList(c), [epoch, refresh]);
  const [sort, onSort] = useSort({ key: "modified", dir: "desc" });
  const view = useMemo(() => sortEntries(st.items, sort, false), [st.items, sort]);
  const { sel, setSel, selected } = useSelection(view, st.items);
  const locationOf = useLocations(st.items, epoch);
  const g = gate("trash");
  const canPurge = app.get().runtime === "mock" ? g.ok : gate("delete_forever").ok;

  const restore = (entries: RemoteEntry[]) => void mutate("restore", entries.map((e) => e.id), undefined, run);
  const purge = async (entries: RemoteEntry[]) => {
    if (!canPurge) return;
    if (!entries.length) return;
    const ok = await confirmDialog({
      title: tx("永久删除 {0} 项？", entries.length),
      body: <p>{tr("永久删除后无法从回收站还原。这与“移至回收站”不同。")}</p>,
      confirmLabel: tr("永久删除"),
      danger: true,
      acknowledge: tr("我了解此操作无法撤销"),
    });
    if (ok) await mutate("delete_forever", entries.map((e) => e.id), undefined, run);
  };

  return (
    <ListShell title={tr("回收站")} commands={
      <div class="cmd-group">
        <Btn tone="tonal" icon="restore" label={tr("还原")} disabled={!selected.length || !g.ok} reason={!g.ok ? g.reason : tr("先选择要还原的项目")} onClick={() => restore(selected)} />
        <Btn tone="danger" icon="trash" label={tr("永久删除")} disabled={!selected.length || !canPurge} reason={!canPurge ? tr("永久删除尚未启用") : tr("先选择要删除的项目")} onClick={() => void purge(selected)} />
        {selected.length ? <span class="sel-count">{tr("已选")} {selected.length} {tr("项")}</span> : null}
      </div>
    } meta={<span class="cmd-meta">{tr("移至回收站的项目可以还原")}</span>}>
      {!signed && st.phase === "error" && st.error?.code === "AUTH_EXPIRED" ? <SignedOutEmpty />
        : st.phase === "error" && st.error ? <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={run} /></div>
          : st.phase === "loading" ? <SkeletonRows rowH={rowH} />
            : st.items.length === 0 ? <Empty title={tr("回收站是空的")}>{tr("移至回收站的项目会显示在这里，可以还原。")}</Empty>
              : (
                <FileTable
                  id="trash-grid"
                  label={tr("回收站中的项目")}
                  items={view}
                  columns={["name", "location", "size", "modified"]}
                  sort={sort}
                  onSort={onSort}
                  selection={sel}
                  setSelection={setSel}
                  rowHeight={rowH}
                  dateLabel={tr("删除前修改")}
                  locationOf={(e) => tx("原位置 {0}", locationOf(e))}
                  onOpen={(e) => void showProperties(e.id)}
                  onMenu={(entries, x, y) => entries.length && openMenu(x, y, [
                    { id: "restore", label: tr("还原"), icon: "restore", disabled: !g.ok, reason: g.reason, run: () => restore(entries) },
                    { id: "props", label: tr("属性"), icon: "info", disabled: entries.length !== 1, reason: tr("请选择一个项目"), run: () => void showProperties(entries[0].id) },
                    { id: "purge", label: tr("永久删除…"), icon: "trash", danger: true, separatorBefore: true, disabled: !canPurge, reason: tr("永久删除尚未启用"), run: () => void purge(entries) },
                  ])}
                  onKeyAction={(a, entries) => {
                    if (a === "trash") void purge(entries);
                  }}
                  hasMore={!!st.next} loadingMore={more} onLoadMore={() => void loadMore()}
                />
              )}
    </ListShell>
  );
}
