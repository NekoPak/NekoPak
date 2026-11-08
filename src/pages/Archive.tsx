import { tr, tx } from "../lib/i18n";
/**
 * Browse a zip / rar / 7z in the cloud and extract it server-side
 * (PROPOSAL UI-001 §P19). Nothing is downloaded to this computer.
 */
import { useEffect, useRef, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import type { ArchiveEntry, ArchiveListing } from "../lib/contract-proposals";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { formatBytes, formatDateTime } from "../lib/format";
import { useStore } from "../lib/store";
import { app, gate } from "../state/app";
import { navigate } from "../state/route";
import { toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { KindTile } from "../components/FileTable";
import { Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { confirmDialog, pickFolder } from "../components/dialogs";
import { loadJobs } from "../state/jobs";
import { NavButtons } from "./Files";
import { downloadEntries } from "./fileActions";
import { onSessionReset } from "../state/session";

/** Password refs live only for this window session, keyed by archive id. */
const unlocked = new Map<string, string>();
onSessionReset(() => unlocked.clear());

function asEntry(a: ArchiveEntry): RemoteEntry {
  return { id: a.path, name: a.name, kind: a.kind, size_bytes: a.size_bytes, modified_at: a.modified_at };
}

export function ArchivePage({ file, path }: { file: string; path: string }) {
  const epoch = useStore(app, (s) => s.epoch);
  const demo = useStore(app, (x) => x.runtime === "mock");
  const [entry, setEntry] = useState<RemoteEntry | null>(null);
  const [listing, setListing] = useState<ArchiveListing | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [pw, setPw] = useState("");
  const [pwErr, setPwErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pwRef = useRef<HTMLInputElement>(null);
  const g = gate("archive");

  const load = () => {
    setError(null);
    setListing(null);
    setSel(new Set());
    api.archiveList(file, path, unlocked.get(file)).then(setListing, (e) => setError(errorOf(e)));
  };
  useEffect(() => {
    api.filesStat(file).then(setEntry, () => setEntry(null));
  }, [file, epoch]);
  useEffect(() => { if (g.ok) load(); }, [file, path, epoch]);
  useEffect(() => { if (listing?.needs_password) pwRef.current?.focus(); }, [listing?.needs_password]);

  const unlock = async () => {
    if (!pw) return setPwErr(tr("请输入解压密码"));
    setBusy(true);
    setPwErr(null);
    try {
      const r = await api.archiveUnlock(file, pw);
      unlocked.set(file, r.password_ref);
      setPw("");
      load();
    } catch (e) {
      setPwErr(errorOf(e).message);
    } finally {
      setPw("");
      if (pwRef.current) pwRef.current.value = "";
      setBusy(false);
    }
  };

  const extract = async (paths: string[] | "all") => {
    const dest = !demo
      ? await confirmDialog({ title: tr("解压到压缩包所在文件夹？"), body: <p>{tr("服务当前只支持在压缩包所在文件夹内解压。")}</p>, confirmLabel: tr("解压到这里") }) ? { id: entry?.parent_id ?? ROOT_ID, path: entry?.name ?? tr("所在文件夹") } : undefined
      : await pickFolder(paths === "all" ? tr("解压全部到…") : tx("解压所选 {0} 项到…", paths.length), tr("解压到这里"), [], entry?.parent_id ?? ROOT_ID);
    if (!dest) return;
    try {
      await api.archiveExtract(file, paths, dest.id, newIdempotencyKey(), unlocked.get(file));
      void loadJobs();
      toast(tx("已开始在云端解压到“{0}”", dest.path), "success", { action: { label: tr("查看进度"), run: () => navigate({ page: "transfers", tab: "active" }) } });
    } catch (e) {
      const err = errorOf(e);
      toast(err.message, "error", { error: err });
    }
  };

  const parts = path ? path.split("/") : [];
  const go = (p: string) => navigate({ page: "archive", file, path: p });
  const name = entry?.name ?? tr("压缩包");
  const rows = listing?.entries ?? [];
  const allOn = rows.length > 0 && rows.every((r) => sel.has(r.path));

  return (
    <PageFrame label={name}
      caption={<>
        <NavButtons up={() => (parts.length ? go(parts.slice(0, -1).join("/")) : navigate({ page: "files", folder: entry?.parent_id ?? ROOT_ID }))} />
        <PageTitle>{name}</PageTitle>
        {parts.length ? (
          <nav class="mini-crumbs" aria-label={tr("压缩包内位置")}>
            <a href="#" onClick={(e) => { e.preventDefault(); go(""); }}>{tr("根")}</a>
            {parts.map((p, i) => (
              <>
                <Icon name="chevronRight" size={16} class="crumb-sep" />
                {i === parts.length - 1 ? <span aria-current="location" dir="auto">{p}</span>
                  : <a href="#" dir="auto" onClick={(e) => { e.preventDefault(); go(parts.slice(0, i + 1).join("/")); }}>{p}</a>}
              </>
            ))}
          </nav>
        ) : null}
      </>}
      commands={<>
        <div class="cmd-group">
          <Btn tone="ghost" icon="folderOpen" label={tr("所在文件夹")} onClick={() => navigate({ page: "files", folder: entry?.parent_id ?? ROOT_ID })} />
          <Btn tone="ghost" icon="download" label={tr("下载压缩包")} disabled={!entry} reason={tr("正在读取")} onClick={() => entry && void downloadEntries([entry])} />
          {sel.size ? <span class="sel-count">{tr("已选")}{sel.size} {tr("项")}</span> : null}
        </div>
        <div class="cmd-group cmd-end">
          {sel.size ? <Btn tone="tonal" icon="unarchive" label={tr("解压所选")} onClick={() => void extract([...sel])} /> : null}
          <Btn tone="primary" icon="unarchive" label={tr("全部解压")} disabled={!listing?.supported || listing.needs_password} reason={listing?.needs_password ? tr("先输入解压密码") : listing?.reason ?? tr("正在读取")} onClick={() => void extract("all")} />
        </div>
      </>}>
      {!g.ok ? <Empty title={tr("暂不能浏览压缩包")}>{g.reason}</Empty>
        : error ? <div class="canvas-pad"><ErrorPanel error={error} onRetry={load} context={`archive_list file=${file}`} /></div>
          : !listing ? <div class="canvas-pad"><Spinner label={tr("正在读取压缩包")} /></div>
            : !listing.supported ? (
              <Empty title={tr("不能在线打开这个压缩包")} action={entry ? <Btn tone="tonal" icon="download" label={tr("下载到本机")} onClick={() => void downloadEntries([entry])} /> : null}>{listing.reason}</Empty>
            ) : listing.needs_password ? (
              <form class="arc-lock" onSubmit={(e) => { e.preventDefault(); if (!busy) void unlock(); }}>
                <Icon name="key" size={32} class="arc-lock-icon" />
                <h2>{tr("需要解压密码")}</h2>
                <label class="signin-field">
                  <span>{tr("密码")}</span>
                  <input ref={pwRef} class="input" type="password" autocomplete="off" value={pw} aria-invalid={pwErr ? "true" : undefined}
                    onInput={(e) => { setPw((e.target as HTMLInputElement).value); setPwErr(null); }} />
                </label>
                {pwErr ? <p class="field-error" role="alert">{pwErr}</p> : null}
                <Btn type="submit" tone="primary" label={tr("打开")} busy={busy} />
                {demo ? <p class="dim signin-demo"><Icon name="flask" size={14} />{tr("演示：密码是 nekopak。")}</p> : null}
              </form>
            ) : !rows.length ? <Empty title={tr("这个文件夹是空的")} />
              : (
                <div class="arc-list" role="table" aria-label={tx("{0} 的内容", name)}>
                  <div class="arc-row arc-head" role="row">
                    <span role="columnheader" class="arc-check">
                      <input type="checkbox" checked={allOn} aria-label={allOn ? tr("取消全选") : tr("全选")} onChange={() => setSel(allOn ? new Set() : new Set(rows.map((r) => r.path)))} />
                    </span>
                    <span role="columnheader">{tr("名称")}</span>
                    <span role="columnheader" class="arc-size">{tr("大小")}</span>
                    <span role="columnheader" class="arc-date">{tr("修改时间")}</span>
                  </div>
                  {rows.map((r) => {
                    const on = sel.has(r.path);
                    return (
                      <div class={`arc-row${on ? " on" : ""}`} role="row" onDblClick={() => r.kind === "folder" && go(r.path)}>
                        <span role="cell" class="arc-check">
                          <input type="checkbox" checked={on} aria-label={tx("选择 {0}", r.name)} onChange={() => { const n = new Set(sel); if (on) n.delete(r.path); else n.add(r.path); setSel(n); }} />
                        </span>
                        <span role="cell" class="arc-name">
                          <KindTile entry={asEntry(r)} />
                          {r.kind === "folder"
                            ? <button type="button" class="link-btn ellipsis" dir="auto" onClick={() => go(r.path)}>{r.name}</button>
                            : <span class="ellipsis" dir="auto" title={r.name}>{r.name}</span>}
                        </span>
                        <span role="cell" class="arc-size tabular">{formatBytes(r.size_bytes)}</span>
                        <span role="cell" class="arc-date dim">{r.modified_at ? formatDateTime(r.modified_at) : "—"}</span>
                      </div>
                    );
                  })}
                </div>
              )}
    </PageFrame>
  );
}
