import { tr, tx } from "../lib/i18n";
/**
 * Download entry points used everywhere (menus, buttons, keyboard). The
 * heavier video tools in Media.tsx load on demand.
 */
import type { RemoteEntry } from "../lib/contract";
import type { QualityCap, TransferOptions } from "../lib/contract-proposals";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { ERROR_COPY } from "../lib/errors";
import { fileKind } from "../lib/names";
import { app, fileGate, hostSupports } from "../state/app";
import { navigate } from "../state/route";
import { noteCreated } from "../state/transfers";
import { toast } from "../state/ui";
import { sessionGeneration } from "../state/session";
import { entrySource } from "../lib/entry-source";

export const isVideo = (e: RemoteEntry) => e.kind === "file" && fileKind(e.name, e.kind, e.mime_type) === "video";
export const isPlayable = (e: RemoteEntry) => e.kind === "file" && ["video", "audio"].includes(fileKind(e.name, e.kind, e.mime_type));

export const CAP_LABEL: Record<QualityCap, string> = { get original() { return tr("原画"); }, "2160p": "2160P", "1080p": "1080P", "720p": "720P", "480p": "480P" };

export function downloadDir(): string | undefined {
  return app.get().settings?.download_dir;
}

export function showQueued(label: string) {
  noteCreated();
  toast(label, "success", { action: { label: tr("查看任务"), run: () => navigate({ page: "transfers", tab: "active" }) } });
}

export async function downloadWithOptions(entries: RemoteEntry[], options?: TransferOptions): Promise<void> {
  const generation=sessionGeneration();
  const permission = fileGate(options?.clip ? "clip_download" : options ? "quality_download" : "download", entries.map(e => e.id));
  if (!permission.ok) return void toast(permission.reason ?? tr("暂不支持下载"), "info");
  if (app.get().runtime === "tauri" && app.get().app?.mode === "mock" && !options) {
    try {
      for (const entry of entries.filter(e => e.kind === "file")) {
        // Rust Mock validates but never writes this simulation target.
        await api.transferStart(entry.id, `C:/NekoPak-Mock/${entry.id}.bin`);
      }
      showQueued(tr("已加入模拟队列，不会下载文件"));
    } catch (e) { toast(errorOf(e).message, "error"); }
    return;
  }
  let dir = downloadDir();
  if(app.get().runtime === "tauri" && hostSupports("directory_picker") && app.get().settings?.ask_download_directory) {
    try {const selected=await api.downloadDirectoryPick(); if(selected===null)return;dir=selected;}
    catch(e){toast(errorOf(e).message,"error");return;}
  }
  if (!dir) return void toast(tr("未设置下载目录，请在设置中选择"), "error");
  const hasFolder = entries.some((e) => e.kind === "folder");
  const fd = fileGate("folder_download", entries.map(e => e.id));
  if (hasFolder && !fd.ok) {
    toast(fd.reason ?? tr("暂不支持下载文件夹"), "info");
    entries = entries.filter((e) => e.kind === "file");
    if (!entries.length) return;
  }
  const cap = app.get().settings?.download?.default_quality;
  const shared = entries.some(entry => entrySource(entry.id) === "shared");
  const opts: TransferOptions | undefined = options ?? (!shared && cap && cap !== "original" ? { quality_cap: cap } : undefined);
  try {
    if (app.get().runtime === "tauri" && !opts && !hasFolder) {
      let queued = 0;
      for (const entry of entries.filter(e => e.kind === "file")) { if(generation!==sessionGeneration()) return; await api.transferStart(entry.id, dir); queued++; }
      if(generation!==sessionGeneration()) return;
      if (queued) showQueued(tx("已加入传输队列 {0} 项", queued));
      return;
    }
    if (entries.length === 1 && entries[0].kind === "file") {
      await api.transferStart(entries[0].id, dir, opts);
      return void showQueued(tr("已加入传输队列 1 项"));
    }
    const res = await api.transferStartBatch(entries.map((e) => e.id), dir, newIdempotencyKey(), opts);
    noteCreated();
    const n = res.task_ids.length;
    if (n) showQueued(tx("已加入传输队列 {0} 项{1}", n, res.expanded_files ? `（含文件夹中的 ${res.expanded_files} 个文件）` : ""));
    if (res.skipped.length) toast(tx("{0} 项未加入：{1}{2}", res.skipped.length, res.skipped[0].name ? `${res.skipped[0].name}，` : "", res.skipped[0].reason), n ? "warning" : "error");
  } catch (e) {
    if(generation!==sessionGeneration()) return;
    const err = errorOf(e);
    toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
  }
}
