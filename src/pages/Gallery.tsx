import { useEffect, useRef, useState } from "preact/hooks";
import { api, errorOf } from "../lib/api";
import type { ClientError, RemoteEntry } from "../lib/contract";
import { tr } from "../lib/i18n";
import { fileKind } from "../lib/names";
import { useStore } from "../lib/store";
import { app } from "../state/app";
import { navigate } from "../state/route";
import { PageFrame, PageTitle } from "../components/Frame";
import { Btn, Empty, ErrorPanel, Segmented, Spinner } from "../components/ui";
import { KindTile } from "../components/FileTable";
import { PlayerView } from "./Player";
import { loadGalleryImageUrl } from "../lib/gallery-image";

function GalleryImage({ entry }: { entry: RemoteEntry }) {
  const ref = useRef<HTMLDivElement>(null); const [url, setUrl] = useState<string>();
  useEffect(() => {
    let alive = true, request: AbortController | undefined, objectUrl: string | undefined;
    const release = () => { request?.abort(); request = undefined; if (objectUrl) URL.revokeObjectURL(objectUrl); objectUrl = undefined; if (alive) setUrl(undefined); };
    const open = () => {
      if (request) return;
      const controller = new AbortController(); request = controller;
      void loadGalleryImageUrl(entry.id, controller.signal).then(next => {
        if (!alive || controller.signal.aborted) URL.revokeObjectURL(next);
        else { objectUrl = next; setUrl(next); }
      }, () => { if (request === controller) request = undefined; });
    };
    const observer = new IntersectionObserver(entries => { if (entries.some(e => e.isIntersecting)) open(); else release(); }, { rootMargin: "120px" });
    if (ref.current) observer.observe(ref.current);
    return () => { alive = false; observer.disconnect(); release(); };
  }, [entry.id]);
  return <div class="gallery-image" ref={ref}>{url ? <img src={url} alt={entry.name} loading="lazy"/> : <KindTile entry={entry} size="lg"/>}</div>;
}
export function GalleryPage({ folder, mode }: { folder: string; mode: "images" | "videos" }) {
  const enabled = useStore(app, s => !!s.app?.feature_flags.media_session);
  const epoch = useStore(app, s => s.sessionEpoch);
  const [entries, setEntries] = useState<RemoteEntry[]>([]), [next, setNext] = useState<string>();
  const [busy, setBusy] = useState(false), [error, setError] = useState<ClientError>(), [active, setActive] = useState<string>();
  const generation = useRef(0);
  const load = async (cursor?: string) => {
    const current = ++generation.current; setBusy(true); setError(undefined);
    try { const page = await api.filesList(folder, cursor); if (current !== generation.current) return;
      setEntries(prev => cursor ? [...prev, ...page.items] : page.items); setNext(page.next_cursor);
    } catch (e) { if (current === generation.current) setError(errorOf(e)); }
    finally { if (current === generation.current) setBusy(false); }
  };
  useEffect(() => { void load(); return () => { generation.current++; }; }, [folder, epoch]);
  const media = entries.filter(e => fileKind(e.name, e.kind, e.mime_type) === (mode === "images" ? "image" : "video"));
  const selected = media.find(e => e.id === active);
  return <PageFrame label={tr("媒体库")} caption={<><Btn icon="arrowBack" iconOnly label={tr("返回文件列表")} onClick={() => navigate({ page: "files", folder })}/><PageTitle>{tr(mode === "images" ? "图库" : "视频信息流")}</PageTitle></>}
    commands={<Segmented label={tr("媒体类型")} value={mode} onChange={v => { setActive(undefined); navigate({ page: "gallery", folder, mode: v }); }} options={[{ value: "images", label: tr("图库") }, { value: "videos", label: tr("视频信息流") }]}/> }>
    <div class="gallery-scroll">
      {selected ? <section class="gallery-viewer"><div class="spread"><strong dir="auto">{selected.name}</strong><Btn label={tr("关闭")} icon="x" onClick={() => setActive(undefined)}/></div><PlayerView entry={selected} onEnded={mode === "videos" ? () => setActive(media[media.findIndex(e => e.id === active) + 1]?.id) : undefined}/></section> : null}
      {!enabled ? <Empty title={tr("当前宿主未提供媒体播放")}/> : <div class="gallery-grid">{media.map(entry => <button type="button" key={entry.id} class="gallery-card" onClick={() => setActive(entry.id)}>{mode === "images" ? <GalleryImage entry={entry}/> : <KindTile entry={entry} size="lg"/>}<span dir="auto" title={entry.name}>{entry.name}</span></button>)}</div>}
      {error ? <ErrorPanel error={error} onRetry={() => void load(next)}/> : busy ? <Spinner/> : next ? <Btn label={tr("继续读取")} onClick={() => void load(next)}/> : media.length === 0 ? <Empty title={tr("此文件夹中没有这种媒体")}/> : null}
    </div>
  </PageFrame>;
}
