import { tr, tx } from "../lib/i18n";
import { Fragment } from "preact";
import type { JSX } from "preact";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { RemoteEntry } from "../lib/contract";
import { isStarred } from "../lib/contract-proposals";
import { formatBytes, formatBytesExact, formatDateTime } from "../lib/format";
import { fileKind, KIND_LABEL, splitName, windowsIssues, type FileKind } from "../lib/names";
import { useStore } from "../lib/store";
import { useVirtualRows } from "../lib/virtual";
import { clip } from "../state/clipboard";
import type { GridSize } from "../state/prefs";
import { Icon } from "./Icon";
import { onSessionReset } from "../state/session";

export type ColumnId = "name" | "size" | "modified" | "kind" | "location";
export type SortKey = "name" | "size" | "modified" | "kind";
export interface SortSpec {
  key: SortKey;
  dir: "asc" | "desc";
}

const HEADER_H = 40;
/** Card geometry per grid size: min column width, row pitch, icon size. */
export const GRID: Record<GridSize, { min: number; row: number; icon: number }> = {
  s: { min: 132, row: 176, icon: 36 },
  m: { min: 168, row: 212, icon: 48 },
  l: { min: 224, row: 272, icon: 64 },
};
const GRID_PAD_X = 16; // .ft-grid scroller padding (12) + row padding (4)
const GRID_PAD_TOP = 8;
const GRID_GAP = 12;

/* ------------------------------------------------------------------ */
/* Drag and drop between the list, breadcrumbs and the sidebar         */
/* ------------------------------------------------------------------ */

export const DRAG_MIME = "application/x-nekopak-entries";

/** What is being dragged right now (dataTransfer is unreadable during dragover). */
export const dragState: { entries: RemoteEntry[] } = { entries: [] };

/** Props for any element that accepts a drop of cloud items into `folderId`. */
export function dropTarget(folderId: string, onDrop: (op: "move" | "copy", entries: RemoteEntry[], dest: string) => void) {
  const accepts = (e: DragEvent) => e.dataTransfer?.types.includes(DRAG_MIME) && !dragState.entries.some((x) => x.id === folderId);
  return {
    onDragOver: (e: DragEvent) => {
      if (!accepts(e)) return;
      e.preventDefault();
      e.dataTransfer!.dropEffect = e.ctrlKey ? "copy" : "move";
      (e.currentTarget as HTMLElement).classList.add("drop-on");
    },
    onDragLeave: (e: DragEvent) => (e.currentTarget as HTMLElement).classList.remove("drop-on"),
    onDrop: (e: DragEvent) => {
      (e.currentTarget as HTMLElement).classList.remove("drop-on");
      if (!accepts(e)) return;
      e.preventDefault();
      const entries = dragState.entries;
      dragState.entries = [];
      if (entries.length) onDrop(e.ctrlKey ? "copy" : "move", entries, folderId);
    },
  };
}

const KIND_ICON: Record<FileKind, string> = {
  folder: "folder", video: "video", audio: "audio", image: "image",
  doc: "doc", archive: "archive", code: "code", other: "file",
};

const COLUMN_META: Record<ColumnId, { label: string; width: string; sortKey?: SortKey; minWidth: number }> = {
  name: { get label() { return tr("名称"); }, width: "minmax(180px, 1fr)", sortKey: "name", minWidth: 0 },
  location: { get label() { return tr("位置"); }, width: "minmax(120px, 0.5fr)", minWidth: 640 },
  kind: { get label() { return tr("类型"); }, width: "104px", sortKey: "kind", minWidth: 900 },
  size: { get label() { return tr("大小"); }, width: "104px", sortKey: "size", minWidth: 440 },
  modified: { get label() { return tr("修改时间"); }, width: "150px", sortKey: "modified", minWidth: 600 },
};

/* ------------------------------------------------------------------ */
/* Selection                                                           */
/* ------------------------------------------------------------------ */

export interface Selection {
  ids: ReadonlySet<string>;
  anchor: number;
  focus: number;
}

const viewSelections = new Map<string, Selection>();
const viewScroll = new Map<string, number>();
onSessionReset(() => { viewSelections.clear(); viewScroll.clear(); });
export function useSelection(items: readonly RemoteEntry[], resetKey: unknown, memoryKey?: string, ready = true) {
  const restore = () => memoryKey && viewSelections.get(memoryKey) || { ids: new Set<string>(), anchor: 0, focus: 0 };
  const [sel, setSel] = useState<Selection>(restore);
  useEffect(() => setSel(restore()), [resetKey, memoryKey]);
  useEffect(() => {
    if (!memoryKey || !ready) return;
    if (viewSelections.size >= 512 && !viewSelections.has(memoryKey)) viewSelections.delete(viewSelections.keys().next().value!);
    viewSelections.set(memoryKey, sel);
  }, [sel, memoryKey, ready]);

  // Drop ids that disappeared after a reload; keep focus in range.
  useEffect(() => {
    if (!ready) return;
    setSel((s) => {
      const present = new Set(items.map((i) => i.id));
      let changed = false;
      const ids = new Set<string>();
      for (const id of s.ids) {
        if (present.has(id)) ids.add(id);
        else changed = true;
      }
      const focus = Math.min(s.focus, Math.max(0, items.length - 1));
      return changed || focus !== s.focus ? { ids, anchor: Math.min(s.anchor, focus), focus } : s;
    });
  }, [items, ready]);

  const selected = useMemo(() => items.filter((i) => sel.ids.has(i.id)), [items, sel.ids]);
  return { sel, setSel, selected };
}

function rangeIds(items: readonly RemoteEntry[], a: number, b: number): Set<string> {
  const [lo, hi] = a < b ? [a, b] : [b, a];
  const out = new Set<string>();
  for (let i = lo; i <= hi && i < items.length; i++) out.add(items[i].id);
  return out;
}

/* ------------------------------------------------------------------ */
/* Name, kind tile and leading checkbox                                */
/* ------------------------------------------------------------------ */

export function kindOf(e: RemoteEntry): FileKind {
  return fileKind(e.name, e.kind, e.mime_type);
}

export function KindTile({ entry, size = "sm" }: { entry: RemoteEntry; size?: "sm" | "lg" }) {
  const kind = kindOf(entry);
  return (
    <span class={`tile tile-${size} kind-${kind}`} aria-hidden="true">
      <Icon name={KIND_ICON[kind]} size={size === "lg" ? 48 : 20} filled={kind === "folder"} />
    </span>
  );
}

export function StarMark({ entry }: { entry: RemoteEntry }) {
  return isStarred(entry) ? <span class="star-mark" title={tr("已加星标")} aria-label={tr("已加星标")}><Icon name="star" size={16} filled /></span> : null;
}

export function EntryName({ entry, showWarn = true, withIcon = true }: { entry: RemoteEntry; showWarn?: boolean; withIcon?: boolean }) {
  const { base, ext } = entry.kind === "folder" ? { base: entry.name, ext: "" } : splitName(entry.name);
  const issues = showWarn ? windowsIssues(entry.name) : [];
  return (
    <span class="entry-name" title={entry.name}>
      {withIcon ? <KindTile entry={entry} /> : null}
      <span class="name-text" dir="auto">
        <span class="name-base">{base}</span>
        {ext ? <span class="name-ext">{ext}</span> : null}
      </span>
      <StarMark entry={entry} />
      {issues.length ? (
        <span class="name-warn" title={tx("下载到 Windows 时需转义：{0}", issues.join("；"))} aria-label={tx("注意：{0}", issues.join("；"))}>
          <Icon name="alert" size={16} filled />
        </span>
      ) : null}
    </span>
  );
}

/** Kind tile that turns into a checkbox on hover / selection, in the same box (no horizontal shift). */
function Lead({ entry, selected, onToggle }: { entry: RemoteEntry; selected: boolean; onToggle: () => void }) {
  return (
    <span class={`lead${selected ? " on" : ""}`}>
      <KindTile entry={entry} />
      <span
        class="lead-check"
        aria-hidden="true"
        onMouseDown={(ev) => {
          ev.stopPropagation();
          onToggle();
        }}
      >
        <Icon name={selected ? "checkbox" : "checkboxOff"} filled={selected} size={22} />
      </span>
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Table / grid                                                        */
/* ------------------------------------------------------------------ */

export interface FileTableProps {
  scrollKey?: string;
  id: string;
  label: string;
  items: readonly RemoteEntry[];
  columns: ColumnId[];
  mode?: "list" | "grid";
  sort?: SortSpec;
  onSort?: (key: SortKey) => void;
  selection: Selection;
  setSelection: (s: Selection | ((p: Selection) => Selection)) => void;
  rowHeight: number;
  onOpen: (entry: RemoteEntry) => void;
  onMenu: (entries: RemoteEntry[], x: number, y: number) => void;
  onKeyAction?: (action: KeyAction, entries: RemoteEntry[]) => void;
  /** Card size in grid mode. */
  gridSize?: GridSize;
  /** Ctrl + wheel over the list: +1 bigger, −1 smaller. */
  onZoom?: (dir: 1 | -1) => void;
  /** Enables dragging items; dropping on a folder row/card calls this. */
  onDropInto?: (op: "move" | "copy", entries: RemoteEntry[], dest: string) => void;
  locationOf?: (e: RemoteEntry) => string;
  dateLabel?: string;
  hasMore?: boolean;
  loadingMore?: boolean;
  onLoadMore?: () => void;
  footer?: JSX.Element | null;
}

export type KeyAction = "rename" | "trash" | "copy-path" | "copy" | "cut" | "paste" | "parent" | "properties" | "star" | "undo";

interface Marquee {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  base: ReadonlySet<string>;
}

export function FileTable(p: FileTableProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(1000);
  const grid = p.mode === "grid";
  const g = GRID[p.gridSize ?? "m"];
  const perRow = grid ? Math.max(1, Math.floor((width - 24) / g.min)) : 1;
  const rowsCount = grid ? Math.ceil(p.items.length / perRow) : p.items.length;
  const rowH = grid ? g.row : p.rowHeight;
  const cut = useStore(clip, (c) => (c.mode === "cut" ? c.entries.map((e) => e.id).join("|") : ""));
  const cutIds = useMemo(() => new Set(cut ? cut.split("|") : []), [cut]);
  const [marquee, setMarquee] = useState<Marquee | null>(null);
  const { range, ensureVisible } = useVirtualRows(scroller, rowsCount, rowH, grid ? 3 : 10);

  useLayoutEffect(() => {
    const el = scroller.current, key = p.scrollKey;
    if (!el || !key) return;
    el.scrollTop = viewScroll.get(key) ?? 0;
    const save = () => viewScroll.set(key, el.scrollTop);
    el.addEventListener("scroll", save, { passive: true });
    return () => { save(); el.removeEventListener("scroll", save); };
  }, [p.scrollKey]);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    setWidth(el.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const cols = useMemo(() => p.columns.filter((c) => width >= COLUMN_META[c].minWidth), [p.columns, width]);
  const template = `44px ${cols.map((c) => COLUMN_META[c].width).join(" ")}`;

  // Infinite paging: ask for more when the viewport nears the end.
  useEffect(() => {
    const end = range.end * perRow;
    if (p.hasMore && !p.loadingMore && p.onLoadMore && end >= p.items.length - 40) p.onLoadMore();
  }, [range.end, p.items.length, p.hasMore, p.loadingMore, perRow]);

  const { sel, setSelection, items } = { sel: p.selection, setSelection: p.setSelection, items: p.items };

  const focusRow = useCallback(
    (i: number, mode: "single" | "extend" | "move" = "single") => {
      if (items.length === 0) return;
      const idx = Math.max(0, Math.min(items.length - 1, i));
      setSelection((s) => {
        if (mode === "move") return { ...s, focus: idx };
        if (mode === "extend") return { ids: rangeIds(items, s.anchor, idx), anchor: s.anchor, focus: idx };
        return { ids: new Set([items[idx].id]), anchor: idx, focus: idx };
      });
      ensureVisible(Math.floor(idx / perRow), grid ? 0 : HEADER_H);
    },
    [items, setSelection, ensureVisible, perRow, grid],
  );

  const toggle = (i: number) =>
    setSelection((s) => {
      const ids = new Set(s.ids);
      if (ids.has(items[i].id)) ids.delete(items[i].id);
      else ids.add(items[i].id);
      return { ids, anchor: i, focus: i };
    });

  const selectedEntries = () => {
    const chosen = items.filter((x) => sel.ids.has(x.id));
    if (chosen.length) return chosen;
    const f = items[sel.focus];
    return f ? [f] : [];
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / rowH) - 1) * perRow;
    const mode = e.shiftKey ? "extend" : e.ctrlKey || e.metaKey ? "move" : "single";
    const ctrl = e.ctrlKey || e.metaKey;
    switch (e.key) {
      case "ArrowDown": focusRow(sel.focus + perRow, mode); break;
      case "ArrowUp":
        if (e.altKey) p.onKeyAction?.("parent", []);
        else focusRow(sel.focus - perRow, mode);
        break;
      case "ArrowRight": if (!grid) return; focusRow(sel.focus + 1, mode); break;
      case "ArrowLeft": if (!grid) return; focusRow(sel.focus - 1, mode); break;
      case "PageDown": focusRow(sel.focus + page, mode); break;
      case "PageUp": focusRow(sel.focus - page, mode); break;
      case "Home": focusRow(0, mode); break;
      case "End": focusRow(items.length - 1, mode); break;
      case " ":
        if (items[sel.focus]) {
          setSelection((s) => {
            const ids = new Set(s.ids);
            const id = items[s.focus].id;
            if (ids.has(id) && ctrl) ids.delete(id);
            else if (ids.has(id) && !ctrl) return s;
            else ids.add(id);
            return { ...s, ids, anchor: s.focus };
          });
        }
        break;
      case "Enter":
        if (items[sel.focus]) {
          if (e.altKey) p.onKeyAction?.("properties", [items[sel.focus]]);
          else p.onOpen(items[sel.focus]);
        }
        break;
      case "Backspace": p.onKeyAction?.("parent", []); break;
      case "Escape":
        if (sel.ids.size === 0) return;
        setSelection((s) => ({ ...s, ids: new Set() }));
        break;
      case "F2": p.onKeyAction?.("rename", selectedEntries()); break;
      case "Delete": p.onKeyAction?.("trash", selectedEntries()); break;
      case "ContextMenu":
      case "F10": {
        if (e.key === "F10" && !e.shiftKey) return;
        const el = scroller.current?.querySelector<HTMLElement>(`[data-index="${sel.focus}"]`);
        const r = el?.getBoundingClientRect() ?? scroller.current!.getBoundingClientRect();
        p.onMenu(selectedEntries(), r.left + 48, r.top + Math.min(r.height, 36));
        break;
      }
      default:
        if (ctrl && (e.key === "a" || e.key === "A")) {
          setSelection((s) => ({ ...s, ids: new Set(items.map((i) => i.id)) }));
          break;
        }
        if (ctrl && (e.key === "c" || e.key === "C")) {
          p.onKeyAction?.(e.shiftKey ? "copy-path" : "copy", selectedEntries());
          break;
        }
        if (ctrl && (e.key === "x" || e.key === "X")) {
          p.onKeyAction?.("cut", selectedEntries());
          break;
        }
        if (ctrl && (e.key === "v" || e.key === "V")) {
          p.onKeyAction?.("paste", []);
          break;
        }
        if (ctrl && (e.key === "d" || e.key === "D")) {
          p.onKeyAction?.("star", selectedEntries());
          break;
        }
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  // A plain press on an item that is part of a multi-selection keeps the
  // selection (so the group can be dragged) and collapses it on click.
  const pendingSingle = useRef<number | null>(null);
  const onItemMouseDown = (e: MouseEvent, i: number) => {
    pendingSingle.current = null;
    if (e.button === 2) {
      if (!sel.ids.has(items[i].id)) setSelection({ ids: new Set([items[i].id]), anchor: i, focus: i });
      return;
    }
    if (e.button !== 0) return;
    if (e.shiftKey) {
      setSelection((s) => ({ ids: rangeIds(items, s.anchor, i), anchor: s.anchor, focus: i }));
      e.preventDefault();
    } else if (e.ctrlKey || e.metaKey) {
      toggle(i);
    } else if (p.onDropInto && sel.ids.has(items[i].id) && sel.ids.size > 1) {
      pendingSingle.current = i;
      setSelection((s) => ({ ...s, focus: i }));
    } else {
      setSelection({ ids: new Set([items[i].id]), anchor: i, focus: i });
    }
  };
  const onItemClick = (i: number) => {
    if (pendingSingle.current === i) setSelection({ ids: new Set([items[i].id]), anchor: i, focus: i });
    pendingSingle.current = null;
  };

  const drop = p.onDropInto;
  const itemProps = (e: RemoteEntry, i: number, selected: boolean) => ({
    id: `${p.id}-row-${i}`,
    "aria-selected": selected ? "true" : "false",
    "data-index": i,
    "data-id": e.id,
    ...(drop ? {
      draggable: true,
      onDragStart: (ev: DragEvent) => {
        const chosen = sel.ids.has(e.id) ? items.filter((x) => sel.ids.has(x.id)) : [e];
        if (!sel.ids.has(e.id)) setSelection({ ids: new Set([e.id]), anchor: i, focus: i });
        dragState.entries = chosen;
        ev.dataTransfer!.effectAllowed = "copyMove";
        ev.dataTransfer!.setData(DRAG_MIME, chosen.map((x) => x.id).join(","));
        ev.dataTransfer!.setData("text/plain", chosen.map((x) => x.name).join("\n"));
        const ghost = document.createElement("div");
        ghost.className = "drag-ghost";
        ghost.textContent = chosen.length > 1 ? tx("{0} 个项目", chosen.length) : chosen[0].name;
        document.body.appendChild(ghost);
        ev.dataTransfer!.setDragImage(ghost, 16, 16);
        setTimeout(() => ghost.remove(), 0);
      },
      onDragEnd: () => {
        dragState.entries = [];
        pendingSingle.current = null;
      },
      ...(e.kind === "folder" ? dropTarget(e.id, drop) : {}),
    } : {}),
    onMouseDown: (ev: MouseEvent) => onItemMouseDown(ev, i),
    onClick: () => onItemClick(i),
    onDblClick: () => p.onOpen(e),
    onContextMenu: (ev: MouseEvent) => {
      ev.preventDefault();
      const chosen = sel.ids.has(e.id) ? items.filter((x) => sel.ids.has(x.id)) : [e];
      p.onMenu(chosen, ev.clientX, ev.clientY);
    },
  });

  const allSelected = items.length > 0 && sel.ids.size === items.length;
  const someSelected = sel.ids.size > 0 && !allSelected;
  const activeId = items[sel.focus] ? `${p.id}-row-${sel.focus}` : undefined;

  const rows: JSX.Element[] = [];
  if (grid) {
    for (let r = range.start; r < range.end; r++) {
      const cells: JSX.Element[] = [];
      for (let c = 0; c < perRow; c++) {
        const i = r * perRow + c;
        const e = items[i];
        if (!e) break;
        const selected = sel.ids.has(e.id);
        cells.push(
          <div key={e.id} role="gridcell" class={`card${selected ? " selected" : ""}${i === sel.focus ? " focused" : ""}${cutIds.has(e.id) ? " is-cut" : ""}`}
            style={{ height: `${g.row - GRID_GAP}px`, "--card-icon": `${g.icon}px` } as JSX.CSSProperties} {...(itemProps(e, i, selected) as object)}>
            <div class="card-art">
              <KindTile entry={e} size="lg" />
              {isStarred(e) ? <span class="card-star" aria-label={tr("已加星标")}><Icon name="star" size={18} filled /></span> : null}
              <span class={`card-check${selected ? " on" : ""}`} aria-hidden="true" onMouseDown={(ev) => { ev.stopPropagation(); toggle(i); }}>
                <Icon name={selected ? "checkbox" : "checkboxOff"} filled={selected} size={22} />
              </span>
            </div>
            <span class="card-name" dir="auto" title={e.name}>{e.name}</span>
            <span class="card-meta">{e.kind === "folder" ? formatDateTime(e.modified_at) : `${formatBytes(e.size_bytes)}`}</span>
          </div>,
        );
      }
      rows.push(<div key={`r${r}`} role="row" aria-rowindex={r + 1} class="grid-row" style={{ gridTemplateColumns: `repeat(${perRow}, minmax(0, 1fr))`, height: `${g.row}px` }}>{cells}</div>);
    }
  } else {
    for (let i = range.start; i < range.end; i++) {
      const e = items[i];
      const selected = sel.ids.has(e.id);
      rows.push(
        <div key={e.id} role="row" aria-rowindex={i + 2} class={`ft-row${selected ? " selected" : ""}${i === sel.focus ? " focused" : ""}${cutIds.has(e.id) ? " is-cut" : ""}`}
          style={{ gridTemplateColumns: template, height: `${p.rowHeight}px` }} {...(itemProps(e, i, selected) as object)}>
          <div role="gridcell" class="ft-lead"><Lead entry={e} selected={selected} onToggle={() => toggle(i)} /></div>
          {cols.map((c) => (
            <div role="gridcell" class={`ft-cell ft-${c}`}>
              {c === "name" ? <EntryName entry={e} withIcon={false} />
                : c === "size" ? (e.kind === "folder" ? <span class="dim">—</span> : <span title={formatBytesExact(e.size_bytes)}>{formatBytes(e.size_bytes)}</span>)
                  : c === "modified" ? <span title={e.modified_at}>{formatDateTime(e.modified_at)}</span>
                    : c === "kind" ? KIND_LABEL[kindOf(e)]
                      : <span title={p.locationOf?.(e)} dir="auto">{p.locationOf?.(e) ?? "—"}</span>}
            </div>
          ))}
        </div>,
      );
    }
  }

  /* Marquee (rubber-band) selection from empty space. Coordinates are in
     scroll-content space so the band survives scrolling. */
  const contentPoint = (ev: MouseEvent) => {
    const el = scroller.current!;
    const r = el.getBoundingClientRect();
    return { x: ev.clientX - r.left + el.scrollLeft, y: ev.clientY - r.top + el.scrollTop };
  };

  const hitIds = (m: Marquee): Set<string> => {
    const top = Math.min(m.y0, m.y1);
    const bottom = Math.max(m.y0, m.y1);
    const left = Math.min(m.x0, m.x1);
    const right = Math.max(m.x0, m.x1);
    const out = new Set(m.base);
    if (grid) {
      const colW = (width - 24 - 8 - GRID_GAP * (perRow - 1)) / perRow;
      const r0 = Math.max(0, Math.floor((top - GRID_PAD_TOP) / g.row));
      const r1 = Math.min(rowsCount - 1, Math.floor((bottom - GRID_PAD_TOP) / g.row));
      for (let r = r0; r <= r1; r++) {
        const y = GRID_PAD_TOP + r * g.row;
        if (y > bottom || y + g.row - GRID_GAP < top) continue;
        for (let c = 0; c < perRow; c++) {
          const x = GRID_PAD_X + c * (colW + GRID_GAP);
          if (x > right || x + colW < left) continue;
          const e = items[r * perRow + c];
          if (e) out.add(e.id);
        }
      }
    } else {
      const r0 = Math.max(0, Math.floor((top - HEADER_H) / rowH));
      const r1 = Math.min(items.length - 1, Math.floor((bottom - HEADER_H) / rowH));
      for (let i = r0; i <= r1; i++) out.add(items[i].id);
    }
    return out;
  };

  const onBlankMouseDown = (ev: MouseEvent) => {
    if (ev.button !== 0) return;
    const t = ev.target as HTMLElement;
    if (t.closest(".ft-row, .card, .ft-head, .ft-foot, button, a, input")) return;
    const el = scroller.current!;
    if (ev.offsetX > el.clientWidth || ev.offsetY > el.clientHeight) return; // scrollbar
    const pt = contentPoint(ev);
    const additive = ev.ctrlKey || ev.metaKey || ev.shiftKey;
    const base = additive ? new Set(sel.ids) : new Set<string>();
    let m: Marquee = { x0: pt.x, y0: pt.y, x1: pt.x, y1: pt.y, base };
    let moved = false;
    el.focus({ preventScroll: true });
    ev.preventDefault();
    const onMove = (e2: MouseEvent) => {
      const q = contentPoint(e2);
      if (!moved && Math.abs(q.x - m.x0) + Math.abs(q.y - m.y0) < 4) return;
      moved = true;
      m = { ...m, x1: q.x, y1: q.y };
      setMarquee(m);
      setSelection((s0) => ({ ...s0, ids: hitIds(m) }));
      // auto-scroll near the edges
      const r = el.getBoundingClientRect();
      if (e2.clientY > r.bottom - 24) el.scrollTop += 24;
      else if (e2.clientY < r.top + 24 + (grid ? 0 : HEADER_H)) el.scrollTop -= 24;
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setMarquee(null);
      if (!moved && !additive) setSelection((s0) => ({ ...s0, ids: new Set() }));
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const onWheel = (ev: WheelEvent) => {
    if (!(ev.ctrlKey || ev.metaKey) || !p.onZoom) return;
    ev.preventDefault();
    p.onZoom(ev.deltaY < 0 ? 1 : -1);
  };
  useEffect(() => {
    const el = scroller.current;
    if (!el || !p.onZoom) return;
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  return (
    <div class={`ft${grid ? " ft-grid" : ""}`}>
      <div
        ref={scroller}
        class="ft-scroll"
        onMouseDown={(e) => onBlankMouseDown(e as unknown as MouseEvent)}
        role="grid"
        id={p.id}
        aria-label={p.label}
        aria-multiselectable="true"
        aria-rowcount={grid ? rowsCount : items.length + 1}
        aria-colcount={grid ? perRow : cols.length + 1}
        aria-activedescendant={activeId}
        tabIndex={0}
        onKeyDown={(e) => onKeyDown(e as unknown as KeyboardEvent)}
        onContextMenu={(e) => {
          if ((e.target as HTMLElement).closest(".ft-row, .card")) return;
          e.preventDefault();
          p.onMenu([], e.clientX, e.clientY);
        }}
      >
        {grid ? null : (
          <div class="ft-head" role="row" aria-rowindex={1} style={{ gridTemplateColumns: template }}>
            <div role="columnheader" class="ft-lead">
              <button
                type="button"
                class={`head-check${allSelected || someSelected ? " on" : ""}`}
                aria-label={allSelected ? tr("取消全选") : tr("全选（Ctrl+A）")}
                tabIndex={-1}
                onClick={() => setSelection((s) => ({ ...s, ids: allSelected ? new Set() : new Set(items.map((i) => i.id)) }))}
              >
                <Icon name={allSelected ? "checkbox" : someSelected ? "indeterminate" : "checkboxOff"} filled={allSelected || someSelected} size={20} />
              </button>
            </div>
            {cols.map((c) => {
              const meta = COLUMN_META[c];
              const sortable = !!(meta.sortKey && p.onSort);
              const active = p.sort && meta.sortKey === p.sort.key;
              const label = c === "modified" && p.dateLabel ? p.dateLabel : meta.label;
              return (
                <div role="columnheader" class={`ft-cell ft-${c}`} aria-sort={active ? (p.sort!.dir === "asc" ? "ascending" : "descending") : sortable ? "none" : undefined}>
                  {sortable ? (
                    <button type="button" class="ft-sort" tabIndex={-1} onClick={() => p.onSort!(meta.sortKey!)}>
                      {label}
                      {active ? <Icon name={p.sort!.dir === "asc" ? "chevronUp" : "chevronDown"} size={16} /> : null}
                    </button>
                  ) : label}
                </div>
              );
            })}
          </div>
        )}
        <div class="ft-body" style={{ height: `${range.total}px` }}>
          <div class="ft-window" style={{ transform: `translateY(${range.padTop}px)` }}>
            <Fragment>{rows}</Fragment>
          </div>
        </div>
        {marquee ? (
          <div class="marquee" aria-hidden="true" style={{
            left: `${Math.min(marquee.x0, marquee.x1)}px`, top: `${Math.min(marquee.y0, marquee.y1)}px`,
            width: `${Math.abs(marquee.x1 - marquee.x0)}px`, height: `${Math.abs(marquee.y1 - marquee.y0)}px`,
          }} />
        ) : null}
        {p.footer ?? null}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Sorting (stable, BigInt sizes, locale-aware names)                  */
/* ------------------------------------------------------------------ */

const collator = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });

export function sortEntries(items: readonly RemoteEntry[], spec: SortSpec, foldersFirst = true): RemoteEntry[] {
  const dir = spec.dir === "asc" ? 1 : -1;
  const indexed = items.map((e, i) => ({ e, i, size: e.kind === "folder" ? -1n : safeBig(e.size_bytes) }));
  indexed.sort((a, b) => {
    if (foldersFirst && a.e.kind !== b.e.kind) return a.e.kind === "folder" ? -1 : 1;
    let c = 0;
    switch (spec.key) {
      case "name": c = collator.compare(a.e.name, b.e.name); break;
      case "size": c = a.size < b.size ? -1 : a.size > b.size ? 1 : 0; break;
      case "modified": c = (a.e.modified_at ?? "").localeCompare(b.e.modified_at ?? ""); break;
      case "kind": c = collator.compare(KIND_LABEL[fileKind(a.e.name, a.e.kind, a.e.mime_type)], KIND_LABEL[fileKind(b.e.name, b.e.kind, b.e.mime_type)]); break;
    }
    if (c === 0 && spec.key !== "name") c = collator.compare(a.e.name, b.e.name);
    return c * dir || a.i - b.i;
  });
  return indexed.map((x) => x.e);
}

function safeBig(s: string): bigint {
  return /^\d+$/.test(s) ? BigInt(s) : 0n;
}
