import { tr, tx } from "../lib/i18n";
/**
 * “查找重复”: the core scans a folder subtree and groups byte-identical
 * files (same hash and size; PROPOSAL UI-001 §P17). The UI suggests which
 * copies to move to trash — the oldest copy in each group is kept — and
 * nothing is removed without confirmation. Trashing is undoable.
 */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError } from "../lib/contract";
import type { DuplicateEntry, DuplicateGroup, DuplicateReport } from "../lib/contract-proposals";
import { PROPOSED_EVENTS, ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey, on } from "../lib/api";
import { formatBytes, formatDateTime, groupDigits } from "../lib/format";
import { useStore } from "../lib/store";
import { app, gate } from "../state/app";
import { navigate } from "../state/route";
import { toast } from "../state/ui";
import { pushUndo, undo } from "../state/undo";
import { Icon } from "../components/Icon";
import { KindTile } from "../components/FileTable";
import { Badge, Btn, Empty, ErrorPanel, Progress, Spinner } from "../components/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { confirmDialog } from "../components/dialogs";
import { NavButtons } from "./Files";
import { requestRefresh } from "./common";

const BATCH = 100;

function keeperOf(g: DuplicateGroup): string {
  // Oldest copy (then shortest path) is the suggested original.
  return [...g.entries].sort((a, b) => (a.modified_at ?? "").localeCompare(b.modified_at ?? "") || a.path.length - b.path.length)[0].id;
}

export function DuplicatesPage({ folder }: { folder: string }) {
  const epoch = useStore(app, (s) => s.epoch);
  const [scanId, setScanId] = useState<string | null>(null);
  const [progress, setProgress] = useState({ files: 0, folders: 0 });
  const [report, setReport] = useState<DuplicateReport | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [limit, setLimit] = useState(60);
  const scanRef = useRef<string | null>(null);
  const g = gate("duplicates");

  const start = async () => {
    setError(null);
    setReport(null);
    setSel(new Set());
    setProgress({ files: 0, folders: 0 });
    try {
      const { scan_id } = await api.duplicatesScan(folder);
      scanRef.current = scan_id;
      setScanId(scan_id);
    } catch (e) {
      setError(errorOf(e));
    }
  };

  const fetchReport = async (id: string) => {
    try {
      const r = await api.duplicatesReport(id);
      if (scanRef.current !== id) return;
      setReport(r);
      if (r.state === "failed" && r.error) setError(r.error);
    } catch (e) {
      setError(errorOf(e));
    }
  };

  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    void on(PROPOSED_EVENTS.duplicatesProgress, (ev) => {
      if (ev.scan_id !== scanRef.current) return;
      setProgress({ files: ev.files_scanned, folders: ev.folders_scanned });
      if (ev.state !== "running") void fetchReport(ev.scan_id);
    }).then((u) => (alive ? (off = u) : u()));
    if (g.ok) void start();
    return () => {
      alive = false;
      off?.();
      if (scanRef.current) void api.duplicatesCancel(scanRef.current).catch(() => undefined);
      scanRef.current = null;
    };
  }, [folder, epoch]);

  const groups = report?.state === "completed" ? report.groups : [];
  const byId = useMemo(() => {
    const m = new Map<string, DuplicateEntry>();
    for (const gr of groups) for (const e of gr.entries) m.set(e.id, e);
    return m;
  }, [groups]);
  const selBytes = [...sel].reduce((s, id) => s + BigInt(byId.get(id)?.size_bytes ?? "0"), 0n);

  const suggest = () => {
    const next = new Set<string>();
    for (const gr of groups) {
      const keep = keeperOf(gr);
      for (const e of gr.entries) if (e.id !== keep && !e.read_only) next.add(e.id);
    }
    setSel(next);
  };

  const toggle = (id: string, gr: DuplicateGroup) => {
    const next = new Set(sel);
    if (next.has(id)) next.delete(id);
    else {
      // never let every copy of a group be selected
      const others = gr.entries.filter((e) => e.id !== id && !next.has(e.id));
      if (!others.length) return void toast(tr("每组至少保留一份"), "info");
      next.add(id);
    }
    setSel(next);
  };

  const trash = async () => {
    const ids = [...sel];
    if (!ids.length) return;
    const tg = gate("trash");
    if (!tg.ok) return void toast(tg.reason ?? tr("不能移至回收站"), "info");
    const ok = await confirmDialog({
      title: tx("将 {0} 个重复文件移至回收站？", ids.length),
      body: <p>{tr("这些文件与同组保留的文件内容完全相同（哈希与大小一致）。移至回收站后可以还原，约释放")}{formatBytes(selBytes)}。</p>,
      confirmLabel: tr("移至回收站"),
      danger: true,
    });
    if (!ok) return;
    setBusy(true);
    const done: string[] = [];
    let firstErr = "";
    for (let i = 0; i < ids.length; i += BATCH) {
      const part = ids.slice(i, i + BATCH);
      try {
        const res = await api.filesMutate("trash", part, undefined, newIdempotencyKey());
        done.push(...res.succeeded);
        if (res.failed[0] && !firstErr) firstErr = res.failed[0].error.message;
      } catch (e) {
        if (!firstErr) firstErr = errorOf(e).message;
        break;
      }
    }
    setBusy(false);
    setSel(new Set(ids.filter((id) => !done.includes(id))));
    if (done.length) {
      const entry = pushUndo(tx("移除 {0} 个重复文件", done.length), async () => {
        try {
          const res = await api.filesMutate("restore", done, undefined, newIdempotencyKey());
          requestRefresh();
          toast(res.failed.length ? tx("撤销时 {0} 项失败", res.failed.length) : tx("已还原 {0} 个文件", res.succeeded.length), res.failed.length ? "error" : "success");
          if (scanRef.current) void fetchReport(scanRef.current);
          return !res.failed.length;
        } catch (e) {
          toast(errorOf(e).message, "error");
          return false;
        }
      });
      toast(tx("已将 {0} 个重复文件移至回收站{1}", done.length, firstErr ? `，部分失败：${firstErr}` : ""), firstErr ? "warning" : "success", { action: { label: tr("撤销"), run: () => void undo(entry.id) } });
    } else if (firstErr) {
      toast(tx("未能移除：{0}", firstErr), "error");
    }
    if (scanRef.current) void fetchReport(scanRef.current);
  };

  const scanning = !!scanId && (!report || report.state === "running");

  // Events are the fast path; poll the report too in case one was missed.
  useEffect(() => {
    if (!scanning || !scanId) return;
    const t = setInterval(() => void fetchReport(scanId), 1500);
    return () => clearInterval(t);
  }, [scanning, scanId]);
  const title = tr("查找重复");
  const path = report?.folder_path ?? (folder === ROOT_ID ? "/" : "…");

  return (
    <PageFrame label={title} caption={<><NavButtons /><PageTitle>{title}</PageTitle><span class="pf-sub ellipsis" dir="auto" title={path}>{path}</span></>}
      commands={<>
        <div class="cmd-group">
          <Btn tone="tonal" icon="checkCircle" label={tr("选中建议移走项")} disabled={!groups.length} reason={tr("没有重复文件")} onClick={suggest} />
          <Btn tone="ghost" icon="deselect" label={tr("全部保留")} disabled={!sel.size} reason={tr("当前没有选中项")} onClick={() => setSel(new Set())} />
          <Btn tone="quiet" iconOnly icon="refresh" label={tr("重新扫描")} busy={scanning} onClick={() => void start()} />
        </div>
        <div class="cmd-group cmd-end">
          {sel.size ? <span class="cmd-meta">{tr("已选")}{sel.size} {tr("项，约")}{formatBytes(selBytes)}</span> : null}
          <Btn tone="danger" icon="trash" label={sel.size ? tx("移至回收站（{0}）", sel.size) : tr("移至回收站")} busy={busy} disabled={!sel.size} reason={tr("先选择要移走的副本")} onClick={() => void trash()} />
        </div>
      </>}>
      {!g.ok ? <Empty title={tr("当前不能查找重复")}>{g.reason}</Empty>
        : error ? <div class="canvas-pad"><ErrorPanel error={error} onRetry={() => void start()} context={`duplicates_scan folder=${folder}`} /></div>
          : scanning ? (
            <div class="dup-scan" role="status" aria-live="polite">
              <Spinner label={tr("正在扫描")} />
              <p><strong>{tr("正在扫描")}{path}</strong></p>
              <p class="dim tabular">{tr("已检查")}{groupDigits(String(progress.folders))} {tr("个文件夹、")}{groupDigits(String(progress.files))} {tr("个文件")}</p>
              <Progress value={null} label={tr("扫描进度")} />
              <Btn tone="ghost" label={tr("停止")} onClick={() => { if (scanRef.current) void api.duplicatesCancel(scanRef.current); }} />
            </div>
          ) : report?.state === "cancelled" ? (
            <Empty title={tr("已停止扫描")} action={<Btn tone="tonal" icon="refresh" label={tr("重新扫描")} onClick={() => void start()} />} />
          ) : report && !groups.length ? (
            <Empty title={tr("没有发现重复文件")} action={<Btn tone="tonal" label={tr("返回文件夹")} onClick={() => navigate({ page: "files", folder })} />}>
              {tr("已检查")}{groupDigits(String(report.files_scanned))} {tr("个文件。只有哈希与大小都相同的文件才算重复。")}</Empty>
          ) : report ? (
            <div class="dup-scroll">
              <div class="dup-summary">
                <span><strong>{groups.length}</strong> {tr("组重复，最多可释放")}<strong>{formatBytes(report.reclaimable_bytes)}</strong></span>
                <span class="dim">{tr("检查了")}{groupDigits(String(report.folders_scanned))} {tr("个文件夹、")}{groupDigits(String(report.files_scanned))} {tr("个文件")}{report.unhashed_files ? tx("；{0} 个文件没有哈希，无法比较", groupDigits(String(report.unhashed_files))) : ""}{report.skipped.length ? tx("；跳过 {0} 个无法读取的文件夹", report.skipped.length) : ""}</span>
              </div>
              {groups.slice(0, limit).map((gr) => {
                const keep = keeperOf(gr);
                const first = gr.entries[0];
                return (
                  <section class="dup-group" aria-label={first.name}>
                    <header class="dup-head">
                      <KindTile entry={first} />
                      <span class="ellipsis dup-name" dir="auto" title={first.name}>{first.name}</span>
                      <span class="dim tabular">{formatBytes(gr.size_bytes)} × {gr.entries.length}</span>
                      <Badge>{tr("可释放")}{formatBytes((BigInt(gr.size_bytes) * BigInt(gr.entries.length - 1)).toString())}</Badge>
                    </header>
                    <ul class="dup-items">
                      {gr.entries.map((e) => {
                        const on = sel.has(e.id);
                        const ro = !!e.read_only;
                        return (
                          <li class={`dup-item${on ? " on" : ""}`}>
                            <label class="dup-check">
                              <input type="checkbox" checked={on} disabled={ro} onChange={() => toggle(e.id, gr)} aria-label={tx("选择 {0}", e.path)} />
                              <span class="dup-path ellipsis" dir="auto" title={e.path}>{e.path}</span>
                            </label>
                            {e.id === keep ? <Badge tone="ok">{tr("建议保留")}</Badge> : ro ? <Badge>{tr("只读共享")}</Badge> : null}
                            <span class="dim tabular">{formatDateTime(e.modified_at)}</span>
                            <Btn tone="quiet" iconOnly size="sm" icon="folderOpen" label={tr("打开所在文件夹")} onClick={() => navigate({ page: "files", folder: e.parent_id ?? ROOT_ID })} />
                          </li>
                        );
                      })}
                    </ul>
                  </section>
                );
              })}
              {groups.length > limit ? <button type="button" class="link-btn more-btn" onClick={() => setLimit(limit + 100)}>{tr("显示更多（还有")}{groups.length - limit} {tr("组）")}</button> : null}
              <p class="dim task-note"><Icon name="info" size={14} />{tr("建议保留每组中最早的一份；共享给你的只读文件不会被选中。")}</p>
            </div>
          ) : <div class="canvas-pad"><Spinner /></div>}
    </PageFrame>
  );
}
