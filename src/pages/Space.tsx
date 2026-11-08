import { tr, tx } from "../lib/i18n";
/**
 * “空间分布”: a squarified treemap of what takes up space in a folder
 * (PROPOSAL UI-001 §P18). Sizes come from the core; click a folder tile
 * to drill in. Nothing here changes files.
 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError } from "../lib/contract";
import type { SpaceNode, SpaceSummary } from "../lib/contract-proposals";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { formatBytes, formatDateTime, groupDigits } from "../lib/format";
import { fileKind } from "../lib/names";
import { useStore } from "../lib/store";
import { squarify } from "../lib/treemap";
import { app, gate } from "../state/app";
import { navigate } from "../state/route";
import { Icon } from "../components/Icon";
import { Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { NavButtons } from "./Files";
import { useRefreshKey } from "./common";

const MAX_TILES = 48;

interface Node extends SpaceNode {
  value: number;
  rest?: number;
}

function kindClass(n: SpaceNode): string {
  if (n.id === "__rest") return "kind-other";
  return `kind-${n.kind === "folder" ? "folder" : fileKind(n.name, "file", n.mime_type)}`;
}

function pct(part: bigint, total: bigint): string {
  if (total <= 0n) return "0%";
  const v = Number((part * 1000n) / total) / 10;
  return v < 0.1 && part > 0n ? "<0.1%" : `${v.toFixed(v >= 10 ? 0 : 1)}%`;
}

export function SpacePage({ folder }: { folder: string }) {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const [data, setData] = useState<SpaceSummary | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [path, setPath] = useState<{ id: string; name: string }[]>([]);
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [hover, setHover] = useState<string | null>(null);
  const area = useRef<HTMLDivElement>(null);
  const g = gate("space_summary");

  const load = () => {
    setError(null);
    setData(null);
    api.spaceSummary(folder).then(setData, (e) => setError(errorOf(e)));
    if (folder === ROOT_ID) setPath([]);
    else api.filesPath(folder).then((c) => setPath(c.map((e) => ({ id: e.id, name: e.name }))), () => setPath([{ id: folder, name: "…" }]));
  };
  useEffect(() => { if (g.ok) load(); }, [folder, epoch, refresh]);

  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    const measure = () => setBox({ w: el.clientWidth, h: el.clientHeight });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [data]);

  const nodes: Node[] = useMemo(() => {
    if (!data) return [];
    const list = data.children
      .map((c) => ({ ...c, value: Number(BigInt(c.size_bytes) / 1024n) + (BigInt(c.size_bytes) > 0n ? 1 : 0) }))
      .filter((c) => c.value > 0)
      .sort((a, b) => b.value - a.value);
    if (list.length <= MAX_TILES) return list;
    const head = list.slice(0, MAX_TILES - 1);
    const tail = list.slice(MAX_TILES - 1);
    const bytes = tail.reduce((s, c) => s + BigInt(c.size_bytes), 0n);
    return [...head, { id: "__rest", name: tx("其他 {0} 项", tail.length), kind: "file", size_bytes: bytes.toString(), file_count: tail.reduce((s, c) => s + c.file_count, 0), value: tail.reduce((s, c) => s + c.value, 0), rest: tail.length }];
  }, [data]);

  const tiles = useMemo(() => squarify(nodes, (n) => n.value, { x: 0, y: 0, w: box.w, h: box.h }), [nodes, box.w, box.h]);
  const total = data ? BigInt(data.total_bytes) : 0n;
  const here = path[path.length - 1]?.name ?? tr("网盘");
  const open = (n: Node) => {
    if (n.id === "__rest") return;
    if (n.kind === "folder") navigate({ page: "space", folder: n.id });
    else navigate({ page: "files", folder });
  };

  return (
    <PageFrame label={tr("空间分布")}
      caption={<>
        <NavButtons up={folder !== ROOT_ID ? () => navigate({ page: "space", folder: path[path.length - 2]?.id ?? ROOT_ID }) : undefined} />
        <PageTitle>{tr("空间分布")}</PageTitle>
        <nav class="mini-crumbs" aria-label={tr("位置")}>
          <a href="#/space" onClick={(e) => { e.preventDefault(); navigate({ page: "space", folder: ROOT_ID }); }}>{tr("网盘")}</a>
          {path.map((c, i) => (
            <>
              <Icon name="chevronRight" size={16} class="crumb-sep" />
              {i === path.length - 1 ? <span aria-current="location" dir="auto">{c.name}</span>
                : <a href={`#/space/${encodeURIComponent(c.id)}`} dir="auto" onClick={(e) => { e.preventDefault(); navigate({ page: "space", folder: c.id }); }}>{c.name}</a>}
            </>
          ))}
        </nav>
      </>}
      commands={<>
        <div class="cmd-group">
          <Btn tone="ghost" icon="folderOpen" label={tx("打开“{0}”", here)} onClick={() => navigate({ page: "files", folder })} />
          <Btn tone="quiet" iconOnly icon="refresh" label={tr("重新统计")} onClick={load} />
        </div>
        <div class="cmd-group cmd-end">
          {data ? <span class="cmd-meta">{tr("共")}{formatBytes(data.total_bytes)} · {groupDigits(String(data.file_count))} {tr("个文件 ·")}{groupDigits(String(data.folder_count))} {tr("个文件夹")}</span> : null}
        </div>
      </>}>
      {!g.ok ? <Empty title={tr("当前不能统计空间")}>{g.reason}</Empty>
        : error ? <div class="canvas-pad"><ErrorPanel error={error} onRetry={load} context={`files_space_summary folder=${folder}`} /></div>
          : !data ? <div class="space-wait"><Spinner label={tr("正在统计")} /><p class="dim">{tr("正在统计")}{here} {tr("中每个项目占用的空间…")}</p></div>
            : !nodes.length ? <Empty title={tr("这里没有占用空间的文件")} />
              : (
                <div class="space">
                  <div class="space-map" ref={area} role="list" aria-label={tx("{0} 的空间分布", here)}>
                    {tiles.map((t) => {
                      const n = t.item;
                      const big = t.w > 96 && t.h > 44;
                      return (
                        <button type="button" role="listitem" class={`tm-tile ${kindClass(n)}${hover === n.id ? " hot" : ""}${n.id === "__rest" ? " rest" : ""}`}
                          style={{ left: `${t.x}px`, top: `${t.y}px`, width: `${t.w}px`, height: `${t.h}px` }}
                          title={`${n.name}\n${formatBytes(n.size_bytes)}（${pct(BigInt(n.size_bytes), total)}）`}
                          aria-label={tx("{0}，{1}，占 {2}{3}", n.name, formatBytes(n.size_bytes), pct(BigInt(n.size_bytes), total), n.kind === "folder" ? "，打开查看内部" : "")}
                          disabled={n.id === "__rest"}
                          onMouseEnter={() => setHover(n.id)} onMouseLeave={() => setHover(null)}
                          onClick={() => open(n)}>
                          {big ? (
                            <span class="tm-label">
                              <span class="tm-name ellipsis" dir="auto">{n.kind === "folder" ? <Icon name="folder" size={16} filled /> : null}{n.name}</span>
                              <span class="tm-size tabular">{formatBytes(n.size_bytes)}</span>
                            </span>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                  <aside class="space-legend" aria-label={tr("占用最多的项目")}>
                    <h2 class="space-h">{tr("占用最多")}</h2>
                    <ol>
                      {nodes.slice(0, 14).map((n) => (
                        <li class={hover === n.id ? "hot" : ""} onMouseEnter={() => setHover(n.id)} onMouseLeave={() => setHover(null)}>
                          <button type="button" class="space-row" disabled={n.id === "__rest"} onClick={() => open(n)}>
                            <span class={`space-dot ${kindClass(n)}`} />
                            <span class="ellipsis" dir="auto" title={n.name}>{n.name}</span>
                            <span class="dim tabular">{pct(BigInt(n.size_bytes), total)}</span>
                            <span class="space-bar" aria-hidden="true"><span style={{ width: pct(BigInt(n.size_bytes), total).replace("<", "") }} /></span>
                            <span class="tabular space-size">{formatBytes(n.size_bytes)}{n.kind === "folder" ? tx(" · {0} 个文件", groupDigits(String(n.file_count))) : ""}</span>
                          </button>
                        </li>
                      ))}
                    </ol>
                    {data.exactness === "partial" ? <p class="inline-note warn"><Icon name="alert" size={16} />{data.note ?? tr("部分文件夹无法读取，大小为下限。")}</p> : null}
                    <p class="dim space-foot">{tr("统计于")}{formatDateTime(data.computed_at)}{tr("。点击文件夹查看内部。")}</p>
                  </aside>
                </div>
              )}
    </PageFrame>
  );
}
