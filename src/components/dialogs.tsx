import { tr, tx } from "../lib/i18n";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { ROOT_ID, type LinkPreview, type TorrentPreview } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { formatBytes, formatBytesExact, formatDateTime } from "../lib/format";
import { fileKind, KIND_LABEL, validateName, windowsIssues } from "../lib/names";
import { app, fileGate, gate } from "../state/app";
import { playExternal } from "../pages/lazy";
import { navigate } from "../state/route";
import { noteCreated } from "../state/transfers";
import { openDialog, toast } from "../state/ui";
import { openSide } from "../state/prefs";
import { mobileLayout } from "../lib/mobile";
import { EntryName } from "./FileTable";
import { Icon } from "./Icon";
import { Btn, copyWithToast, ErrorPanel, Field, Spinner } from "./ui";

/* ------------------------------------------------------------------ */
/* Confirm                                                             */
/* ------------------------------------------------------------------ */

export interface ConfirmOptions {
  title: string;
  body: preact.ComponentChildren;
  confirmLabel: string;
  danger?: boolean;
  /** Require ticking a checkbox with this text before confirming. */
  acknowledge?: string;
}

function ConfirmBody({ o, close }: { o: ConfirmOptions; close: (v?: unknown) => void }) {
  const [ack, setAck] = useState(!o.acknowledge);
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (ack) close(true);
      }}
    >
      <div class="confirm-body">{o.body}</div>
      {o.acknowledge ? (
        <label class="ack">
          <input type="checkbox" checked={ack} onChange={(e) => setAck((e.target as HTMLInputElement).checked)} data-autofocus />
          <span>{o.acknowledge}</span>
        </label>
      ) : null}
      <div class="dialog-actions">
        <Btn tone="ghost" label={tr("取消")} onClick={() => close(false)} />
        <Btn
          type="submit"
          tone={o.danger ? "danger" : "primary"}
          label={o.confirmLabel}
          disabled={!ack}
          reason={tr("请先勾选确认项")}
          {...(o.acknowledge ? {} : { "data-autofocus": "" })}
        />
      </div>
    </form>
  );
}

export async function confirmDialog(o: ConfirmOptions): Promise<boolean> {
  const v = await openDialog<boolean>({ title: o.title, width: 460, render: (close) => <ConfirmBody o={o} close={close} /> });
  return v === true;
}

/* ------------------------------------------------------------------ */
/* Name prompt with live validation                                    */
/* ------------------------------------------------------------------ */

function NameBody({ initial, siblings, current, confirmLabel, close }: { initial: string; siblings: string[]; current?: string; confirmLabel: string; close: (v?: unknown) => void }) {
  const [name, setName] = useState(initial);
  const [touched, setTouched] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const check = useMemo(() => validateName(name, siblings, current), [name, siblings, current]);

  useEffect(() => {
    const el = input.current;
    if (!el) return;
    el.focus();
    const dot = current ? name.lastIndexOf(".") : -1;
    el.setSelectionRange(0, dot > 0 ? dot : name.length);
  }, []);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        setTouched(true);
        if (!check.error) close(name);
      }}
    >
      <Field label={tr("名称")} htmlFor="name-input" error={touched || name !== initial ? check.error : undefined}>
        <input id="name-input" ref={input} class="input" value={name} spellcheck={false} autocomplete="off"
          onInput={(e) => setName((e.target as HTMLInputElement).value)} />
      </Field>
      {check.warnings.length ? (
        <ul class="warn-list">
          {check.warnings.map((w) => (
            <li><Icon name="alert" size={14} />{w}</li>
          ))}
        </ul>
      ) : null}
      <div class="dialog-actions">
        <Btn tone="ghost" label={tr("取消")} onClick={() => close()} />
        <Btn type="submit" tone="primary" label={confirmLabel} disabled={!!check.error} reason={check.error} />
      </div>
    </form>
  );
}

export function promptName(title: string, opts: { initial?: string; siblings: string[]; current?: string; confirmLabel: string }): Promise<string | undefined> {
  return openDialog<string>({
    title,
    width: 460,
    render: (close) => <NameBody initial={opts.initial ?? ""} siblings={opts.siblings} current={opts.current} confirmLabel={opts.confirmLabel} close={close} />,
  });
}

/* ------------------------------------------------------------------ */
/* Folder picker                                                       */
/* ------------------------------------------------------------------ */

export interface PickedFolder {
  id: string;
  path: string;
}

function FolderPicker({ exclude, confirmLabel, close, start }: { exclude: Set<string>; confirmLabel: string; close: (v?: unknown) => void; start: string }) {
  const [stack, setStack] = useState<{ id: string; name: string }[]>([{ id: ROOT_ID, name: tr("我的云盘") }]);
  const [items, setItems] = useState<RemoteEntry[] | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const cur = stack[stack.length - 1];

  useEffect(() => {
    if (start === ROOT_ID) return;
    api.filesPath(start).then(
      (chain) => setStack([{ id: ROOT_ID, name: tr("我的云盘") }, ...chain.filter((e) => e.kind === "folder").map((e) => ({ id: e.id, name: e.name }))]),
      () => undefined,
    );
  }, [start]);

  const load = () => {
    setItems(null);
    setError(null);
    api.filesList(cur.id, undefined, 500).then(
      (p) => setItems(p.items.filter((e) => e.kind === "folder")),
      (e) => setError(errorOf(e)),
    );
  };
  useEffect(load, [cur.id]);

  const path = "/" + stack.slice(1).map((s) => s.name).join("/");
  const blocked = exclude.has(cur.id);

  return (
    <div class="picker">
      <nav class="crumbs small" aria-label={tr("目标位置")}>
        {stack.map((s, i) => (
          <>
            {i > 0 ? <Icon name="chevronRight" size={12} /> : null}
            <button type="button" class="crumb" aria-current={i === stack.length - 1 ? "location" : undefined} onClick={() => setStack(stack.slice(0, i + 1))}>
              {s.name}
            </button>
          </>
        ))}
      </nav>
      <div class="picker-list" role="listbox" aria-label={tr("文件夹")}>
        {error ? <ErrorPanel error={error} compact onRetry={load} />
          : items === null ? <Spinner />
            : items.length === 0 ? <p class="muted pad">{tr("没有子文件夹")}</p>
              : items.map((f) => {
                const off = exclude.has(f.id);
                return (
                  <button type="button" role="option" aria-selected="false" class="picker-item" aria-disabled={off ? "true" : undefined}
                    title={off ? tr("不能选择被移动的文件夹本身") : undefined}
                    onDblClick={() => !off && setStack([...stack, { id: f.id, name: f.name }])}
                    onClick={() => !off && setStack([...stack, { id: f.id, name: f.name }])}>
                    <Icon name="folder" />
                    <span dir="auto">{f.name}</span>
                    <Icon name="chevronRight" size={12} class="push" />
                  </button>
                );
              })}
      </div>
      <div class="dialog-actions spread">
        <span class="muted ellipsis" dir="auto" title={path}>{tr("目标：")}{path}</span>
        <div class="row-gap">
          <Btn tone="ghost" label={tr("取消")} onClick={() => close()} />
          <Btn tone="primary" label={confirmLabel} disabled={blocked} reason={tr("不能选择被移动的文件夹本身")} onClick={() => close({ id: cur.id, path } satisfies PickedFolder)} />
        </div>
      </div>
    </div>
  );
}

export function pickFolder(title: string, confirmLabel: string, exclude: string[] = [], start = ROOT_ID): Promise<PickedFolder | undefined> {
  return openDialog<PickedFolder>({ title, width: 520, render: (close) => <FolderPicker exclude={new Set(exclude)} confirmLabel={confirmLabel} close={close} start={start} /> });
}

/* ------------------------------------------------------------------ */
/* Properties                                                          */
/* ------------------------------------------------------------------ */

function Properties({ id, path, onDownload, close }: { id: string; path?: string; onDownload?: (e: RemoteEntry) => void; close: () => void }) {
  const [entry, setEntry] = useState<RemoteEntry | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const load = () => {
    setError(null);
    api.filesStat(id).then(setEntry, (e) => setError(errorOf(e)));
  };
  useEffect(load, [id]);
  if (error) return <ErrorPanel error={error} onRetry={load} />;
  if (!entry) return <Spinner />;
  const issues = windowsIssues(entry.name);
  const stream = fileGate("external_player", id);
  const dl = fileGate("download", id);
  return (
    <div class="props">
      <div class="props-name"><EntryName entry={entry} showWarn={false} /></div>
      <dl class="kv">
        <dt>{tr("类型")}</dt><dd>{KIND_LABEL[fileKind(entry.name, entry.kind, entry.mime_type)]}{entry.mime_type ? <span class="muted">（{entry.mime_type}）</span> : null}</dd>
        {entry.kind === "file" ? (<><dt>{tr("大小")}</dt><dd>{formatBytes(entry.size_bytes)} <span class="muted">{formatBytesExact(entry.size_bytes)}</span></dd></>) : null}
        <dt>{tr("修改时间")}</dt><dd>{formatDateTime(entry.modified_at)} <span class="muted">{entry.modified_at ?? ""}</span></dd>
        {path ? (<><dt>{tr("路径")}</dt><dd class="wrap" dir="auto">{path}</dd></>) : null}
        <dt>ID</dt><dd><code>{entry.id}</code></dd>
        {entry.hash ? (<><dt>{entry.hash.algorithm.toUpperCase()}</dt><dd><code class="wrap">{entry.hash.value}</code></dd></>) : (<><dt>{tr("哈希")}</dt><dd class="muted">{tr("未提供")}</dd></>)}
        {entry.etag ? (<><dt>ETag</dt><dd><code>{entry.etag}</code></dd></>) : null}
      </dl>
      {issues.length ? (
        <ul class="warn-list">{issues.map((i) => <li><Icon name="alert" size={14} />{i}{tr("，下载到 Windows 时将被转义")}</li>)}</ul>
      ) : null}
      <div class="side-actions">
        <div class="row-gap">
          <Btn size="sm" tone="quiet" icon="copy" label={tr("复制路径")} disabled={!path} reason={tr("路径不可用")} onClick={() => void copyWithToast(path ?? "", tr("路径"))} />
          <Btn size="sm" tone="quiet" icon="copy" label={tr("复制 ID")} onClick={() => void copyWithToast(entry.id, " ID")} />
        </div>
        <div class="row-gap">
          {entry.kind === "file" && ["audio", "video"].includes(fileKind(entry.name, entry.kind, entry.mime_type)) ? (
            <Btn icon="playCircle" label={tr("用外部播放器打开")} disabled={!stream.ok} reason={stream.reason} onClick={() => void playExternal(entry)} />
          ) : null}
          {entry.kind === "file" && onDownload ? (
            <Btn tone="primary" icon="download" label={tr("下载")} disabled={!dl.ok} reason={dl.reason} onClick={() => { close(); onDownload(entry); }} />
          ) : null}
        </div>
      </div>
    </div>
  );
}

/** Properties open in the non-modal side island; the list stays interactive. */
export function showProperties(id: string, path?: string, onDownload?: (e: RemoteEntry) => void): Promise<unknown> {
  openSide({ key: `props-${id}`, title: tr("属性"), render: (close) => <Properties id={id} path={path} onDownload={onDownload} close={close} /> });
  return Promise.resolve();
}

/* ------------------------------------------------------------------ */
/* New cloud download                                                  */
/* ------------------------------------------------------------------ */

const MAGNET = /^magnet:\?xt=urn:btih:[a-z0-9]{32,40}/i;
const HTTP = /^https?:\/\/[^\s/]+\.[^\s]+$/i;

export function classifySource(line: string): "magnet" | "http" | "invalid" {
  const s = line.trim();
  if (MAGNET.test(s)) return "magnet";
  if (HTTP.test(s)) return "http";
  return "invalid";
}

interface LineState {
  text: string;
  key: string;
  status: "pending" | "submitting" | "submitted" | "failed";
  error?: ClientError;
  task_id?: string;
}

type PickGroup = "video" | "subtitle" | "image" | "other";
const SUB_RE = /\.(ass|ssa|srt|vtt|sub|sup)$/i;
function pickGroup(path: string): PickGroup {
  if (SUB_RE.test(path)) return "subtitle";
  const k = fileKind(path, "file");
  return k === "video" ? "video" : k === "image" ? "image" : "other";
}
const GROUP_LABEL: Record<PickGroup, string> = { video: tr("视频"), get subtitle() { return tr("字幕"); }, image: tr("图片"), other: tr("其他") };

/** What a single link contains; the user picks which files to save (§P20). */
function LinkPick({ preview, picked, setPicked }: { preview: LinkPreview; picked: Set<number>; setPicked: (s: Set<number>) => void }) {
  const files = preview.files;
  const groups = (["video", "subtitle", "image", "other"] as PickGroup[])
    .map((g) => ({ g, idx: files.filter((f) => pickGroup(f.path) === g).map((f) => f.index) }))
    .filter((x) => x.idx.length);
  const bytes = files.filter((f) => picked.has(f.index)).reduce((sum, f) => sum + BigInt(f.size_bytes), 0n);
  const toggleGroup = (idx: number[]) => {
    const all = idx.every((i) => picked.has(i));
    const next = new Set(picked);
    for (const i of idx) (all ? next.delete(i) : next.add(i));
    setPicked(next);
  };
  return (
    <div class="link-pick">
      <div class="link-pick-head">
        <strong class="ellipsis" dir="auto" title={preview.title}>{preview.title}</strong>
        <span class="dim tabular">{tr("已选")} {picked.size}/{files.length} · {formatBytes(bytes.toString())}</span>
      </div>
      {files.length > 1 ? (
        <div class="chip-row" role="group" aria-label={tr("按类型选择")}>
          <button type="button" class={`chip${picked.size === files.length ? " on" : ""}`} onClick={() => setPicked(picked.size === files.length ? new Set() : new Set(files.map((f) => f.index)))}>{tr("全部")}</button>
          {groups.map(({ g, idx }) => {
            const n = idx.filter((i) => picked.has(i)).length;
            const all = n === idx.length;
            return (
              <button type="button" class={`chip${all ? " on" : ""}`} aria-pressed={all ? "true" : n ? "mixed" : "false"} onClick={() => toggleGroup(idx)}>
                {GROUP_LABEL[g]} {n && !all ? `${n}/${idx.length}` : idx.length}
              </button>
            );
          })}
        </div>
      ) : null}
      <ul class="link-files" aria-label={tr("资源中的文件")}>
        {files.map((f) => {
          const cut = f.path.lastIndexOf("/");
          const dir = cut > 0 ? f.path.slice(0, cut) : "";
          const name = f.path.slice(cut + 1);
          const on = picked.has(f.index);
          return (
            <li class={on ? "on" : ""}>
              <label>
                <input type="checkbox" checked={on} onChange={() => { const n = new Set(picked); if (on) n.delete(f.index); else n.add(f.index); setPicked(n); }} />
                <span class="lf-name">
                  <span class="ellipsis" dir="auto" title={f.path}>{name}</span>
                  {dir ? <span class="dim ellipsis" dir="auto">{dir}</span> : null}
                </span>
                <span class="dim tabular">{formatBytes(f.size_bytes)}</span>
              </label>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function CloudTaskBody({ initialParent, initialPath, initialText = "", close }: { initialParent: string; initialPath: string; initialText?: string; close: () => void }) {
  const [sourceMode, setSourceMode] = useState<"link" | "torrent">("link");
  const [torrent, setTorrent] = useState<TorrentPreview | null>(null);
  const [torrentError, setTorrentError] = useState<ClientError>();
  const torrentKey = useRef(newIdempotencyKey());
  const torrentRef = useRef<TorrentPreview | null>(null);
  const alive = useRef(true);
  const torrentSupported = app.get().runtime === "tauri" && app.get().app?.feature_flags.torrent_import === true;
  useEffect(() => () => {
    alive.current = false;
    if (torrentRef.current) void api.cloudTorrentRelease(torrentRef.current.import_id).catch(() => {});
  }, []);
  const [raw, setRaw] = useState(initialText);
  const [target, setTarget] = useState<{ id: string; path: string }>({ id: initialParent, path: initialPath });
  const [lines, setLines] = useState<LineState[]>([]);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<LinkPreview | null>(null);
  const [previewErr, setPreviewErr] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const g = fileGate("cloud_task", target.id);
  const lp = gate("link_preview");
  const chooseTorrent = async () => {
    setBusy(true); setTorrentError(undefined);
    try {
      const next = await api.cloudTorrentPick();
      if (!alive.current) { if(next) await api.cloudTorrentRelease(next.import_id); return; }
      if (next) {
        if (torrentRef.current) await api.cloudTorrentRelease(torrentRef.current.import_id);
        torrentRef.current = next; setTorrent(next); torrentKey.current = newIdempotencyKey();
      }
    } catch (error) { if(alive.current) setTorrentError(errorOf(error)); }
    finally { if(alive.current) setBusy(false); }
  };
  const submitTorrent = async () => {
    if(!torrent || !g.ok || busy) return;
    setBusy(true); setTorrentError(undefined);
    try {
      await api.cloudTorrentCreate(torrent.import_id, target.id, torrentKey.current);
      noteCreated();toast(tx("已提交 {0} 个云下载任务。提交成功不代表下载完成", 1),"success");close();
    } catch(error) { setTorrentError(errorOf(error)); }
    finally { if(alive.current) setBusy(false); }
  };

  const parsed = raw.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  const kinds = parsed.map(classifySource);
  const invalid = kinds.filter((k) => k === "invalid").length;
  const single = parsed.length === 1 && !invalid ? parsed[0] : null;

  // One link: ask the server what it contains so the user can pick files.
  useEffect(() => {
    setPreview(null);
    setPreviewErr(null);
    if (!single || !lp.ok || sourceMode !== "link") return;
    let alive = true;
    const t = setTimeout(() => {
      setPreviewing(true);
      api.cloudTaskPreview(single).then(
        (p) => { if (!alive) return; setPreview(p); setPicked(new Set(p.files.filter((f) => !/sample/i.test(f.path)).map((f) => f.index))); },
        (e) => { if (alive) setPreviewErr(errorOf(e).message); },
      ).finally(() => alive && setPreviewing(false));
    }, 350);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [single, sourceMode]);

  const submit = async () => {
    // Keep each line's idempotency key across retries: re-submitting after a
    // network error can never create a duplicate cloud task.
    const prior = new Map(lines.map((l) => [l.text, l]));
    const next: LineState[] = parsed.map((text) => prior.get(text) ?? { text, key: newIdempotencyKey(), status: "pending" });
    setLines(next);
    setBusy(true);
    for (const l of next) {
      if (l.status === "submitted") continue;
      l.status = "submitting";
      setLines([...next]);
      try {
        const pick = preview && preview.selection_supported !== false && l.text === single ? { preview_id: preview.preview_id, file_indices: [...picked].sort((a, b) => a - b) } : undefined;
        const ref = await api.cloudTaskCreate(l.text, target.id, l.key, pick);
        l.status = "submitted";
        l.task_id = ref.task_id;
        l.error = undefined;
      } catch (e) {
        l.status = "failed";
        l.error = errorOf(e);
      }
      setLines([...next]);
    }
    setBusy(false);
    noteCreated();
    const ok = next.filter((l) => l.status === "submitted").length;
    if (ok === next.length) {
      toast(tx("已提交 {0} 个云下载任务。提交成功不代表下载完成", ok), "success", { action: { label: tr("查看"), run: () => navigate({ page: "cloud" }) } });
      close();
    }
  };

  const done = lines.filter((l) => l.status === "submitted").length;

  return (
    <form onSubmit={(e) => { e.preventDefault(); if(sourceMode === "torrent") { void submitTorrent(); return; } if (g.ok && parsed.length && !invalid && !busy) void submit(); }}>
      {!g.ok ? <div class="inline-note warn"><Icon name="alert" size={14} />{g.reason}</div> : null}
      <div class="chip-row" role="tablist" aria-label={tr("云下载来源")}>
        <button type="button" role="tab" class={`chip${sourceMode === "link" ? " on" : ""}`} aria-selected={sourceMode === "link"} disabled={busy} onClick={() => setSourceMode("link")}>{tr("链接")}</button>
        <button type="button" role="tab" class={`chip${sourceMode === "torrent" ? " on" : ""}`} aria-selected={sourceMode === "torrent"} disabled={busy || !torrentSupported} title={!torrentSupported ? tr("请在桌面或手机应用中选择种子文件") : undefined} onClick={() => setSourceMode("torrent")}>{tr("种子文件")}</button>
      </div>
      {sourceMode === "torrent" ? <>
        <p class="dim">{tr("选择 .torrent 文件，预览后提交云下载。最大 10 MiB。")}</p>
        <Btn label={torrent ? tr("重新选择种子文件") : tr("选择种子文件")} icon="folder" busy={busy} disabled={!g.ok} onClick={() => void chooseTorrent()}/>
        {torrent ? <div class="link-pick">
          <div class="link-pick-head"><strong dir="auto">{torrent.title}</strong><span>{torrent.file_count} {tr("个文件")} · {formatBytes(torrent.total_bytes)}</span></div>
          <p class="dim">{tr("将提交整个种子中的文件。")}</p>
          {torrent.private ? <p class="inline-note warn">{tr("这是私有种子，服务端可能需要对应站点权限。")}</p> : null}
          <ul class="link-files">{torrent.files.map(file => <li key={file.path} style={{display:"flex",alignItems:"center",justifyContent:"space-between",gap:"12px",padding:"8px 12px"}}><span dir="auto" class="ellipsis">{file.path}</span><span class="dim" style={{flexShrink:0}}>{formatBytes(file.size_bytes)}</span></li>)}</ul>
          {torrent.file_count > torrent.files.length ? <p class="dim">{tr("仅展示前 200 个文件，提交包含全部文件。")}</p> : null}
        </div> : null}
        {torrentError ? <ErrorPanel error={torrentError} compact/> : null}
      </> : <>
      <Field label={tr("链接")} htmlFor="cloud-src" hint={tr("每行一个 magnet: 或 http(s) 链接。")}
        error={invalid ? tx("有 {0} 行不是可识别的 magnet 或 http(s) 链接", invalid) : undefined}>
        <textarea id="cloud-src" class="input mono" rows={4} value={raw} spellcheck={false} data-autofocus
          placeholder={"magnet:?xt=urn:btih:…\nhttps://example.com/file.iso"}
          onInput={(e) => setRaw((e.target as HTMLTextAreaElement).value)} />
      </Field>
      {single && lp.ok ? (
        previewing ? <Spinner label={tr("正在读取链接内容")} />
          : previewErr ? <p class="dim"><Icon name="info" size={14} />{tr("无法预览内容（")}{previewErr}{tr("），可以直接提交整个链接。")}</p>
            : preview ? preview.selection_supported === false
              ? <div class="inline-note"><strong dir="auto">{preview.title}</strong><p>{tr("此资源将完整保存，服务暂不支持选择其中的文件。")}</p>{preview.total_bytes ? <span>{formatBytes(preview.total_bytes)}</span> : null}</div>
              : <LinkPick preview={preview} picked={picked} setPicked={setPicked} /> : null
      ) : null}
      </>}
      <Field label={tr("保存到云盘")}>
        <div class="row-gap">
          <span class="path-pill" dir="auto" title={target.path}><Icon name="folder" />{target.path}</span>
          <Btn size="sm" label={tr("更改…")} onClick={async () => { const f = await pickFolder(tr("选择保存位置"), tr("保存到这里"), [], target.id); if (f) setTarget(f); }} />
        </div>
      </Field>
      {lines.length ? (
        <ul class="submit-list" aria-live="polite">
          {lines.map((l) => (
            <li class={`st-${l.status}`}>
              <Icon name={l.status === "submitted" ? "checkCircle" : l.status === "failed" ? "alertCircle" : "clock"} size={14} />
              <span class="ellipsis mono" title={l.text}>{l.text.startsWith("magnet:") ? l.text.slice(0, 60) : l.text.replace(/\?.*$/, "?…")}</span>
              <span class="muted">{l.status === "submitted" ? tr("已提交") : l.status === "failed" ? l.error?.message : l.status === "submitting" ? tr("提交中") : tr("待提交")}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {lines.find((l) => l.error?.code === "QUOTA_EXCEEDED")?.error ? <ErrorPanel error={lines.find((l) => l.error?.code === "QUOTA_EXCEEDED")!.error!} compact /> : null}
      <div class="side-actions">
        <span class="muted">{sourceMode === "link" && parsed.length ? tx("{0} 个链接{1}", parsed.length, done ? `，已提交 ${done}` : "") : tr("提交后由服务端下载到云盘")}</span>
        <div class="row-gap">
          <Btn tone="ghost" label={done ? tr("关闭") : tr("取消")} onClick={close} />
          <Btn type="submit" tone="primary" icon="cloudDown" busy={busy}
            label={lines.some((l) => l.status === "failed") ? tr("重试失败项") : tr("提交云下载")}
            disabled={!g.ok || (sourceMode === "torrent" ? !torrent : parsed.length === 0 || invalid > 0 || (!!preview && preview.selection_supported !== false && picked.size === 0))}
            reason={!g.ok ? g.reason : sourceMode === "torrent" ? tr("请先选择种子文件") : parsed.length === 0 ? tr("请先粘贴链接") : invalid ? tr("请修正无法识别的链接") : tr("至少选择一个文件")} />
        </div>
      </div>
    </form>
  );
}

/** “添加链接” lives in the side island like a draft workspace. */
export function newCloudTask(parentId: string, parentPath: string, text = ""): Promise<unknown> {
  const g = fileGate("cloud_task", parentId);
  if (!g.ok) { toast(g.reason ?? tr("当前不可用"), "info"); return Promise.resolve(); }
  if (mobileLayout()) return openDialog({title:tr("添加链接"),render:close=><CloudTaskBody initialParent={parentId} initialPath={parentPath} initialText={text} close={close}/>});
  void openSide({ key: "add-link", title: tr("添加链接"), render: (close) => <CloudTaskBody initialParent={parentId} initialPath={parentPath} initialText={text} close={close} /> });
  return Promise.resolve();
}

/* ------------------------------------------------------------------ */
/* Shortcuts help                                                      */
/* ------------------------------------------------------------------ */

export const SHORTCUTS = (): [string, string][] => [
  ["Ctrl + T", tr("新建标签页")],
  ["Ctrl + W", tr("关闭标签页")],
  ["Ctrl + Tab / Ctrl + Shift + Tab", tr("切换标签页")],
  ["Ctrl + F", tr("搜索云盘文件")],
  ["Ctrl + K", tr("命令面板：跳转页面、文件夹或执行命令")],
  ["Ctrl + 1 … 8", tr("切换侧栏页面")],
  ["Ctrl + ,", tr("打开设置")],
  ["F5", tr("刷新当前页面")],
  [tr("F1 或 ?"), tr("显示快捷键")],
  ["↑ ↓ / Home / End / PgUp / PgDn", tr("在列表中移动")],
  [tr("Shift + 方向键"), tr("扩展选择")],
  [tr("Ctrl + 方向键，Ctrl + 空格"), tr("移动焦点 / 切换选择")],
  ["Ctrl + A", tr("全选")],
  [tr("Enter / 双击"), tr("打开文件夹，或查看文件属性")],
  ["Alt + Enter", tr("属性")],
  ["Backspace / Alt + ↑", tr("返回上一级")],
  ["F2", tr("重命名（选中多项时批量重命名）")],
  ["Delete", tr("移至回收站（需确认）")],
  ["Ctrl + X / Ctrl + C / Ctrl + V", tr("剪切 / 复制 / 粘贴项目")],
  ["Ctrl + Shift + C", tr("复制云盘路径")],
  ["Ctrl + D", tr("添加或取消星标")],
  ["Ctrl + Z", tr("撤销上一次移动、重命名、移入回收站等操作")],
  [tr("拖动项目"), tr("移到文件夹（按住 Ctrl 复制）")],
  [tr("在空白处拖动"), tr("框选")],
  [tr("Ctrl + 滚轮"), tr("在列表与卡片大小之间切换")],
  [tr("Shift + F10 / 菜单键"), tr("打开右键菜单")],
  ["Ctrl + Shift + N", tr("新建文件夹")],
  ["Esc", tr("清除选择 / 关闭对话框与菜单")],
];

export function showShortcuts(): Promise<unknown> {
  return openDialog({
    title: tr("键盘快捷键"),
    width: 520,
    render: (close) => (
      <div>
        <table class="kbd-table">
          <tbody>
            {SHORTCUTS().map(([k, v]) => (
              <tr><th scope="row"><kbd class="kbd">{k}</kbd></th><td>{v}</td></tr>
            ))}
          </tbody>
        </table>
        <div class="dialog-actions"><Btn tone="primary" label={tr("知道了")} onClick={() => close()} data-autofocus="" /></div>
      </div>
    ),
  });
}
