import { tr, tx } from "../lib/i18n";
/** “分享…” dialog and the “我的分享” page (PROPOSAL UI-001 §P22). */
import { useEffect, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import type { SharedFolder, ShareInfo } from "../lib/contract-proposals";
import { api, collectAll, errorOf, newIdempotencyKey } from "../lib/api";
import { ERROR_COPY } from "../lib/errors";
import { formatDateTime, formatRelative } from "../lib/format";
import { useStore } from "../lib/store";
import { app, fileGate, gate, sharedFileGate, signedIn } from "../state/app";
import { openDialog, openMenu, toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Badge, Btn, copyWithToast, Empty, ErrorPanel, Segmented, Spinner, Switch } from "../components/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { confirmDialog, pickFolder } from "../components/dialogs";
import { sessionGeneration } from "../state/session";
import { formatBytes } from "../lib/format";
import { NavButtons } from "./Files";
import { useRefreshKey } from "./common";
import { fileKind } from "../lib/names";
import { entrySource } from "../lib/entry-source";
import { downloadWithOptions } from "./download";
import { openPlayer } from "./lazy";

type Expiry = "1" | "7" | "30" | "never";

function shareText(s: ShareInfo): string {
  return s.code ? tx("{0}\n提取码：{1}", s.url, s.code) : s.url;
}

function ShareBody({ entries, close }: { entries: RemoteEntry[]; close: (v?: unknown) => void }) {
  const [expiry, setExpiry] = useState<Expiry>("7");
  const [code, setCode] = useState(true);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ShareInfo | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [key] = useState(newIdempotencyKey);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await api.shareCreate(entries.map((e) => e.id), expiry === "never" ? null : (Number(expiry) as 1 | 7 | 30), code, key);
      setResult(s);
    } catch (e) {
      setError(errorOf(e));
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    return (
      <div class="share-done">
        <p class="share-ok"><Icon name="checkCircle" filled />{tr("已创建分享")}{result.expires_at ? tx("，{0} 失效", formatDateTime(result.expires_at)) : tr("，永久有效")}</p>
        <div class="share-link">
          <code class="wrap" dir="ltr">{result.url}</code>
          {result.code ? <span>{tr("提取码")} <strong class="mono">{result.code}</strong></span> : null}
        </div>
        <p class="dim">{tr("任何拿到链接")}{result.code ? tr("和提取码") : ""}{tr("的人都能查看与转存。可以在“我的分享”中随时取消。")}</p>
        <div class="dialog-actions">
          <Btn tone="ghost" label={tr("完成")} onClick={() => close(true)} />
          <Btn tone="primary" icon="copy" label={result.code ? tr("复制链接和提取码") : tr("复制链接")} data-autofocus="" onClick={() => void copyWithToast(shareText(result), tr("分享链接"))} />
        </div>
      </div>
    );
  }

  return (
    <form onSubmit={(e) => { e.preventDefault(); if (!busy) void create(); }}>
      <p class="muted pad-bottom" dir="auto">{entries.length === 1 ? entries[0].name : tx("{0} 等 {1} 项", entries[0].name, entries.length)}</p>
      <div class="set-row compact">
        <div class="set-text"><span class="set-label">{tr("有效期")}</span></div>
        <div class="set-control">
          <Segmented<Expiry> label={tr("有效期")} value={expiry} onChange={setExpiry}
            options={[{ value: "1", label: tr("1 天") }, { value: "7", label: tr("7 天") }, { value: "30", label: tr("30 天") }, { value: "never", label: tr("永久") }]} />
        </div>
      </div>
      <div class="set-row compact">
        <div class="set-text"><span class="set-label">{tr("提取码")}</span><p class="set-hint">{tr("打开链接时需要输入 4 位提取码。")}</p></div>
        <div class="set-control"><Switch label={tr("提取码")} checked={code} onChange={setCode} /></div>
      </div>
      {error ? <ErrorPanel error={error} compact /> : null}
      <div class="dialog-actions">
        <Btn tone="ghost" label={tr("取消")} onClick={() => close()} />
        <Btn type="submit" tone="primary" icon="share" label={tr("创建分享链接")} busy={busy} data-autofocus="" />
      </div>
    </form>
  );
}

export function openShareDialog(entries: RemoteEntry[]): void {
  const g = fileGate("share", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不能分享"), "info");
  if (!entries.length) return;
  void openDialog({ title: tr("分享"), width: 480, render: (close) => <ShareBody entries={entries} close={close} /> });
}

/* ------------------------------------------------------------------ */

const STATE_BADGE: Record<ShareInfo["state"], { label: string; tone: "ok" | "neutral" | "warn" }> = {
  active: { get label() { return tr("有效"); }, tone: "ok" },
  expired: { get label() { return tr("已过期"); }, tone: "neutral" },
  cancelled: { get label() { return tr("已取消"); }, tone: "neutral" },
  unavailable: { get label() { return tr("不可用"); }, tone: "warn" },
};

export function SharesPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const signed = useStore(app, (s) => signedIn(s));
  const refresh = useRefreshKey();
  const [rows, setRows] = useState<ShareInfo[] | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const g = gate("share");

  const load = () => {
    setError(null);
    collectAll(() => api.sharesList(), cursor => api.sharesList(cursor)).then(setRows, (e) => setError(errorOf(e)));
  };
  useEffect(load, [epoch, refresh]);

  const cancel = async (s: ShareInfo) => {
    const ok = await confirmDialog({ title: tr("取消分享？"), body: <p>{tr("取消后，“")}<span dir="auto">{s.title}</span>{tr("”的分享链接立即失效，已转存的副本不受影响。")}</p>, confirmLabel: tr("取消分享"), danger: true });
    if (!ok) return;
    try {
      const res = await api.sharesCancel([s.share_id], newIdempotencyKey());
      if (res.failed.length) toast(res.failed[0].error.message, "error");
      else toast(tr("已取消分享"), "success");
      load();
    } catch (e) {
      const err = errorOf(e);
      toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
    }
  };

  return (
    <PageFrame label={tr("我的分享")} caption={<><NavButtons /><PageTitle>{tr("我的分享")}</PageTitle></>}
      commands={<><Btn label={tr("打开分享")} icon="link" disabled={!g.ok||app.get().runtime!=="tauri"} reason={!g.ok?g.reason:tr("浏览器演示不读取真实分享")} onClick={()=>void openSharedLink()}/><div class="cmd-group cmd-end"><span class="cmd-meta">{rows ? tx("{0} 个分享", rows.length) : ""}</span><Btn tone="quiet" iconOnly icon="refresh" label={tr("刷新")} onClick={load} /></div></>}>
      {!g.ok && signed ? <div class="canvas-pad"><div class="inline-note warn">{g.reason}</div></div> : null}
      {error ? <div class="canvas-pad"><ErrorPanel error={error} onRetry={load} /></div>
        : !rows ? <div class="canvas-pad"><Spinner /></div>
          : !rows.length ? <Empty title={tr("还没有分享")}>{tr("在文件的右键菜单中选择“分享…”创建链接。")}</Empty>
            : (
              <div class="share-list" role="list">
                {rows.map((s) => {
                  const b = STATE_BADGE[s.state];
                  return (
                    <div class={`share-row st-${s.state}`} role="listitem">
                      <span class="tile kind-folder"><Icon name="share" size={20} /></span>
                      <div class="share-main">
                        <span class="ellipsis share-title" dir="auto" title={s.title}>{s.title}</span>
                        <span class="dim share-meta">
                          {s.item_count > 1 ? tx("{0} 项 · ", s.item_count) : ""}{tr("创建于")} {formatRelative(s.created_at)} · {s.expires_at ? (s.state === "expired" ? tx("已于 {0} 过期", formatDateTime(s.expires_at)) : tx("{0} 失效", formatDateTime(s.expires_at))) : s.expires_known === false ? tr("有效期未提供") : tr("永久有效")}
                          {s.view_count !== undefined ? tx(" · 浏览 {0} 次", s.view_count) : ""}{s.save_count !== undefined ? tx(" · 转存 {0} 次", s.save_count) : ""}
                        </span>
                      </div>
                      <Badge tone={b.tone}>{b.label}</Badge>
                      <Btn tone="quiet" iconOnly icon="copy" label={tr("复制链接")} disabled={s.state !== "active"} reason={tr("分享已失效")} onClick={() => void copyWithToast(shareText(s), tr("分享链接"))} />
                      <Btn tone="quiet" iconOnly icon="more" label={tr("更多操作")} onClick={(e) => {
                        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                        openMenu(r.right - 220, r.bottom + 4, [
                          { id: "copy", label: tr("复制链接"), icon: "link", disabled: s.state !== "active", reason: tr("分享已失效"), run: () => void copyWithToast(shareText(s), tr("分享链接")) },
                          { id: "cancel", label: tr("取消分享"), icon: "linkOff", danger: true, separatorBefore: true, disabled: s.state !== "active" || !g.ok, reason: !g.ok ? g.reason : tr("分享已失效"), run: () => void cancel(s) },
                        ], s.title);
                      }} />
                    </div>
                  );
                })}
              </div>
            )}
    </PageFrame>
  );
}

export function parseShareInput(input:string):string|null {
  const value=input.trim();if(/^[A-Za-z0-9_-]{4,256}$/.test(value))return value;
  try{const url=new URL(value);if(url.protocol!=="https:"||!['mypikpak.com','www.mypikpak.com','mypikpak.net','pikpak.me','pikpakdrive.com'].includes(url.hostname)||url.username||url.password)return null;const match=/^\/s\/([A-Za-z0-9_-]{4,256})\/?$/.exec(url.pathname);return match?.[1]??null;}catch{return null;}
}
function SharedLinkBody(){
  const state = useStore(app, s => s);
  const [input,setInput]=useState("");const[code,setCode]=useState("");const[folder,setFolder]=useState<SharedFolder>();
  const[stack,setStack]=useState<{id:string;name:string}[]>([]);const[selected,setSelected]=useState<Set<string>>(new Set());
  const[busy,setBusy]=useState(false);const[error,setError]=useState<ClientError>();const generation=sessionGeneration();
  const previewGate = sharedFileGate("stream_url", state), downloadGate = sharedFileGate("download", state);
  const selectedEntries = folder?.entries.filter(entry => selected.has(entry.id)) ?? [];
  const onlyFiles = selectedEntries.length > 0 && selectedEntries.every(entry => entry.kind === "file");
  const reportError = (e: unknown) => { if (generation === sessionGeneration()) setError(errorOf(e)); };
  const open=async()=>{const id=parseShareInput(input);if(!id)return setError({code:"CONFLICT",message:tr("请输入有效的 PikPak 分享链接或分享 ID"),retryable:false});setBusy(true);setError(undefined);
    try{const result=await api.shareOpen(id,code.trim()||undefined);if(generation!==sessionGeneration())return;setFolder(result);setStack([{id:result.parent_id,name:result.title}]);setSelected(new Set());}catch(e){reportError(e);}finally{setCode("");setBusy(false);}};
  const read=async(parent:string,name?:string,cursor?:string)=>{if(!folder||busy)return;setBusy(true);setError(undefined);try{const result=await api.shareFolder(folder.access_ref,parent,cursor);if(generation!==sessionGeneration())return;setFolder(cursor?{...result,entries:[...folder.entries,...result.entries]}:result);if(!cursor){setSelected(new Set());if(name)setStack(prev=>[...prev,{id:parent,name}]);else setStack(prev=>prev.slice(0,prev.findIndex(p=>p.id===parent)+1));}}catch(e){reportError(e);}finally{setBusy(false);}};
  const restore=async()=>{
    if(!folder||!selected.size||busy)return;setBusy(true);setError(undefined);
    try{const destination=await pickFolder(tr("转存到云盘"),tr("保存到这里"));if(!destination||generation!==sessionGeneration())return;const result=await api.shareRestore(folder.access_ref,[...selected],destination.id,newIdempotencyKey());if(result.failed.length)setError(result.failed[0].error);if(result.succeeded.length)toast(tx("已转存 {0} 项",result.succeeded.length),"success");}catch(e){reportError(e);}finally{setBusy(false);}
  };
  const preview = async (entry: RemoteEntry) => {
    if (!folder || busy || !previewGate.ok) return;
    setBusy(true); setError(undefined);
    try { const source = await resolveSharedEntry(folder.access_ref, entry); if (generation === sessionGeneration()) openPlayer(source); }
    catch (e) { reportError(e); } finally { setBusy(false); }
  };
  const download = async (entries: RemoteEntry[]) => {
    if (!folder || busy || !entries.length || !downloadGate.ok || entries.some(entry => entry.kind !== "file")) return;
    setBusy(true); setError(undefined);
    try {
      const sources: RemoteEntry[] = [];
      for (const entry of entries) { if (generation !== sessionGeneration()) return; sources.push(await resolveSharedEntry(folder.access_ref, entry)); }
      if (generation === sessionGeneration()) await downloadWithOptions(sources);
    } catch (e) { reportError(e); } finally { setBusy(false); }
  };
  return <div class="shared-preview">
    <form onSubmit={e=>{e.preventDefault();if(!busy)void open();}}><label class="field"><span>{tr("分享链接")}</span><input class="input" value={input} onInput={e=>setInput((e.target as HTMLInputElement).value)} placeholder="https://mypikpak.com/s/…"/></label><label class="field"><span>{tr("提取码")}</span><input class="input" type="password" autoComplete="off" value={code} onInput={e=>setCode((e.target as HTMLInputElement).value)}/></label><Btn type="submit" label={tr("预览分享")} icon="search" busy={busy}/></form>
    {error?<ErrorPanel error={error}/>:null}
    {folder?<><nav class="mini-crumbs">{stack.map(p=><button class="link-btn" type="button" disabled={busy} onClick={()=>void read(p.id)}>{p.name}</button>)}</nav><ul class="shared-entries">{folder.entries.map(entry=><li key={entry.id}><input type="checkbox" checked={selected.has(entry.id)} disabled={busy} aria-label={tx("选择 {0}",entry.name)} onChange={()=>setSelected(old=>{const next=new Set(old);if(next.has(entry.id))next.delete(entry.id);else next.add(entry.id);return next;})}/><Icon name={entry.kind==="folder"?"folder":"file"}/>{entry.kind==="folder"?<button class="link-btn" type="button" disabled={busy} onClick={()=>void read(entry.id,entry.name)}>{entry.name}</button>:<span dir="auto">{entry.name}</span>}<span class="dim tabular">{entry.kind==="file"?formatBytes(entry.size_bytes):"—"}</span>
      {entry.kind === "file" ? <div class="shared-file-actions">
        {["video", "audio", "image"].includes(fileKind(entry.name, entry.kind, entry.mime_type)) ? <Btn label={tr("播放或查看")} icon="playCircle" size="sm" disabled={busy || !previewGate.ok} reason={previewGate.reason} onClick={() => void preview(entry)}/> : null}
        <Btn label={tr("下载原始文件")} icon="download" size="sm" disabled={busy || !downloadGate.ok} reason={downloadGate.reason} onClick={() => void download([entry])}/>
      </div> : null}
    </li>)}</ul>{folder.next_cursor?<Btn label={tr("继续读取")} busy={busy} onClick={()=>void read(folder.parent_id,undefined,folder.next_cursor)}/>:null}
      <p class="dim">{tr("分享文件只读；下载到本地不会转存到云盘。文件夹请进入后选择文件下载，或转存整个文件夹。")}</p>
      <div class="dialog-actions"><Btn label={tr("下载所选原始文件")} disabled={!onlyFiles || !downloadGate.ok} reason={!onlyFiles ? tr("请选择文件；文件夹请进入后选择内容") : downloadGate.reason} busy={busy} onClick={() => void download(selectedEntries)}/><Btn tone="primary" label={tr("转存所选")} disabled={!selected.size} busy={busy} onClick={()=>void restore()}/></div></>:null}
  </div>;
}

/** The core mints the encoded identity; raw shared IDs never enter drive IPC. */
export async function resolveSharedEntry(accessRef: string, entry: RemoteEntry): Promise<RemoteEntry> {
  const source = await api.sharePreviewEntry(accessRef, entry.id);
  if (source.kind !== "file" || entrySource(source.id) !== "shared") throw { code: "INTEGRITY_FAILED", message: tr("分享文件来源校验失败，请重新打开分享"), retryable: false } satisfies ClientError;
  return source;
}
function openSharedLink(){return openDialog({title:tr("打开分享"),width:760,render:()=> <SharedLinkBody/>});}
