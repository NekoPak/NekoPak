import { useCallback, useEffect, useLayoutEffect, useState } from "preact/hooks";
import type { RefObject } from "preact";

export interface Range {
  start: number;
  end: number; // exclusive
  padTop: number;
  total: number;
}

/** Pure range math for fixed-height rows (unit-tested). */
export function computeRange(scrollTop: number, viewport: number, rowHeight: number, count: number, overscan = 8): Range {
  const total = count * rowHeight;
  if (count === 0 || rowHeight <= 0) return { start: 0, end: 0, padTop: 0, total: 0 };
  const first = Math.floor(Math.max(0, scrollTop) / rowHeight);
  const visible = Math.ceil(Math.max(viewport, rowHeight) / rowHeight) + 1;
  const start = Math.max(0, first - overscan);
  const end = Math.min(count, first + visible + overscan);
  return { start, end, padTop: start * rowHeight, total };
}

/** Scroll offset that brings `index` fully into view, or null if already visible. */
export function scrollIntoViewOffset(index: number, scrollTop: number, viewport: number, rowHeight: number, headerOffset = 0): number | null {
  const top = index * rowHeight;
  const bottom = top + rowHeight;
  const viewTop = scrollTop;
  const viewBottom = scrollTop + viewport - headerOffset;
  if (top < viewTop) return top;
  if (bottom > viewBottom) return bottom - (viewport - headerOffset);
  return null;
}

export function useVirtualRows(ref: RefObject<HTMLElement>, count: number, rowHeight: number, overscan = 8) {
  const [state, setState] = useState({ scrollTop: 0, viewport: 600 });

  const measure = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    setState((s) =>
      s.scrollTop === el.scrollTop && s.viewport === el.clientHeight ? s : { scrollTop: el.scrollTop, viewport: el.clientHeight },
    );
  }, [ref]);

  useLayoutEffect(() => {
    measure();
  }, [measure, count]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let raf = 0;
    const onScroll = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        measure();
      });
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(measure) : null;
    ro?.observe(el);
    return () => {
      el.removeEventListener("scroll", onScroll);
      ro?.disconnect();
      if (raf) cancelAnimationFrame(raf);
    };
  }, [ref, measure]);

  const range = computeRange(state.scrollTop, state.viewport, rowHeight, count, overscan);

  const ensureVisible = useCallback(
    (index: number, headerOffset = 0) => {
      const el = ref.current;
      if (!el) return;
      const off = scrollIntoViewOffset(index, el.scrollTop, el.clientHeight, rowHeight, headerOffset);
      if (off !== null) el.scrollTop = off;
    },
    [ref, rowHeight],
  );

  return { range, ensureVisible, scrollTop: state.scrollTop, viewport: state.viewport };
}
