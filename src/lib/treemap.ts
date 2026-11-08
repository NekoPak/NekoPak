/**
 * Squarified treemap layout (Bruls, Huizing & van Wijk, 2000).
 * Input values are non-negative numbers; output rectangles tile the box.
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile<T> extends Rect {
  item: T;
  value: number;
}

function worst(row: number[], side: number, scale: number): number {
  let max = 0;
  let min = Infinity;
  let sum = 0;
  for (const v of row) {
    const a = v * scale;
    sum += a;
    if (a > max) max = a;
    if (a < min) min = a;
  }
  if (sum === 0 || min === 0) return Infinity;
  const s2 = side * side;
  return Math.max((s2 * max) / (sum * sum), (sum * sum) / (s2 * min));
}

/**
 * Lay out `items` (largest first works best; the function sorts a copy)
 * inside `box`. Items with value 0 are omitted.
 */
export function squarify<T>(items: readonly T[], valueOf: (t: T) => number, box: Rect): Tile<T>[] {
  const list = items
    .map((item) => ({ item, value: Math.max(0, valueOf(item)) }))
    .filter((x) => x.value > 0)
    .sort((a, b) => b.value - a.value);
  const total = list.reduce((s, x) => s + x.value, 0);
  const out: Tile<T>[] = [];
  if (!list.length || box.w <= 0 || box.h <= 0 || total <= 0) return out;

  const scale = (box.w * box.h) / total;
  let free: Rect = { ...box };
  let i = 0;
  while (i < list.length) {
    const side = Math.min(free.w, free.h);
    const row: number[] = [list[i].value];
    let j = i + 1;
    while (j < list.length) {
      const next = [...row, list[j].value];
      if (worst(next, side, scale) > worst(row, side, scale)) break;
      row.push(list[j].value);
      j++;
    }
    const rowArea = row.reduce((s, v) => s + v * scale, 0);
    const horizontal = free.w >= free.h; // lay the row along the short side
    const thick = rowArea / side;
    let offset = 0;
    for (let k = i; k < j; k++) {
      const len = (list[k].value * scale) / thick;
      out.push(
        horizontal
          ? { item: list[k].item, value: list[k].value, x: free.x, y: free.y + offset, w: thick, h: len }
          : { item: list[k].item, value: list[k].value, x: free.x + offset, y: free.y, w: len, h: thick },
      );
      offset += len;
    }
    free = horizontal
      ? { x: free.x + thick, y: free.y, w: Math.max(0, free.w - thick), h: free.h }
      : { x: free.x, y: free.y + thick, w: free.w, h: Math.max(0, free.h - thick) };
    i = j;
  }
  return out;
}
