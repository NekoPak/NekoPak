import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { formatBytes, formatBytesExact, formatDateTime } from "../lib/format";
import { fileKind, KIND_LABEL, validateName, windowsIssues } from "../lib/names";
import { gate } from "../state/app";
import { navigate } from "../state/route";
import { noteCreated } from "../state/transfers";
import { openDialog, toast } from "../state/ui";
import { openSide } from "../state/prefs";
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
        <Btn tone="ghost" label="取消" onClick={() => close(false)} />
        <Btn
          type="submit"
          tone={o.danger ? "danger" : "primary"}
          label={o.confirmLabel}
          disabled={!ack}
          reason="请先勾选确认项"
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
      <Field label="名称" htmlFor="name-input" error={touched || name !== initial ? check.error : undefined}>
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
        <Btn tone="ghost" label="取消" onClick={() => close()} />
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
  const [stack, setStack] = useState<{ id: string; name: string }[]>([{ id: ROOT_ID, name: "我的云盘" }]);
  const [items, setItems] = useState<RemoteEntry[] | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const cur = stack[stack.length - 1];

  useEffect(() => {
    if (start === ROOT_ID) return;
    api.filesPath(start).then(
      (chain) => setStack([{ id: ROOT_ID, name: "我的云盘" }, ...chain.filter((e) => e.kind === "folder").map((e) => ({ id: e.id, name: e.name }))]),
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
      <nav class="crumbs small" aria-label="目标位置">
        {stack.map((s, i) => (
          <>
            {i > 0 ? <Icon name="chevronRight" size={12} /> : null}
            <button type="button" class="crumb" aria-current={i === stack.length - 1 ? "location" : undefined} onClick={() => setStack(stack.slice(0, i + 1))}>
              {s.name}
            </button>
          </>
        ))}
      </nav>
      <div class="picker-list" role="listbox" aria-label="文件夹">
        {error ? <ErrorPanel error={error} compact onRetry={load} />
          : items === null ? <Spinner />
            : items.length === 0 ? <p class="muted pad">没有子文件夹</p>
              : items.map((f) => {
                const off = exclude.has(f.id);
                return (
                  <button type="button" role="option" aria-selected="false" class="picker-item" aria-disabled={off ? "true" : undefined}
                    title={off ? "不能选择被移动的文件夹本身" : undefined}
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
        <span class="muted ellipsis" dir="auto" title={path}>目标：{path}</span>
        <div class="row-gap">
          <Btn tone="ghost" label="取消" onClick={() => close()} />
          <Btn tone="primary" label={confirmLabel} disabled={blocked} reason="不能选择被移动的文件夹本身" onClick={() => close({ id: cur.id, path } satisfies PickedFolder)} />
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
  const stream = gate("stream_url");
  const dl = gate("download");
  return (
    <div class="props">
      <div class="props-name"><EntryName entry={entry} showWarn={false} /></div>
      <dl class="kv">
        <dt>类型</dt><dd>{KIND_LABEL[fileKind(entry.name, entry.kind, entry.mime_type)]}{entry.mime_type ? <span class="muted">（{entry.mime_type}）</span> : null}</dd>
        {entry.kind === "file" ? (<><dt>大小</dt><dd>{formatBytes(entry.size_bytes)} <span class="muted">{formatBytesExact(entry.size_bytes)}</span></dd></>) : null}
        <dt>修改时间</dt><dd>{formatDateTime(entry.modified_at)} <span class="muted">{entry.modified_at ?? ""}</span></dd>
        {path ? (<><dt>路径</dt><dd class="wrap" dir="auto">{path}</dd></>) : null}
        <dt>ID</dt><dd><code>{entry.id}</code></dd>
        {entry.hash ? (<><dt>{entry.hash.algorithm.toUpperCase()}</dt><dd><code class="wrap">{entry.hash.value}</code></dd></>) : (<><dt>哈希</dt><dd class="muted">后端未提供</dd></>)}
        {entry.etag ? (<><dt>ETag</dt><dd><code>{entry.etag}</code></dd></>) : null}
      </dl>
      {issues.length ? (
        <ul class="warn-list">{issues.map((i) => <li><Icon name="alert" size={14} />{i}，下载到 Windows 时将被转义</li>)}</ul>
      ) : null}
      <div class="side-actions">
        <div class="row-gap">
          <Btn size="sm" tone="quiet" icon="copy" label="复制路径" disabled={!path} reason="路径不可用" onClick={() => void copyWithToast(path ?? "", "路径")} />
          <Btn size="sm" tone="quiet" icon="copy" label="复制 ID" onClick={() => void copyWithToast(entry.id, " ID")} />
        </div>
        <div class="row-gap">
          {entry.kind === "file" ? (
            <Btn icon="playCircle" label="用外部播放器打开" disabled reason={stream.ok ? "外部播放器集成在 P1 提供" : stream.reason} />
          ) : null}
          {entry.kind === "file" && onDownload ? (
            <Btn tone="primary" icon="download" label="下载" disabled={!dl.ok} reason={dl.reason} onClick={() => { close(); onDownload(entry); }} />
          ) : null}
        </div>
      </div>
    </div>
  );
}

/** Properties open in the non-modal side island; the list stays interactive. */
export function showProperties(id: string, path?: string, onDownload?: (e: RemoteEntry) => void): Promise<unknown> {
  openSide({ key: `props-${id}`, title: "属性", render: (close) => <Properties id={id} path={path} onDownload={onDownload} close={close} /> });
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

function CloudTaskBody({ initialParent, initialPath, close }: { initialParent: string; initialPath: string; close: () => void }) {
  const [raw, setRaw] = useState("");
  const [target, setTarget] = useState<{ id: string; path: string }>({ id: initialParent, path: initialPath });
  const [lines, setLines] = useState<LineState[]>([]);
  const [busy, setBusy] = useState(false);
  const g = gate("cloud_task");

  const parsed = raw.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  const kinds = parsed.map(classifySource);
  const invalid = kinds.filter((k) => k === "invalid").length;

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
        const ref = await api.cloudTaskCreate(l.text, target.id, l.key);
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
      toast(`已提交 ${ok} 个云下载任务。提交成功不代表下载完成`, "success", { action: { label: "查看", run: () => navigate({ page: "cloud" }) } });
      close();
    }
  };

  const done = lines.filter((l) => l.status === "submitted").length;

  return (
    <form onSubmit={(e) => { e.preventDefault(); if (g.ok && parsed.length && !invalid && !busy) void submit(); }}>
      {!g.ok ? <div class="inline-note warn"><Icon name="alert" size={14} />{g.reason}</div> : null}
      <Field label="链接" htmlFor="cloud-src" hint="每行一个 magnet: 或 http(s) 链接。链接只发送给当前后端，不会出现在日志中。"
        error={invalid ? `有 ${invalid} 行不是可识别的 magnet 或 http(s) 链接` : undefined}>
        <textarea id="cloud-src" class="input mono" rows={4} value={raw} spellcheck={false} data-autofocus
          placeholder={"magnet:?xt=urn:btih:…\nhttps://example.com/file.iso"}
          onInput={(e) => setRaw((e.target as HTMLTextAreaElement).value)} />
      </Field>
      <Field label="保存到云盘">
        <div class="row-gap">
          <span class="path-pill" dir="auto" title={target.path}><Icon name="folder" />{target.path}</span>
          <Btn size="sm" label="更改…" onClick={async () => { const f = await pickFolder("选择保存位置", "保存到这里", [], target.id); if (f) setTarget(f); }} />
        </div>
      </Field>
      {lines.length ? (
        <ul class="submit-list" aria-live="polite">
          {lines.map((l) => (
            <li class={`st-${l.status}`}>
              <Icon name={l.status === "submitted" ? "checkCircle" : l.status === "failed" ? "alertCircle" : "clock"} size={14} />
              <span class="ellipsis mono" title={l.text}>{l.text.startsWith("magnet:") ? l.text.slice(0, 60) : l.text.replace(/\?.*$/, "?…")}</span>
              <span class="muted">{l.status === "submitted" ? "已提交" : l.status === "failed" ? l.error?.message : l.status === "submitting" ? "提交中" : "待提交"}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {lines.find((l) => l.error?.code === "QUOTA_EXCEEDED")?.error ? <ErrorPanel error={lines.find((l) => l.error?.code === "QUOTA_EXCEEDED")!.error!} compact /> : null}
      <div class="side-actions">
        <span class="muted">{parsed.length ? `${parsed.length} 个链接${done ? `，已提交 ${done}` : ""}` : "提交后由服务端下载到云盘"}</span>
        <div class="row-gap">
          <Btn tone="ghost" label={done ? "关闭" : "取消"} onClick={close} />
          <Btn type="submit" tone="primary" icon="cloudDown" busy={busy}
            label={lines.some((l) => l.status === "failed") ? "重试失败项" : "提交云下载"}
            disabled={!g.ok || parsed.length === 0 || invalid > 0}
            reason={!g.ok ? g.reason : parsed.length === 0 ? "请先粘贴链接" : "请修正无法识别的链接"} />
        </div>
      </div>
    </form>
  );
}

/** “添加链接” lives in the side island like a draft workspace. */
export function newCloudTask(parentId: string, parentPath: string): Promise<unknown> {
  openSide({ key: "add-link", title: "添加链接", render: (close) => <CloudTaskBody initialParent={parentId} initialPath={parentPath} close={close} /> });
  return Promise.resolve();
}

/* ------------------------------------------------------------------ */
/* Shortcuts help                                                      */
/* ------------------------------------------------------------------ */

export const SHORTCUTS: [string, string][] = [
  ["Ctrl + F", "搜索云盘文件"],
  ["Ctrl + 1 … 7", "切换侧栏页面"],
  ["Ctrl + ,", "打开设置"],
  ["F5", "刷新当前页面"],
  ["F1 或 ?", "显示快捷键"],
  ["↑ ↓ / Home / End / PgUp / PgDn", "在列表中移动"],
  ["Shift + 方向键", "扩展选择"],
  ["Ctrl + 方向键，Ctrl + 空格", "移动焦点 / 切换选择"],
  ["Ctrl + A", "全选"],
  ["Enter / 双击", "打开文件夹，或查看文件属性"],
  ["Alt + Enter", "属性"],
  ["Backspace / Alt + ↑", "返回上一级"],
  ["F2", "重命名"],
  ["Delete", "移至回收站（需确认）"],
  ["Ctrl + C", "复制所选项目的云盘路径"],
  ["Shift + F10 / 菜单键", "打开右键菜单"],
  ["Ctrl + Shift + N", "新建文件夹"],
  ["Esc", "清除选择 / 关闭对话框与菜单"],
];

export function showShortcuts(): Promise<unknown> {
  return openDialog({
    title: "键盘快捷键",
    width: 520,
    render: (close) => (
      <div>
        <table class="kbd-table">
          <tbody>
            {SHORTCUTS.map(([k, v]) => (
              <tr><th scope="row"><kbd class="kbd">{k}</kbd></th><td>{v}</td></tr>
            ))}
          </tbody>
        </table>
        <div class="dialog-actions"><Btn tone="primary" label="知道了" onClick={() => close()} data-autofocus="" /></div>
      </div>
    ),
  });
}
