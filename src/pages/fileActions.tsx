import { tr, tx } from "../lib/i18n";
import type { RemoteEntry } from "../lib/contract";
import type { MutateOp, MutateTarget, OperationResult } from "../lib/contract-proposals";
import { isStarred, ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { ERROR_COPY } from "../lib/errors";
import { fileKind } from "../lib/names";
import { app, fileGate, gate } from "../state/app";
import { entrySource } from "../lib/entry-source";
import { navigate, openTab } from "../state/route";
import { toast, type MenuItem } from "../state/ui";
import { isPinned, togglePin } from "../state/prefs";
import { clearClip, clip, setClip } from "../state/clipboard";
import { pushUndo, undo } from "../state/undo";
import { confirmDialog, pickFolder, promptName, showProperties } from "../components/dialogs";
import { copyWithToast } from "../components/ui";
import { downloadWithOptions, isPlayable, isVideo } from "./download";
import { chooseQuality, createReferenceArchive, openBatchRename, openClipPanel, openPlayer, openShareDialog, playExternal } from "./lazy";
import { requestRefresh } from "./common";
import { sessionGeneration } from "../state/session";

const OP_DONE: Record<MutateOp, string> = {
  get mkdir() { return tr("已新建文件夹"); },
  get rename() { return tr("已重命名"); },
  get move() { return tr("已移动"); },
  get copy() { return tr("已复制"); },
  get trash() { return tr("已移至回收站"); },
  get restore() { return tr("已还原"); },
  get delete_forever() { return tr("已永久删除"); },
  get star() { return tr("已添加星标"); },
  get unstar() { return tr("已取消星标"); },
};

const OP_NOUN: Partial<Record<MutateOp, string>> = { get rename() { return tr("重命名"); }, get move() { return tr("移动"); }, get copy() { return tr("复制"); }, get trash() { return tr("移至回收站"); }, get star() { return tr("星标"); }, get unstar() { return tr("取消星标"); } };

interface Inverse {
  op: MutateOp;
  ids: string[];
  target?: MutateTarget;
}

/** Run inverse operations for an undo; reports the outcome like any other operation. */
async function runInverse(label: string, steps: Inverse[]): Promise<boolean> {
  const generation=sessionGeneration();
  let failed = 0;
  let first = "";
  for (const s of steps) {
    if (generation!==sessionGeneration()) return false;
    if (!s.ids.length) continue;
    try {
      const res = await api.filesMutate(s.op, s.ids, s.target, newIdempotencyKey());
      failed += res.failed.length;
      if (res.failed[0] && !first) first = res.failed[0].error.message;
    } catch (e) {
      failed += s.ids.length;
      if (!first) first = errorOf(e).message;
    }
  }
  requestRefresh();
  if (generation!==sessionGeneration()) return false;
  if (failed) toast(tx("撤销{0}时 {1} 项失败：{2}", label, failed, first), "error");
  else toast(tx("已撤销{0}", label), "success");
  return failed === 0;
}

function inverseOf(op: MutateOp, res: OperationResult, before: RemoteEntry[]): Inverse[] | null {
  const ok = new Set(res.succeeded);
  const was = before.filter((e) => ok.has(e.id));
  switch (op) {
    case "rename": return was.map((e) => ({ op: "rename", ids: [e.id], target: { name: e.name } }));
    case "move": {
      const byParent = new Map<string, string[]>();
      for (const e of was) {
        const p = e.parent_id ?? ROOT_ID;
        byParent.set(p, [...(byParent.get(p) ?? []), e.id]);
      }
      return [...byParent].map(([parent_id, ids]) => ({ op: "move", ids, target: { parent_id } }));
    }
    case "copy": return res.created?.length ? [{ op: "trash", ids: res.created.map((c) => c.id) }] : null;
    case "trash": return [{ op: "restore", ids: was.map((e) => e.id) }];
    case "star": return [{ op: "unstar", ids: was.map((e) => e.id) }];
    case "unstar": return [{ op: "star", ids: was.map((e) => e.id) }];
    default: return null;
  }
}

/**
 * Run files_mutate with one idempotency key per user intent. A retry
 * triggered from the error toast reuses that key, so a request that did
 * reach the backend is never applied twice. Reversible operations get an
 * undo entry (Ctrl+Z, or “撤销” on the snackbar) when `before` is given.
 */
export async function mutate(op: MutateOp, ids: string[], target: MutateTarget | undefined, onDone: () => void, key = newIdempotencyKey(), before?: RemoteEntry[]): Promise<OperationResult | null> {
  const generation=sessionGeneration();
  try {
    const res = await api.filesMutate(op, ids, target, key);
    if (generation!==sessionGeneration()) return null;
    const ok = op === "mkdir" ? 1 : res.succeeded.length;
    const inv = before && ok && !res.replayed ? inverseOf(op, res, before) : null;
    const entry = inv ? pushUndo(`${OP_NOUN[op] ?? ""}${ok > 1 ? tx(" {0} 项", ok) : ""}`, () => runInverse(`${OP_NOUN[op] ?? ""}${ok > 1 ? tx(" {0} 项", ok) : ""}`, inv)) : null;
    if (res.failed.length === 0) {
      toast(`${OP_DONE[op]}${ok > 1 ? tx(" {0} 项", ok) : ""}${res.replayed ? tr("（重复请求已忽略）") : ""}`, "success",
        entry ? { action: { label: tr("撤销"), run: () => void undo(entry.id) } } : {});
    } else {
      const first = res.failed[0].error;
      toast(tx("{0}{1} 项失败：{2}", ok ? `${OP_DONE[op]} ${ok} 项，` : "", res.failed.length, first.message), "error", { error: res.failed.length === ids.length ? first : undefined });
    }
    onDone();
    return res;
  } catch (e) {
    if (generation!==sessionGeneration()) return null;
    const err = errorOf(e);
    toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", {
      error: err,
      action: err.retryable ? { label: tr("重试"), run: () => void mutate(op, ids, target, onDone, key, before) } : undefined,
    });
    return null;
  }
}

export function pathOf(crumbs: string, name: string): string {
  return `${crumbs.replace(/\/$/, "")}/${name}`;
}

/** Ordinary “下载”: files and (when supported) folders, using the default tier. */
export async function downloadEntries(entries: RemoteEntry[]): Promise<void> {
  const g = fileGate("download", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不能下载"), "info");
  await downloadWithOptions(entries);
}

export interface ActionContext {
  /** Cloud path of the folder these entries live in (for copy-path). */
  pathFor: (e: RemoteEntry) => string;
  siblings: () => string[];
  reload: () => void;
  open: (e: RemoteEntry) => void;
  /** Folder the entries are listed in, when they share one (enables paste). */
  folder?: string;
}

export async function renameEntry(e: RemoteEntry, ctx: ActionContext): Promise<void> {
  const g = fileGate("rename", e.id);
  if (!g.ok) return void toast(g.reason ?? tr("不能重命名"), "info");
  const name = await promptName(tr("重命名"), { initial: e.name, siblings: ctx.siblings(), current: e.name, confirmLabel: tr("重命名") });
  if (name !== undefined) await mutate("rename", [e.id], { name }, ctx.reload, undefined, [e]);
}

export async function trashEntries(entries: RemoteEntry[], ctx: ActionContext): Promise<void> {
  if (!entries.length) return;
  const g = fileGate("trash", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("不能移至回收站"), "info");
  const ok = await confirmDialog({
    title: entries.length === 1 ? tr("移至回收站？") : tx("将 {0} 项移至回收站？", entries.length),
    body: entries.length === 1
      ? <p>“<span dir="auto">{entries[0].name}</span>{tr("”将移至云盘回收站，可在回收站中还原，也可以立即撤销（Ctrl+Z）。")}</p>
      : <p>{tr("所选")}{entries.length} {tr("项将移至云盘回收站，可在回收站中还原，也可以立即撤销（Ctrl+Z）。")}</p>,
    confirmLabel: tr("移至回收站"),
    danger: true,
  });
  if (ok) await mutate("trash", entries.map((e) => e.id), undefined, ctx.reload, undefined, entries);
}

/** Move or copy to a known folder (picker, paste, drag and drop). */
export async function sendTo(op: "move" | "copy", entries: RemoteEntry[], dest: string, reload: () => void): Promise<OperationResult | null> {
  const g = fileGate(op, [...entries.map(e => e.id), dest]);
  if (!g.ok) {
    toast(g.reason ?? tr("当前不可用"), "info");
    return null;
  }
  const list = op === "move" ? entries.filter((e) => (e.parent_id ?? ROOT_ID) !== dest) : entries;
  if (!list.length) {
    toast(tr("已在目标文件夹中"), "info");
    return null;
  }
  if (list.some((e) => e.id === dest)) {
    toast(tr("不能移动或复制到自身"), "info");
    return null;
  }
  return mutate(op, list.map((e) => e.id), { parent_id: dest }, reload, undefined, list);
}

export async function moveOrCopy(op: "move" | "copy", entries: RemoteEntry[], ctx: ActionContext, from?: string): Promise<void> {
  if (!entries.length) return;
  const g = fileGate(op, entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不可用"), "info");
  const dest = await pickFolder(op === "move" ? tx("移动 {0} 项到…", entries.length) : tx("复制 {0} 项到…", entries.length), op === "move" ? tr("移动到这里") : tr("复制到这里"), entries.filter((e) => e.kind === "folder").map((e) => e.id), from);
  if (dest) await sendTo(op, entries, dest.id, ctx.reload);
}

export async function copyPaths(entries: RemoteEntry[], ctx: ActionContext): Promise<void> {
  if (!entries.length) return;
  await copyWithToast(entries.map(ctx.pathFor).join("\n"), entries.length > 1 ? tx(" {0} 个路径", entries.length) : tr("路径"));
}

export function cutOrCopy(mode: "cut" | "copy", entries: RemoteEntry[]): void {
  if (!entries.length) return;
  const g = fileGate(mode === "cut" ? "move" : "copy", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不可用"), "info");
  setClip(mode, entries);
  toast(tx("已{0} {1} 项，到目标文件夹按 Ctrl+V 粘贴", mode === "cut" ? tr("剪切") : tr("复制"), entries.length), "info");
}

export async function pasteInto(folder: string, reload: () => void): Promise<void> {
  const c = clip.get();
  if (!c.mode || !c.entries.length) return void toast(tr("剪贴板中没有云盘项目"), "info");
  const res = await sendTo(c.mode === "cut" ? "move" : "copy", c.entries, folder, reload);
  if (res && c.mode === "cut" && res.failed.length === 0) clearClip();
}

export async function setStar(entries: RemoteEntry[], on: boolean, reload: () => void): Promise<void> {
  const g = fileGate("star", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不能使用星标"), "info");
  const list = entries.filter((e) => isStarred(e) !== on);
  if (!list.length) return;
  await mutate(on ? "star" : "unstar", list.map((e) => e.id), undefined, reload, undefined, list);
}

const isArchive = (e: RemoteEntry) => e.kind === "file" && fileKind(e.name, e.kind, e.mime_type) === "archive" && /\.(zip|rar|7z)$/i.test(e.name);

export function openArchive(e: RemoteEntry): void {
  const g = fileGate("archive", e.id);
  if (!g.ok) return void toast(g.reason ?? tr("当前不能浏览压缩包"), "info");
  navigate({ page: "archive", file: e.id, path: "" });
}

/**
 * One action table drives the context menu: a quick icon row (≤5), then
 * grouped commands, then “属性”, then the destructive action last.
 */
export function entryMenu(entries: RemoteEntry[], ctx: ActionContext): MenuItem[] {
  const one = entries.length === 1 ? entries[0] : null;
  if (entries.some(e => entrySource(e.id) !== "drive")) {
    return [
      ...(one?.kind === "folder" ? [{id:"open",label:tr("打开"),icon:"folderOpen",run:()=>ctx.open(one)}] : []),
      ...(one && entrySource(one.id) === "local" && one.kind === "file" && (isPlayable(one) || fileKind(one.name,one.kind,one.mime_type)==="image") ? [{id:"preview",label:tr("播放或查看"),icon:"playCircle",disabled:!app.get().app?.feature_flags.media_session,reason:tr("当前宿主未提供媒体播放"),run:()=>openPlayer(one)}] : []),
      ...(one && isPlayable(one) ? [{id:"play",label:tr("用外部播放器打开"),icon:"playCircle",disabled:!fileGate("external_player",one.id).ok,reason:fileGate("external_player",one.id).reason,run:()=>void playExternal(one)}] : []),
      ...(one && isVideo(one) ? [{id:"download-clip",label:tr("下载指定段落…"),icon:"clip",disabled:!fileGate("clip_download",one.id).ok,reason:fileGate("clip_download",one.id).reason,run:()=>openClipPanel(one)}] : []),
      {id:"props",label:tr("属性"),icon:"info",disabled:!one,reason:tr("请选择一个项目"),run:()=>one&&void showProperties(one.id,ctx.pathFor(one))},
    ];
  }
  const files = entries.filter((e) => e.kind === "file");
  const folders = entries.filter((e) => e.kind === "folder");
  const d = gate("download");
  const r = gate("rename");
  const m = gate("move");
  const c = gate("copy");
  const t = gate("trash");
  const st = gate("star");
  const sh = gate("share");
  const fd = gate("folder_download");
  const folder = one?.kind === "folder" ? one : null;
  const video = one && isVideo(one) ? one : null;
  const archive = one && isArchive(one) ? one : null;
  const pinned = folder ? isPinned(folder.id) : false;
  const allStarred = entries.length > 0 && entries.every(isStarred);
  const anyVideo = entries.some(isVideo);
  const dlDisabled = !d.ok || (files.length === 0 && (!folders.length || !fd.ok));
  const dlLabel = entries.length > 1 ? tx("下载 {0} 项", entries.length) : tr("下载");

  const items: MenuItem[] = [
    { id: "q-cut", quick: true, label: tr("剪切"), icon: "cut", shortcut: "Ctrl+X", disabled: !m.ok, reason: m.reason, run: () => cutOrCopy("cut", entries) },
    { id: "q-copy", quick: true, label: tr("复制"), icon: "copy", shortcut: "Ctrl+C", disabled: !c.ok, reason: c.reason, run: () => cutOrCopy("copy", entries) },
    { id: "q-rename", quick: true, label: entries.length > 1 ? tr("批量重命名…") : tr("重命名"), icon: "pencil", shortcut: "F2", disabled: !r.ok, reason: r.reason,
      run: () => (one ? void renameEntry(one, ctx) : openBatchRename(entries, ctx)) },
    { id: "q-star", quick: true, label: allStarred ? tr("取消星标") : tr("添加星标"), icon: "star", disabled: !st.ok, reason: st.reason, run: () => void setStar(entries, !allStarred, ctx.reload) },
    { id: "q-share", quick: true, label: tr("分享…"), icon: "share", disabled: !sh.ok, reason: sh.reason, run: () => openShareDialog(entries) },
  ];

  if (folder) items.push({ id: "open", label: tr("打开"), icon: "folderOpen", shortcut: "Enter", separatorBefore: true, run: () => ctx.open(folder) });
  else if (archive) items.push({ id: "open-archive", label: tr("浏览压缩包内容"), icon: "archive", shortcut: "Enter", separatorBefore: true, disabled: !gate("archive").ok, reason: gate("archive").reason, run: () => openArchive(archive) });
  if (folder) items.push({ id: "open-tab", label: tr("在新标签页打开"), icon: "plus", run: () => { if (!openTab({ page: "files", folder: folder.id }, folder.name)) toast(tr("最多同时打开12个标签页"), "info"); } });
  if (one && isPlayable(one)) {
    items.push({id:"preview",label:tr("播放或查看"),icon:"play",disabled:!app.get().app?.feature_flags.media_session,reason:tr("当前宿主未提供媒体播放"),run:()=>openPlayer(one)});
    const g = gate("external_player");
    items.push({ id: "play", label: tr("用外部播放器打开"), icon: "playCircle", separatorBefore: !folder && !archive, disabled: !g.ok, reason: g.reason, run: () => void playExternal(one) });
  }

  items.push(
    { id: "download", label: dlLabel, icon: "download", separatorBefore: true, disabled: dlDisabled, reason: !d.ok ? d.reason : fd.reason, run: () => void downloadEntries(entries) },
  );
  if (anyVideo || folders.length) {
    const q = gate("quality_download");
    items.push({ id: "download-quality", label: tr("下载指定画质…"), icon: "quality", disabled: !q.ok || dlDisabled, reason: q.ok ? d.reason : q.reason, run: () => chooseQuality(entries) });
  }
  if (video) {
    const cl = gate("clip_download");
    items.push({ id: "download-clip", label: tr("下载指定段落…"), icon: "clip", disabled: !cl.ok, reason: cl.reason, run: () => openClipPanel(video) });
  }

  items.push(
    { id: "move", label: tr("移动到…"), icon: "folderMove", separatorBefore: true, disabled: !m.ok, reason: m.reason, run: () => void moveOrCopy("move", entries, ctx, entries[0]?.parent_id) },
    { id: "copy-to", label: tr("复制到…"), icon: "folderCopy", disabled: !c.ok, reason: c.reason, run: () => void moveOrCopy("copy", entries, ctx, entries[0]?.parent_id) },
    { id: "copy-path", label: entries.length > 1 ? tx("复制 {0} 个路径", entries.length) : tr("复制路径"), icon: "link", shortcut: "Ctrl+Shift+C", run: () => void copyPaths(entries, ctx) },
  );
  if (entries.length > 1) items.push({ id: "batch-rename", label: tr("批量重命名…"), icon: "findReplace", shortcut: "F2", disabled: !r.ok, reason: r.reason, run: () => openBatchRename(entries, ctx) });

  if (folder) {
    const dup = gate("duplicates");
    const sp = gate("space_summary");
    items.push(
      { id: "dups", label: tr("在此文件夹中查找重复…"), icon: "duplicates", separatorBefore: true, disabled: !dup.ok, reason: dup.reason, run: () => navigate({ page: "duplicates", folder: folder.id }) },
      { id: "space", label: tr("查看空间分布"), icon: "space", disabled: !sp.ok, reason: sp.reason, run: () => navigate({ page: "space", folder: folder.id }) },
      { id: "pin", label: pinned ? tr("取消固定") : tr("固定到快速访问"), icon: pinned ? "unpin" : "pin", run: () => { const before = isPinned(folder.id); const on = togglePin({ id: folder.id, name: folder.name }); if (on !== before) toast(on ? tx("已固定“{0}”到快速访问", folder.name) : tx("已取消固定“{0}”", folder.name), "success"); } },
    );
  }
  if (archive) {
    const a = gate("archive");
    items.push({ id: "extract", label: tr("解压到…"), icon: "unarchive", separatorBefore: !folder, disabled: !a.ok, reason: a.reason, run: () => openArchive(archive) });
  }

  items.push(
    { id: "reference", label: tr("创建引用归档清单"), icon: "archive", separatorBefore: true, disabled: !app.get().app?.feature_flags.reference_archive || !gate("stat").ok, reason: tr("当前版本尚未启用引用归档"), run: () => createReferenceArchive(entries) },
    { id: "props", label: tr("属性"), icon: "info", shortcut: "Alt+Enter", separatorBefore: true, disabled: !one, reason: tr("请选择一个项目"), run: () => one && void showProperties(one.id, ctx.pathFor(one), (e) => void downloadEntries([e])) },
    { id: "trash", label: tr("移至回收站"), icon: "trash", shortcut: "Delete", danger: true, separatorBefore: true, disabled: !t.ok, reason: t.reason, run: () => void trashEntries(entries, ctx) },
  );
  return items;
}
