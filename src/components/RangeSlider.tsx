import { tx } from "../lib/i18n";
/**
 * Two-thumb range slider (M3 style) for choosing a clip. Each thumb is a
 * focusable role="slider"; arrows move 1 s, Shift/PageUp/PageDown 10 s,
 * Home/End jump to the limits. Clicking the track moves the nearer thumb.
 */
import { useRef } from "preact/hooks";
import { formatClock, nearerEnd, nudgeRange, setEnd, type Range } from "../lib/clip";

export interface RangeSliderProps {
  total: number;
  value: Range;
  active: "start" | "end";
  onActive: (which: "start" | "end") => void;
  onChange: (r: Range, which: "start" | "end") => void;
  /** Where the backend will really start (previous keyframe), if known. */
  alignedStart?: number;
  disabled?: boolean;
  label: string;
}

export function RangeSlider(p: RangeSliderProps) {
  const track = useRef<HTMLDivElement>(null);
  const dragging = useRef<"start" | "end" | null>(null);
  const pct = (ms: number) => (p.total > 0 ? (ms / p.total) * 100 : 0);

  const posFromEvent = (clientX: number) => {
    const r = track.current!.getBoundingClientRect();
    const f = Math.min(1, Math.max(0, (clientX - r.left) / Math.max(1, r.width)));
    return Math.round(f * p.total);
  };

  const moveTo = (which: "start" | "end", pos: number) => {
    const next = setEnd(p.total, p.value, which, pos);
    if (next) p.onChange(next, which);
  };

  const onPointerDown = (e: PointerEvent) => {
    if (p.disabled || e.button !== 0) return;
    const pos = posFromEvent(e.clientX);
    const thumb = (e.target as HTMLElement).closest<HTMLElement>("[data-thumb]")?.dataset.thumb as "start" | "end" | undefined;
    const which = thumb ?? nearerEnd(p.value, pos);
    dragging.current = which;
    p.onActive(which);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    if (!thumb) moveTo(which, pos);
    track.current?.querySelector<HTMLElement>(`[data-thumb="${which}"]`)?.focus({ preventScroll: true });
    e.preventDefault();
  };

  const onPointerMove = (e: PointerEvent) => {
    if (!dragging.current) return;
    moveTo(dragging.current, posFromEvent(e.clientX));
  };

  const onPointerUp = () => {
    dragging.current = null;
  };

  const onKey = (which: "start" | "end") => (e: KeyboardEvent) => {
    if (p.disabled) return;
    const big = e.shiftKey ? 10_000 : 1000;
    let next: Range | null = null;
    switch (e.key) {
      case "ArrowLeft": case "ArrowDown": next = nudgeRange(p.total, p.value, which, -big); break;
      case "ArrowRight": case "ArrowUp": next = nudgeRange(p.total, p.value, which, big); break;
      case "PageDown": next = nudgeRange(p.total, p.value, which, -10_000); break;
      case "PageUp": next = nudgeRange(p.total, p.value, which, 10_000); break;
      case "Home": next = setEnd(p.total, p.value, which, 0); break;
      case "End": next = setEnd(p.total, p.value, which, p.total); break;
      default: return;
    }
    e.preventDefault();
    p.onActive(which);
    if (next) p.onChange(next, which);
  };

  const thumb = (which: "start" | "end") => {
    const v = which === "start" ? p.value.start : p.value.end;
    return (
      <span
        role="slider"
        tabIndex={p.disabled ? -1 : 0}
        data-thumb={which}
        class={`rs-thumb${p.active === which ? " on" : ""}`}
        style={{ left: `${pct(v)}%` }}
        aria-label={which === "start" ? tx("{0}起点", p.label) : tx("{0}终点", p.label)}
        aria-valuemin={which === "start" ? 0 : p.value.start + 1000}
        aria-valuemax={which === "start" ? p.value.end - 1000 : p.total}
        aria-valuenow={v}
        aria-valuetext={formatClock(v)}
        aria-disabled={p.disabled ? "true" : undefined}
        onFocus={() => p.onActive(which)}
        onKeyDown={(e) => onKey(which)(e as unknown as KeyboardEvent)}
      />
    );
  };

  return (
    <div
      class={`range-slider${p.disabled ? " disabled" : ""}`}
      ref={track}
      onPointerDown={(e) => onPointerDown(e as unknown as PointerEvent)}
      onPointerMove={(e) => onPointerMove(e as unknown as PointerEvent)}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <span class="rs-rail" />
      <span class="rs-fill" style={{ left: `${pct(p.value.start)}%`, width: `${pct(p.value.end - p.value.start)}%` }} />
      {p.alignedStart !== undefined && p.alignedStart < p.value.start ? (
        <span class="rs-align" style={{ left: `${pct(p.alignedStart)}%`, width: `${pct(p.value.start - p.alignedStart)}%` }} title={tx("实际从 {0} 开始", formatClock(p.alignedStart, true))} />
      ) : null}
      {thumb("start")}
      {thumb("end")}
    </div>
  );
}
