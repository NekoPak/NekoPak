import { tr, tx } from "../lib/i18n";
/**
 * Video tools: “下载指定段落…” (clip download), “下载指定画质…” and
 * “用外部播放器打开”. All media work happens in the core (PROPOSAL UI-001
 * §P14/§P21); the UI only picks a range and a tier. No FFmpeg/mpv here.
 */
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError, RemoteEntry } from "../lib/contract";
import type { ClipAlignment, MediaProbe, PreviewFrame, QualityCap, QualityOption } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { fullRange, estimateBytes, formatClock, nudgeRange, parseClock, setEnd, type Range } from "../lib/clip";
import { ERROR_COPY } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { app, fileGate, updateSettings } from "../state/app";
import { openDialog, toast } from "../state/ui";
import { dismissSide, openSide, setSideGuard } from "../state/prefs";
import { Icon } from "../components/Icon";
import { RangeSlider } from "../components/RangeSlider";
import { Btn, ErrorPanel, Segmented, Spinner } from "../components/ui";
import { confirmDialog } from "../components/dialogs";
import { ClipVideoPreview } from "../components/ClipVideoPreview";
import { useStore } from "../lib/store";

import { CAP_LABEL, downloadDir, downloadWithOptions, isVideo, showQueued } from "./download";

/* ------------------------------------------------------------------ */
/* Clip panel                                                          */
/* ------------------------------------------------------------------ */

function ClipPanel({ entry, close }: { entry: RemoteEntry; close: () => void }) {
  const nativePreview = useStore(app, s => s.runtime === "tauri" && s.app?.feature_flags.media_session === true);
  const [probe, setProbe] = useState<MediaProbe | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [range, setRange] = useState<Range | null>(null);
  const [initial, setInitial] = useState<Range | null>(null);
  const [active, setActive] = useState<"start" | "end">("start");
  const [quality, setQuality] = useState("original");
  const [align, setAlign] = useState<ClipAlignment | null>(null);
  const [alignErr, setAlignErr] = useState<ClientError | null>(null);
  const [frame, setFrame] = useState<PreviewFrame | null>(null);
  const [frameErr, setFrameErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [timeText, setTimeText] = useState<string | null>(null);
  const alignSeq = useRef(0);
  const frameSeq = useRef(0);

  const load = () => {
    setError(null);
    setProbe(null);
    api.mediaProbe(entry.id).then(
      (p) => {
        setProbe(p);
        const r = fullRange(p.duration_ms);
        setRange(r);
        setInitial(r);
      },
      (e) => setError(errorOf(e)),
    );
  };
  useEffect(load, [entry.id]);

  const dirty = !!range && !!initial && (range.start !== initial.start || range.end !== initial.end || quality !== "original");
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;
  useEffect(() => {
    setSideGuard(async () => {
      if (!dirtyRef.current) return true;
      return confirmDialog({ title: tr("放弃下载片段？"), body: <p>{tr("已选择的起点和终点将会丢失。")}</p>, confirmLabel: tr("放弃") });
    });
    return () => setSideGuard(null);
  }, []);

  // Ask the core where the cut will really start (previous keyframe).
  useEffect(() => {
    if (!probe || !range) return;
    const my = ++alignSeq.current;
    const t = setTimeout(() => {
      api.mediaAlign(entry.id, quality, range.start, range.end).then(
        (a) => { if (my === alignSeq.current) { setAlign(a); setAlignErr(null); } },
        (e) => { if (my === alignSeq.current) { setAlign(null); setAlignErr(errorOf(e)); } },
      );
    }, 220);
    return () => clearTimeout(t);
  }, [probe, range?.start, range?.end, quality]);

  // Preview follows the end being edited.
  const editingPos = range ? (active === "start" ? range.start : range.end) : 0;
  useEffect(() => {
    if (nativePreview || !probe || !range || probe.preview !== "available") return;
    const my = ++frameSeq.current;
    const t = setTimeout(() => {
      api.mediaPreviewFrame(entry.id, editingPos, 480).then(
        (f) => { if (my === frameSeq.current) { setFrame(f); setFrameErr(null); } },
        (e) => { if (my === frameSeq.current) setFrameErr(errorOf(e).message); },
      );
    }, 140);
    return () => clearTimeout(t);
  }, [probe, editingPos, nativePreview]);

  if (error) return <ErrorPanel error={error} onRetry={load} compact />;
  if (!probe || !range) return <ClipSkeleton />;

  const tiers = probe.qualities;
  const tier = tiers.find((q) => q.id === quality) ?? tiers[0];
  const g = fileGate("clip_download", entry.id);
  const keyframesOk = probe.keyframes !== "unavailable";
  const est = align?.estimated_bytes ?? estimateBytes(tier.size_bytes, probe.duration_ms, range);
  const shift = align ? range.start - align.start_ms : 0;
  const canDownload = g.ok && keyframesOk && tier.readable && !alignErr;
  const why = !g.ok ? g.reason : !keyframesOk ? tr("无法读取此视频的关键帧，不能生成片段") : !tier.readable ? tier.unreadable_reason : alignErr?.message;

  const apply = (r: Range | null, which?: "start" | "end") => {
    if (!r) return;
    setRange(r);
    if (which) setActive(which);
  };

  const commitTime = () => {
    if (timeText === null) return;
    const v = parseClock(timeText);
    setTimeText(null);
    if (v === null) return void toast(tr("时间格式应为 时:分:秒 或 分:秒"), "info");
    apply(setEnd(probe.duration_ms, range, active, v));
  };

  const start = async () => {
    let dir = downloadDir();
    setBusy(true);
    try {
      if (app.get().runtime === "tauri" && app.get().settings?.ask_download_directory) {
        const selected = await api.downloadDirectoryPick(); if (selected === null) return; dir = selected;
      }
      if (!dir) return void toast(tr("未设置下载目录，请在设置中选择"), "error");
      await api.transferStart(entry.id, dir, {
        ...(tier.id !== "original" ? { quality_id: tier.id } : {}),
        clip: { start_ms: range.start, end_ms: range.end, align: "previous_keyframe" },
      });
      dismissSide();
      showQueued(tx("已加入片段下载：{0} 至 {1}", formatClock(align?.start_ms ?? range.start), formatClock(range.end)));
    } catch (e) {
      const err = errorOf(e);
      toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="clip">
      <p class="clip-file ellipsis" dir="auto" title={entry.name}>{entry.name}</p>

      {nativePreview ? <ClipVideoPreview remoteId={entry.id} qualityId={quality} positionMs={editingPos}
        startMs={range.start} endMs={range.end} edge={active}/> : <div class={`clip-preview${frame?.synthetic ? " synthetic" : ""}`} aria-live="off">
        {probe.preview !== "available" ? (
          <span class="clip-preview-off"><Icon name="visibilityOff" size={20} />{probe.preview_reason ?? tr("暂无预览画面")}</span>
        ) : frame ? (
          <img src={`data:${frame.mime};base64,${frame.data_base64}`} alt={tx("{0} 处的{1}画面", formatClock(frame.position_ms), frame.synthetic ? "示意" : "预览")} />
        ) : frameErr ? <span class="clip-preview-off"><Icon name="alert" size={20} />{frameErr}</span> : <Spinner label={tr("正在读取预览")} />}
        <span class="clip-pos tabular">{active === "start" ? tr("起点") : tr("终点")} {formatClock(editingPos)}</span>
      </div>}

      <div class="clip-edit">
        <Segmented<"start" | "end"> label={tr("正在调整")} value={active} onChange={setActive}
          options={[{ value: "start", label: tr("起点") }, { value: "end", label: tr("终点") }]} />
        <input class="clip-time tabular" aria-label={active === "start" ? tr("起点时间") : tr("终点时间")}
          value={timeText ?? formatClock(editingPos)}
          onFocus={(e) => setTimeText((e.target as HTMLInputElement).value)}
          onInput={(e) => setTimeText((e.target as HTMLInputElement).value)}
          onBlur={commitTime}
          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitTime(); } if (e.key === "Escape") { setTimeText(null); (e.target as HTMLInputElement).blur(); } }} />
        <div class="nudge" role="group" aria-label={tr("按秒微调")}>
          <button type="button" class="nudge-btn" aria-label={tr("后退 1 秒")} disabled={!nudgeRange(probe.duration_ms, range, active, -1000) || (active === "start" ? range.start === 0 : range.end - range.start <= 1000)}
            onClick={() => apply(nudgeRange(probe.duration_ms, range, active, -1000))}>−1</button>
          <button type="button" class="nudge-btn" aria-label={tr("前进 1 秒")} disabled={active === "end" ? range.end >= probe.duration_ms : range.end - range.start <= 1000}
            onClick={() => apply(nudgeRange(probe.duration_ms, range, active, 1000))}>+1</button>
        </div>
      </div>

      <div>
        <RangeSlider total={probe.duration_ms} value={range} active={active} onActive={setActive}
          onChange={(r, w) => apply(r, w)} alignedStart={align?.start_ms} label={tr("片段")} />
        <div class="clip-scale">
          <span class="tabular">{formatClock(range.start)} {tr("至")} {formatClock(range.end)}</span>
          <span class="dim tabular">{tr("全长")}{formatClock(probe.duration_ms)}</span>
        </div>
      </div>

      {tiers.length > 1 ? (
        <div class="clip-quality">
          <span class="clip-label">{tr("画质")}</span>
          <Segmented label={tr("画质")} value={quality} onChange={setQuality}
            options={tiers.map((q) => ({ value: q.id, label: q.label, disabled: !q.readable, reason: q.unreadable_reason }))} />
        </div>
      ) : (
        <p class="dim clip-note">{probe.qualities.length === 1 ? tr("此视频暂无转码画质，只能截取原画。") : null}</p>
      )}

      <div class="clip-sum">
        {probe.warnings?.map(warning => <p class="warn-text">{warning}</p>)}
        {probe.tracks?.length ? <details><summary>{tr("媒体轨道")}</summary><ul>{probe.tracks.map(track=><li>{track.kind} · {track.codec} · {track.language??tr("未知")}</li>)}</ul></details> : null}
        <p class="tabular">
          {tr("时长")}{formatClock(range.end - (align?.start_ms ?? range.start))}
          {est ? <> · {align?.exactness === "exact" ? "" : tr("约 ")}{formatBytes(est)}</> : <> {tr("· 大小未知")}</>}
          {tier.container === "mp4" ? <span class="dim"> {tr("· 保存为 MP4")}</span> : null}
        </p>
        <p class="dim">
          {probe.keyframes === "unavailable" ? tr("无法读取关键帧。")
            : align && shift > 0 ? tx("起点对齐到前一个关键帧 {0}（提前 {1} 秒）", formatClock(align.start_ms, true), (shift / 1000).toFixed(1))
              : tr("起点对齐到前一个关键帧")}
          {probe.keyframes === "estimated" ? tr("，关键帧位置为估算") : ""}
        </p>
        {frame?.synthetic ? <p class="dim clip-mock"><Icon name="flask" size={14} />{tr("演示画面，不是视频内容。")}</p> : null}
      </div>

      <div class="side-actions">
        <Btn tone="primary" icon="download" label={tr("下载片段")} busy={busy} disabled={!canDownload} reason={why} onClick={() => void start()} class="clip-go" />
        <div class="row-gap">
          <Btn tone="ghost" label={tr("取消")} onClick={close} />
        </div>
      </div>
    </div>
  );
}

function ClipSkeleton() {
  return (
    <div class="clip" aria-busy="true" aria-label={tr("正在读取视频信息")}>
      <span class="sk-line" style={{ width: "70%" }} />
      <div class="clip-preview"><Spinner label={tr("正在读取视频信息")} /></div>
      <span class="sk-line" style={{ width: "100%", height: "40px" }} />
      <span class="sk-line" style={{ width: "100%" }} />
      <span class="sk-line short" />
    </div>
  );
}

export function openClipPanel(entry: RemoteEntry): void {
  const g = fileGate("clip_download", entry.id);
  if (!g.ok) return void toast(g.reason ?? tr("当前不能下载片段"), "info");
  void openSide({ key: `clip-${entry.id}`, title: tr("下载片段"), render: (close) => <ClipPanel entry={entry} close={close} /> });
}

/* ------------------------------------------------------------------ */
/* Quality dialog                                                      */
/* ------------------------------------------------------------------ */


function QualityBody({ entries, close }: { entries: RemoteEntry[]; close: (v?: unknown) => void }) {
  const single = entries.length === 1 && isVideo(entries[0]) ? entries[0] : null;
  const [probe, setProbe] = useState<MediaProbe | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [choice, setChoice] = useState<string>("original");
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = () => {
    if (!single) return;
    setError(null);
    api.mediaProbe(single.id).then(setProbe, (e) => setError(errorOf(e)));
  };
  useEffect(load, [single?.id]);

  const options: { id: string; label: string; detail: string; disabled?: boolean; reason?: string }[] = useMemo(() => {
    if (single) {
      return (probe?.qualities ?? []).map((q: QualityOption) => ({
        id: q.id,
        label: q.label,
        detail: [q.height ? `${q.width ?? "?"}×${q.height}` : null, q.size_bytes ? formatBytes(q.size_bytes) : tr("大小未知"), q.container === "mp4" ? "MP4" : tr("原始格式")].filter(Boolean).join(" · "),
        disabled: !q.readable,
        reason: q.unreadable_reason,
      }));
    }
    return (["original", "2160p", "1080p", "720p", "480p"] as QualityCap[]).map((c) => ({
      id: c,
      label: CAP_LABEL[c],
      detail: c === "original" ? tr("下载原始文件") : tx("每个视频选不高于 {0} 的最高画质；没有转码的视频下载原画", CAP_LABEL[c]),
    }));
  }, [probe, single]);

  const go = async () => {
    let dir = downloadDir();
    setBusy(true);
    try {
      if (single && app.get().runtime === "tauri" && app.get().settings?.ask_download_directory) {
        const selected = await api.downloadDirectoryPick(); if (selected === null) return; dir = selected;
      }
      if (!dir) return void toast(tr("未设置下载目录，请在设置中选择"), "error");
      if (remember) {
        const height = probe?.qualities.find(q => q.id === choice)?.height;
        const cap: QualityCap = single && choice !== "original"
          ? height ? height >= 2160 ? "2160p" : height >= 1080 ? "1080p" : height >= 720 ? "720p" : "480p" : choice as QualityCap
          : choice as QualityCap;
        if (!["original","2160p","1080p","720p","480p"].includes(cap)) throw {code:"UNSUPPORTED",message:tr("该画质没有可保存的分辨率档位"),retryable:false};
        await updateSettings({ download: { default_quality: cap } });
      }
      if (single) {
        await api.transferStart(single.id, dir, choice === "original" ? undefined : { quality_id: choice });
        close(true);
        showQueued(tx("已加入下载（{0}）{1}", options.find((o) => o.id === choice)?.label ?? choice, remember ? tr("，以后按此画质直接下载") : ""));
      } else {
        close(true);
        await downloadWithOptions(entries, choice === "original" ? undefined : { quality_cap: choice as QualityCap });
      }
    } catch (e) {
      const err = errorOf(e);
      toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
    } finally {
      setBusy(false);
    }
  };

  if (single && error) return <ErrorPanel error={error} onRetry={load} compact />;
  if (single && !probe) return <Spinner label={tr("正在读取可用画质")} />;

  return (
    <form onSubmit={(e) => { e.preventDefault(); void go(); }}>
      <p class="muted pad-bottom">{single ? <span dir="auto">{single.name}</span> : tx("所选 {0} 项。普通“下载”使用设置中的默认画质，这里只改变这一次。", entries.length)}</p>
      <div class="radio-cards" role="radiogroup" aria-label={tr("画质")}>
        {options.map((o) => (
          <label class={`radio-card${choice === o.id ? " on" : ""}${o.disabled ? " off" : ""}`} title={o.disabled ? o.reason : undefined}>
            <input type="radio" name="quality" value={o.id} checked={choice === o.id} disabled={o.disabled}
              onChange={() => setChoice(o.id)} {...(choice === o.id ? { "data-autofocus": "" } : {})} />
            <span class="rc-main">
              <strong>{o.label}</strong>
              <span class="dim">{o.disabled ? o.reason : o.detail}</span>
            </span>
          </label>
        ))}
      </div>
      <label class="ack">
        <input type="checkbox" checked={remember} onChange={(e) => setRemember((e.target as HTMLInputElement).checked)} />
        <span>{tr("以后按此画质直接下载（可在设置中修改）")}</span>
      </label>
      <div class="dialog-actions">
        <Btn tone="ghost" label={tr("取消")} onClick={() => close()} />
        <Btn type="submit" tone="primary" icon="download" label={tr("下载")} busy={busy} />
      </div>
    </form>
  );
}

export function chooseQuality(entries: RemoteEntry[]): void {
  const g = fileGate("quality_download", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("当前不能按画质下载"), "info");
  void openDialog({ title: tr("下载指定画质"), width: 480, render: (close) => <QualityBody entries={entries} close={close} /> });
}

/* ------------------------------------------------------------------ */
/* External player                                                     */
/* ------------------------------------------------------------------ */

export async function playExternal(entry: RemoteEntry): Promise<void> {
  const g = fileGate("external_player", entry.id);
  if (!g.ok) return void toast(g.reason ?? tr("当前不能用外部播放器打开"), "info");
  try {
    const r = await api.mediaOpenExternal(entry.id);
    if (r.launched) toast(tx("已交给{0}播放", r.player ?? "系统播放器"), "success");
    else toast(r.note ?? tr("播放器未启动"), "info");
  } catch (e) {
    const err = errorOf(e);
    toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
  }
}
