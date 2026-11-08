import { useEffect, useRef, useState } from "preact/hooks";
import type { RemoteEntry, ClientError } from "../lib/contract";
import type { MediaProbe, MediaSession } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { tr } from "../lib/i18n";
import { app, fileGate } from "../state/app";
import { openDialog, toast } from "../state/ui";
import { onSessionReset, sessionGeneration } from "../state/session";
import { Btn, ErrorPanel, Spinner } from "../components/ui";
import { openClipPanel, playExternal } from "./lazy";
import { safeMediaSessionUrl } from "../lib/gallery-image";
import { entrySource } from "../lib/entry-source";
export { safeMediaSessionUrl } from "../lib/gallery-image";

const positions = new Map<string, number>();
onSessionReset(() => positions.clear());
export function subtitleVtt(source: string): string {
  const text = source.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
  if (/^WEBVTT(?:\s|$)/.test(text)) return text;
  return "WEBVTT\n\n" + text.replace(/(\d{2}:\d{2}:\d{2}),(\d{3})/g, "$1.$2");
}

export function PlayerView({ entry, onEnded }: { entry: RemoteEntry; onEnded?: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const [session, setSession] = useState<MediaSession>();
  const [probe, setProbe] = useState<MediaProbe>();
  const [error, setError] = useState<ClientError>();
  const [quality, setQuality] = useState("original");
  const [version, setVersion] = useState(0);
  const [subtitle, setSubtitle] = useState<{ url: string; name: string }>();
  const [rate, setRate] = useState(1);
  const generation = sessionGeneration();
  const image = entry.mime_type?.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|avif)$/i.test(entry.name);
  useEffect(() => {
    let alive = true, opened: MediaSession | undefined;
    setSession(undefined); setError(undefined);
    void api.mediaSessionOpen(entry.id, quality).then(next => {
      opened = next;
      if (!safeMediaSessionUrl(next.url)) throw new Error("Invalid local media session");
      if (!alive || generation !== sessionGeneration()) { void api.mediaSessionClose(next.session_id); return; }
      setSession(next);
    }).catch(e => { if (alive) setError(errorOf(e)); });
    return () => { alive = false; if (opened) void api.mediaSessionClose(opened.session_id).catch(() => {}); };
  }, [entry.id, quality, version, generation]);
  useEffect(() => {
    setProbe(undefined);
    if (image || !fileGate("media_probe", entry.id).ok) return;
    let alive = true;
    void api.mediaProbe(entry.id).then(p => { if (alive) setProbe(p); }, () => {});
    return () => { alive = false; };
  }, [entry.id, image]);
  useEffect(() => () => { if (subtitle) URL.revokeObjectURL(subtitle.url); }, [subtitle]);
  const pickSubtitle = async (file?: File) => {
    if (!file) return;
    if (file.size > 2 * 1024 * 1024 || !/\.(vtt|srt)$/i.test(file.name)) return toast(tr("请选择不超过2 MiB的SRT或VTT字幕"), "error");
    try { const vtt = subtitleVtt(await file.text()); if (generation !== sessionGeneration()) return; setSubtitle({ url: URL.createObjectURL(new Blob([vtt], { type: "text/vtt" })), name: file.name }); }
    catch (e) { setError(errorOf(e)); }
  };
  return <div class="media-player">
    {error ? <ErrorPanel error={error} onRetry={() => setVersion(x => x + 1)}/> : !session ? <Spinner label={tr("正在读取媒体")}/> : image ?
      <img class="media-image" src={session.url} alt={entry.name} onError={() => setError({ code: "UNSUPPORTED", message: tr("此图片无法在当前系统中显示"), retryable: false })}/> :
      <video ref={video} src={session.url} controls preload="metadata" crossOrigin="anonymous"
        onLoadedMetadata={() => { if (video.current) { video.current.currentTime = positions.get(entry.id) ?? 0; video.current.playbackRate = rate; } }}
        onTimeUpdate={() => { if (video.current && generation === sessionGeneration()) positions.set(entry.id, video.current.currentTime); }}
        onEnded={() => { positions.delete(entry.id); onEnded?.(); }}
        onError={() => setError({ code: "UNSUPPORTED", message: tr("当前系统无法解码此媒体，可尝试其他画质或外部播放器"), retryable: false })}>
        {subtitle ? <track key={subtitle.url} src={subtitle.url} kind="subtitles" label={subtitle.name} default/> : null}
      </video>}
    {!image ? <div class="player-controls">
      {probe?.qualities.length ? <label>{tr("画质")} <select class="select" value={quality} onChange={e => setQuality((e.target as HTMLSelectElement).value)}>{probe.qualities.filter(q => entrySource(entry.id) !== "shared" || q.id === "original").map(q => <option value={q.id} disabled={!q.readable}>{q.label}</option>)}</select></label> : null}
      <label>{tr("播放速度")} <select class="select" value={rate} onChange={e => { const n = Number((e.target as HTMLSelectElement).value); setRate(n); if (video.current) video.current.playbackRate = n; }}>{[0.5, 0.75, 1, 1.25, 1.5, 2].map(n => <option value={n}>{n}×</option>)}</select></label>
      <label class="btn btn-tonal btn-sm">{tr("加载字幕")}<input type="file" accept=".vtt,.srt" class="sr-only" onChange={e => void pickSubtitle((e.target as HTMLInputElement).files?.[0])}/></label>
      <Btn label={tr("用外部播放器打开")} size="sm" disabled={!fileGate("external_player", entry.id).ok} reason={fileGate("external_player", entry.id).reason} onClick={() => playExternal(entry)}/>
      <Btn label={tr("下载片段")} size="sm" disabled={!fileGate("clip_download", entry.id).ok} reason={fileGate("clip_download", entry.id).reason} onClick={() => openClipPanel(entry)}/>
    </div> : null}
    {probe?.tracks?.length ? <details><summary>{tr("媒体轨道")}</summary><ul>{probe.tracks.map(t => <li key={t.id}>{t.kind} · {t.codec} · {t.language ?? tr("未知")}</li>)}</ul></details> : null}
    {probe?.warnings?.map(w => <p class="dim">{w}</p>)}
  </div>;
}
export function openPlayer(entry: RemoteEntry): void {
  if (!app.get().app?.feature_flags.media_session) return toast(tr("当前宿主未提供媒体播放"), "info");
  void openDialog({ title: entry.name, width: 1000, render: () => <PlayerView entry={entry}/> });
}
