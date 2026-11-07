import type { RemoteEntry } from "../lib/contract";
import type { MutateOp, MutateTarget, OperationResult } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { ERROR_COPY } from "../lib/errors";
import { app, gate } from "../state/app";
import { navigate } from "../state/route";
import { noteCreated } from "../state/transfers";
import { toast, type MenuItem } from "../state/ui";
import { isPinned, togglePin } from "../state/prefs";
import { confirmDialog, pickFolder, promptName, showProperties } from "../components/dialogs";
import { copyWithToast } from "../components/ui";

const OP_DONE: Record<MutateOp, string> = {
  mkdir: "已新建文件夹",
  rename: "已重命名",
  move: "已移动",
  copy: "已复制",
  trash: "已移至回收站",
  restore: "已还原",
  delete_forever: "已永久删除",
};

/**
 * Run files_mutate with one idempotency key per user intent. A retry
 * triggered from the error toast reuses that key, so a request that did
 * reach the backend is never applied twice.
 */
export async function mutate(op: MutateOp, ids: string[], target: MutateTarget | undefined, onDone: () => void, key = newIdempotencyKey()): Promise<OperationResult | null> {
  try {
    const res = await api.filesMutate(op, ids, target, key);
    const ok = op === "mkdir" ? 1 : res.succeeded.length;
    if (res.failed.length === 0) {
      toast(`${OP_DONE[op]}${ok > 1 ? ` ${ok} 项` : ""}${res.replayed ? "（重复请求已忽略）" : ""}`, "success");
    } else {
      const first = res.failed[0].error;
      toast(`${ok ? `${OP_DONE[op]} ${ok} 项，` : ""}${res.failed.length} 项失败：${first.message}`, "error", { error: res.failed.length === ids.length ? first : undefined });
    }
    onDone();
    return res;
  } catch (e) {
    const err = errorOf(e);
    toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", {
      error: err,
      action: err.retryable ? { label: "重试", run: () => void mutate(op, ids, target, onDone, key) } : undefined,
    });
    return null;
  }
}

export function pathOf(crumbs: string, name: string): string {
  return `${crumbs.replace(/\/$/, "")}/${name}`;
}

export async function downloadEntries(entries: RemoteEntry[]): Promise<void> {
  const g = gate("download");
  if (!g.ok) {
    toast(g.reason ?? "当前不能下载", "info");
    return;
  }
  const files = entries.filter((e) => e.kind === "file");
  const skipped = entries.length - files.length;
  const dir = app.get().settings?.download_dir;
  const rustMock = app.get().runtime === "tauri" && app.get().app?.mode === "mock";
  if (!dir && !rustMock) {
    toast("未设置下载目录，请在设置中选择", "error");
    return;
  }
  let ok = 0;
  const failures: string[] = [];
  for (const f of files) {
    try {
      // The Rust mock writes no files. A stable UUID filename avoids interpreting remote names as paths.
      await api.transferStart(f.id, rustMock ? `C:/NekoPak-Mock/${newIdempotencyKey()}.bin` : dir!);
      ok++;
    } catch (e) {
      const err = errorOf(e);
      failures.push(`${f.name}：${err.message}`);
      if (err.code === "QUOTA_EXCEEDED" || err.code === "AUTH_EXPIRED") {
        toast(err.message, "error", { error: err });
        break;
      }
    }
  }
  noteCreated();
  if (ok) toast(`已加入传输队列 ${ok} 项${skipped ? `，跳过 ${skipped} 个文件夹（M0 暂不支持下载文件夹）` : ""}`, "success", { action: { label: "查看传输", run: () => navigate({ page: "transfers", tab: "active" }) } });
  else if (skipped && !failures.length) toast("M0 暂不支持下载整个文件夹", "info");
  if (failures.length) toast(`${failures.length} 项未能加入：${failures[0]}`, "error");
}

export interface ActionContext {
  /** Cloud path of the folder these entries live in (for copy-path). */
  pathFor: (e: RemoteEntry) => string;
  siblings: () => string[];
  reload: () => void;
  open: (e: RemoteEntry) => void;
}

export async function renameEntry(e: RemoteEntry, ctx: ActionContext): Promise<void> {
  const g = gate("rename");
  if (!g.ok) return void toast(g.reason ?? "不能重命名", "info");
  const name = await promptName("重命名", { initial: e.name, siblings: ctx.siblings(), current: e.name, confirmLabel: "重命名" });
  if (name !== undefined) await mutate("rename", [e.id], { name }, ctx.reload);
}

export async function trashEntries(entries: RemoteEntry[], ctx: ActionContext): Promise<void> {
  if (!entries.length) return;
  const g = gate("trash");
  if (!g.ok) return void toast(g.reason ?? "不能移至回收站", "info");
  const ok = await confirmDialog({
    title: entries.length === 1 ? "移至回收站？" : `将 ${entries.length} 项移至回收站？`,
    body: entries.length === 1
      ? <p>“<span dir="auto">{entries[0].name}</span>”将移至云盘回收站，可在回收站中还原。</p>
      : <p>所选 {entries.length} 项将移至云盘回收站，可在回收站中还原。</p>,
    confirmLabel: "移至回收站",
    danger: true,
  });
  if (ok) await mutate("trash", entries.map((e) => e.id), undefined, ctx.reload);
}

export async function moveOrCopy(op: "move" | "copy", entries: RemoteEntry[], ctx: ActionContext, from?: string): Promise<void> {
  if (!entries.length) return;
  const g = gate(op);
  if (!g.ok) return void toast(g.reason ?? "当前不可用", "info");
  const dest = await pickFolder(op === "move" ? `移动 ${entries.length} 项到…` : `复制 ${entries.length} 项到…`, op === "move" ? "移动到这里" : "复制到这里", entries.filter((e) => e.kind === "folder").map((e) => e.id), from);
  if (dest) await mutate(op, entries.map((e) => e.id), { parent_id: dest.id }, ctx.reload);
}

export async function copyPaths(entries: RemoteEntry[], ctx: ActionContext): Promise<void> {
  if (!entries.length) return;
  await copyWithToast(entries.map(ctx.pathFor).join("\n"), entries.length > 1 ? ` ${entries.length} 个路径` : "路径");
}

/**
 * One action table drives the context menu: a quick icon row (≤5), the
 * grouped list, then “属性”, then the destructive action last.
 */
export function entryMenu(entries: RemoteEntry[], ctx: ActionContext): MenuItem[] {
  const one = entries.length === 1 ? entries[0] : null;
  const files = entries.filter((e) => e.kind === "file");
  const d = gate("download");
  const r = gate("rename");
  const m = gate("move");
  const c = gate("copy");
  const t = gate("trash");
  const folder = one?.kind === "folder" ? one : null;
  const pinned = folder ? isPinned(folder.id) : false;
  return [
    { id: "q-download", quick: true, label: files.length > 1 ? `下载 ${files.length} 个文件` : "下载", icon: "download", disabled: !d.ok || files.length === 0, reason: !d.ok ? d.reason : "M0 暂不支持下载文件夹", run: () => void downloadEntries(entries) },
    { id: "q-copy-path", quick: true, label: "复制路径", icon: "link", shortcut: "Ctrl+C", run: () => void copyPaths(entries, ctx) },
    { id: "q-rename", quick: true, label: "重命名", icon: "pencil", shortcut: "F2", disabled: !one || !r.ok, reason: !r.ok ? r.reason : "一次只能重命名一个项目", run: () => one && void renameEntry(one, ctx) },
    { id: "q-move", quick: true, label: "移动到…", icon: "folderMove", disabled: !m.ok, reason: m.reason, run: () => void moveOrCopy("move", entries, ctx, entries[0]?.parent_id) },
    { id: "q-copy", quick: true, label: "复制到…", icon: "copy", disabled: !c.ok, reason: c.reason, run: () => void moveOrCopy("copy", entries, ctx, entries[0]?.parent_id) },
    ...(folder
      ? [{ id: "open", label: "打开", icon: "folder", shortcut: "Enter", run: () => ctx.open(folder) }]
      : []),
    ...(folder
      ? [{ id: "pin", label: pinned ? "取消固定" : "固定到快速访问", icon: pinned ? "unpin" : "pin", run: () => { const on = togglePin({ id: folder.id, name: folder.name }); toast(on ? `已固定“${folder.name}”到快速访问` : `已取消固定“${folder.name}”`, "success"); } }]
      : []),
    { id: "play", label: "用外部播放器打开", icon: "playCircle", disabled: true, reason: "外部播放器在 P1 提供；当前后端未提供播放链接" },
    { id: "segment", label: "下载指定视频片段", icon: "download", disabled: true, reason: "媒体实验核心尚未接入远端读取和持久任务；功能仍为必需验收项" },
    { id: "props", label: "属性", icon: "info", shortcut: "Alt+Enter", separatorBefore: true, disabled: !one, reason: "请选择一个项目", run: () => one && void showProperties(one.id, ctx.pathFor(one), (e) => void downloadEntries([e])) },
    { id: "trash", label: "移至回收站", icon: "trash", shortcut: "Delete", danger: true, separatorBefore: true, disabled: !t.ok, reason: t.reason, run: () => void trashEntries(entries, ctx) },
  ];
}
