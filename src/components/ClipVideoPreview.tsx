import { useEffect, useRef, useState } from "preact/hooks";
import type { MediaSession } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { safeMediaSessionUrl } from "../lib/gallery-image";
import { tr } from "../lib/i18n";
import { formatClock } from "../lib/clip";
import { onSessionReset, sessionGeneration } from "../state/session";
import { Btn, Spinner } from "./ui";
import { Icon } from "./Icon";

interface Props {
  remoteId: string;
  qualityId: string;
  positionMs: number;
  startMs: number;
  endMs: number;
  edge: "start" | "end";
}

/** System WebView decoding over a revocable Rust loopback stream. */
export function ClipVideoPreview(p: Props) {
  const video = useRef<HTMLVideoElement>(null);
  const [session, setSession] = useState<MediaSession>();
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [version, setVersion] = useState(0);
  const [generation] = useState(sessionGeneration());
  const [revoked, setRevoked] = useState(false);
  const release = useRef<() => void>(() => {});
  const currentPosition = useRef(p.positionMs);
  currentPosition.current = p.positionMs;

  useEffect(() => onSessionReset(() => {
    video.current?.pause();
    release.current();
    setSession(undefined);
    setRevoked(true);
  }), []);

  useEffect(() => {
    let alive = true;
    let opened: MediaSession | undefined;
    let released = false;
    const close = () => {
      if (!opened || released) return;
      released = true;
      void api.mediaSessionClose(opened.session_id).catch(() => {});
    };
    release.current = close;
    setSession(undefined); setError(undefined); setReady(false); setPlaying(false);
    // A reset revokes this panel's source; never reopen an old account's entry.
    if (revoked || generation !== sessionGeneration()) return close;
    void api.mediaSessionOpen(p.remoteId, p.qualityId).then(next => {
      opened = next;
      if (!alive || generation !== sessionGeneration()) { close(); return; }
      if (!safeMediaSessionUrl(next.url)) {
        close();
        setError(tr("当前版本尚未支持这项操作。"));
        return;
      }
      setSession(next);
    }).catch(failure => {
      if (alive && generation === sessionGeneration()) setError(errorOf(failure).message);
    });
    return () => {
      alive = false;
      video.current?.pause();
      close();
    };
  }, [p.remoteId, p.qualityId, version, generation, revoked]);

  const seek = (milliseconds: number) => {
    const element = video.current;
    if (!element || element.readyState < 1) return;
    const lastFrame = Number.isFinite(element.duration) ? Math.max(0, element.duration - 0.04) : milliseconds / 1000;
    const seconds = Math.min(Math.max(0, milliseconds / 1000), lastFrame);
    if (Math.abs(element.currentTime - seconds) > 0.025) element.currentTime = seconds;
  };

  useEffect(() => {
    video.current?.pause();
    setPlaying(false);
    // A dragged range can send dozens of positions; only seek the latest one.
    const timer = setTimeout(() => seek(currentPosition.current), 90);
    return () => clearTimeout(timer);
  }, [p.positionMs, p.startMs, p.endMs, session?.session_id]);

  const toggle = async () => {
    const element = video.current;
    if (!element || !ready) return;
    if (!element.paused) { element.pause(); return; }
    seek(p.startMs);
    try { await element.play(); }
    catch { setPlaying(false); }
  };

  return <div class="clip-video-preview">
    <div class="clip-preview" aria-live="off">
      {revoked ? <span class="clip-preview-off" role="status">{tr("账号会话已改变，本次结果不会应用到新账号。")}</span>
        : error ? <span class="clip-preview-off" role="status"><Icon name="alert" size={20}/>{error}</span>
        : session ? <>
          <video key={session.session_id} ref={video} src={session.url} preload="auto" crossOrigin="anonymous"
            muted playsInline aria-label={tr("播放预览")} style={{width:"100%",height:"100%",objectFit:"contain",display:"block"}}
            onLoadedMetadata={() => seek(currentPosition.current)}
            onLoadedData={() => { setReady(true); seek(currentPosition.current); }}
            onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)}
            onTimeUpdate={() => {
              const element = video.current;
              if (element && !element.paused && element.currentTime >= p.endMs / 1000 - 0.035) {
                element.pause(); seek(p.endMs);
              }
            }}
            onError={() => {
              video.current?.pause(); release.current(); setReady(false); setPlaying(false);
              setError(tr("当前系统无法解码此媒体，可尝试其他画质或外部播放器"));
            }}/>
          {!ready ? <span style={{position:"absolute"}}><Spinner label={tr("正在读取媒体")}/></span> : null}
        </> : <Spinner label={tr("正在读取媒体")}/>}
      <span class="clip-pos tabular">{p.edge === "start" ? tr("起点") : tr("终点")} {formatClock(p.positionMs)}</span>
    </div>
    <div class="row-gap" style={{paddingTop:"8px",justifyContent:"flex-start"}}>
      {revoked ? null : error ? <Btn size="sm" label={tr("重试")} icon="retry" onClick={() => setVersion(n => n + 1)}/>
        : <Btn size="sm" label={playing ? tr("暂停") : tr("播放预览")} icon={playing ? "pause" : "play"} disabled={!ready}
            reason={tr("正在读取媒体")} onClick={() => void toggle()}/>}
      <span class="dim tabular">{formatClock(p.startMs)}–{formatClock(p.endMs)}</span>
    </div>
  </div>;
}
