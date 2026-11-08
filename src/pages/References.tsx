import { useEffect, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import type { ReferenceManifest } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { entrySource } from "../lib/entry-source";
import { formatBytes, formatDateTime } from "../lib/format";
import { tr, tx } from "../lib/i18n";
import { useStore } from "../lib/store";
import { app, gate, signedIn } from "../state/app";
import { navigate } from "../state/route";
import { sessionGeneration } from "../state/session";
import { noteCreated } from "../state/transfers";
import { toast } from "../state/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { Badge, Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { confirmDialog, pickFolder } from "../components/dialogs";
import { useRefreshKey } from "./common";

const STATE: Record<ReferenceManifest["state"], string> = {
  get prepared() { return tr("清单已生成"); }, get uploading() { return tr("清单上传中"); }, get verified() { return tr("云端清单已核验"); },
  get partial() { return tr("部分完成，需处理"); }, get archived() { return tr("已归档"); }, get restored() { return tr("已取回"); },
};
const ENTRY_STATE: Record<ReferenceManifest["entries"][number]["state"], string> = {
  get recorded() { return tr("已记录"); }, get trashed() { return tr("已移至回收站"); }, get restored() { return tr("已取回"); },
  get pending_upload() { return tr("等待上传"); }, get outcome_unknown() { return tr("结果待核实"); }, get failed() { return tr("失败"); },
};

export function referenceActions(manifest: ReferenceManifest, attemptedRestore = false) {
  return {
    store: manifest.state === "prepared",
    archive: manifest.state === "verified" || (manifest.state === "partial" && !attemptedRestore && !manifest.entries.some(item => item.state === "restored" || item.state === "pending_upload")),
    restore: manifest.state === "archived" || manifest.state === "partial",
    recreate: attemptedRestore && manifest.state === "partial" && manifest.entries.some(item => item.state === "failed"),
  };
}

function titleOf(manifest: ReferenceManifest): string {
  const first = manifest.entries[0];
  return first ? first.path.split("/")[0] || first.entry.name : formatDateTime(manifest.created_at);
}

async function create(ids: string[]): Promise<void> {
  const generation = sessionGeneration();
  if (!app.get().app?.feature_flags.reference_archive || !gate("stat").ok || ids.some(id => entrySource(id) !== "drive")) return void toast(tr("当前版本尚未启用引用归档"), "info");
  try {
    const manifest = await api.referenceCreate(ids, newIdempotencyKey());
    if (generation !== sessionGeneration()) return;
    toast(tr("清单已生成，原文件未更改"), "success"); navigate({ page: "references", manifest: manifest.manifest_id });
  } catch (e) { if (generation === sessionGeneration()) toast(errorOf(e).message, "error"); }
}

export function createReferenceArchive(entries: RemoteEntry[]): Promise<void> { return create(entries.map(entry => entry.id)); }

export function ReferencesPage({ manifest: requested }: { manifest?: string }) {
  const state = useStore(app, value => value);
  const enabled = state.app?.feature_flags.reference_archive === true && signedIn(state);
  const refresh = useRefreshKey();
  const [manifests, setManifests] = useState<ReferenceManifest[]>();
  const [error, setError] = useState<ClientError>();
  const [loading, setLoading] = useState(false), [busy, setBusy] = useState(false), [revision, setRevision] = useState(0);
  const [restoreAttempts, setRestoreAttempts] = useState<Set<string>>(new Set());
  const selected = manifests?.find(item => item.manifest_id === requested) ?? manifests?.[0];
  const reload = () => setRevision(value => value + 1);
  useEffect(() => {
    if (!enabled) return;
    let alive = true; setLoading(true); setError(undefined);
    void api.referenceList().then(rows => { if (alive) setManifests(rows); }, e => { if (alive) setError(errorOf(e)); }).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [enabled, state.sessionEpoch, refresh, revision]);
  useEffect(() => {
    if (!enabled || !selected) return;
    let alive = true, pending = false;
    const check = async () => {
      if (pending) return; pending = true;
      try { const result = await api.referenceStatus(selected.manifest_id); if (alive) setManifests(rows => rows?.map(item => item.manifest_id === result.manifest_id ? result : item)); }
      catch (e) { if (alive) setError(errorOf(e)); }
      finally { pending = false; }
    };
    void check(); const timer = setInterval(() => void check(), 3000);
    return () => { alive = false; clearInterval(timer); };
  }, [enabled, selected?.manifest_id]);

  const choose = async () => {
    const generation = sessionGeneration();
    const folder = await pickFolder(tr("选择引用归档目录"), tr("创建只读清单"));
    if (!folder || generation !== sessionGeneration()) return;
    setBusy(true); await create([folder.id]); setBusy(false); reload();
  };
  const run = async (action: "store" | "archive" | "restore" | "recreate") => {
    if (!selected || busy) return;
    const generation = sessionGeneration();
    const id = selected.manifest_id;
    if (action === "archive" && !await confirmDialog({ title: tr("将清单中的文件移至回收站？"), body: <p>{tr("云端清单已经核验。归档会逐个将文件移至回收站，保留目录，不执行永久删除。引用清单不包含文件内容；回收站失效后不保证能够取回。")}</p>, confirmLabel: tr("确认归档"), acknowledge: tr("我理解引用清单不是文件内容备份"), danger: true })) return;
    if (action === "recreate" && !await confirmDialog({ title: tr("允许按 GCID 重建缺失文件？"), body: <p>{tr("先尝试从回收站取回原文件。只有无法取回时才允许按 GCID 重建；服务可能不再保存内容，重建可能失败，也可能占用空间。")}</p>, confirmLabel: tr("允许重建并取回"), acknowledge: tr("我接受重建可能失败或占用空间") })) return;
    if (generation !== sessionGeneration()) return;
    setBusy(true); setError(undefined);
    try {
      if (action === "store") { await api.referenceStoreManifest(id, `${id}:store`); noteCreated(); }
      else if (action === "archive") await api.referenceArchive(id, `${id}:archive`);
      else {
        const result = await api.referenceRestore(id, action === "recreate", `${id}:${action}`);
        if (generation === sessionGeneration() && action === "restore") setRestoreAttempts(previous => new Set([...previous, result.manifest_id]));
      }
      if (generation !== sessionGeneration()) return;
      const result = await api.referenceStatus(id); setManifests(rows => rows?.map(item => item.manifest_id === id ? result : item));
    } catch (e) { if (generation === sessionGeneration()) setError(errorOf(e)); }
    finally { if (generation === sessionGeneration()) setBusy(false); }
  };
  const actions = selected && referenceActions(selected, restoreAttempts.has(selected.manifest_id));
  return <PageFrame label={tr("引用归档")} caption={<PageTitle>{tr("引用归档")}</PageTitle>} commands={<><Btn label={tr("选择目录创建清单")} icon="plus" tone="primary" disabled={!enabled} reason={signedIn(state) ? tr("当前版本尚未启用引用归档") : tr("需要先登录")} busy={busy} onClick={() => void choose()}/><Btn label={tr("刷新")} icon="refresh" busy={loading} disabled={!enabled} onClick={reload}/></>}>
    <div class="canvas-scroll reference-page"><p class="inline-note">{tr("引用清单记录路径、大小和内容标识，不保存文件内容。分享链接或 GCID 都不能代替可靠备份。")}</p>
      {!enabled ? <Empty title={signedIn(state) ? tr("当前版本尚未启用引用归档") : tr("需要先登录")}/> : <>
        {error ? <ErrorPanel error={error} onRetry={reload}/> : null}
        {!manifests ? <Spinner/> : manifests.length === 0 ? <Empty title={tr("还没有引用清单")}>{tr("在文件右键菜单中创建清单，或选择一个云盘目录。创建清单不会改动原文件。")}</Empty> : <>
          <label class="reference-selector">{tr("引用清单")}<select class="select" value={selected?.manifest_id} onChange={e => navigate({ page: "references", manifest: (e.target as HTMLSelectElement).value })}>{manifests.map(item => <option key={item.manifest_id} value={item.manifest_id}>{titleOf(item)} · {formatDateTime(item.created_at)}</option>)}</select></label>
          {selected && actions ? <section class="reference-detail">
            <div class="spread"><h2 dir="auto">{titleOf(selected)}</h2><Badge tone={selected.state === "partial" ? "warn" : selected.state === "verified" || selected.state === "restored" ? "ok" : "neutral"}>{STATE[selected.state]}</Badge></div>
            <p class="dim">{tx("共 {0} 项", selected.entries.length)} · {formatDateTime(selected.created_at)}</p>
            <details><summary>{tr("清单校验信息")}</summary><dl class="kv"><dt>SHA-256</dt><dd class="mono wrap">{selected.content_sha256}</dd><dt>{tr("清单 ID")}</dt><dd class="mono wrap">{selected.manifest_id}</dd></dl></details>
            <div class="reference-actions"><Btn label={tr("上传并核验清单")} icon="upload" busy={busy} disabled={!actions.store || !gate("upload").ok} reason={tr("先创建完整清单，再等待云端核验")} onClick={() => void run("store")}/><Btn label={tr("归档文件…")} tone="danger" busy={busy} disabled={!actions.archive || !gate("trash").ok} reason={tr("清单上传并核验完成后才能归档")} onClick={() => void run("archive")}/><Btn label={tr("从回收站取回")} icon="restore" busy={busy} disabled={!actions.restore || !gate("restore").ok} reason={tr("归档后可尝试取回原文件")} onClick={() => void run("restore")}/>{actions.recreate ? <Btn label={tr("允许重建缺失文件…")} busy={busy} onClick={() => void run("recreate")}/> : null}</div>
            {selected.manifest_task_id ? <Btn label={tr("查看清单上传任务")} tone="quiet" onClick={() => navigate({ page: "transfers", tab: "active" })}/> : null}
            <ul class="reference-entries">{selected.entries.map(item => <li key={item.path}><span class="reference-path" dir="auto">{item.path}</span><span class="dim">{item.entry.kind === "file" ? formatBytes(item.entry.size_bytes) : tr("文件夹")}</span><Badge tone={item.state === "failed" || item.state === "outcome_unknown" ? "warn" : "neutral"}>{ENTRY_STATE[item.state]}</Badge>{item.error ? <div class="reference-error"><ErrorPanel error={item.error} compact/></div> : null}</li>)}</ul>
          </section> : null}
        </>}
      </>}
    </div>
  </PageFrame>;
}
