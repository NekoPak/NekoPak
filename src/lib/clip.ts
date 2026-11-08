/**
 * Pure helpers for choosing a time range of a video ("下载片段").
 * All values are integer milliseconds. No DOM, unit-tested.
 */

/** Shortest clip the UI allows, matching one nudge step. */
export const MIN_CLIP_MS = 1000;

/** "1:02:03", "06:04" or "00:09" — hours only when needed. */
export function formatClock(ms: number, withTenths = false): string {
  const neg = ms < 0;
  let t = Math.abs(Math.round(ms));
  const tenths = Math.floor((t % 1000) / 100);
  t = Math.floor(t / 1000);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  const core = h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
  return `${neg ? "−" : ""}${core}${withTenths ? `.${tenths}` : ""}`;
}

/** Windows-safe label for file names: "06.04-16.59" (no colons). */
export function clipLabel(start: number, end: number): string {
  return `${formatClock(start).replace(/:/g, ".")}-${formatClock(end).replace(/:/g, ".")}`;
}

/** Parse "1:02:03", "62:03", "75" (seconds) or "01:02.5"; null when invalid. */
export function parseClock(text: string): number | null {
  const s = text.trim().replace(/：/g, ":");
  if (!/^\d+(:\d{1,2}){0,2}(\.\d+)?$/.test(s)) return null;
  const [main, frac] = s.split(".");
  const parts = main.split(":").map(Number);
  if (parts.slice(1).some((p) => p >= 60)) return null;
  let sec = 0;
  for (const p of parts) sec = sec * 60 + p;
  const ms = sec * 1000 + (frac ? Math.round(Number(`0.${frac}`) * 1000) : 0);
  return Number.isFinite(ms) ? ms : null;
}

export interface Range {
  start: number;
  end: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Bring a range inside [0, total] with at least MIN_CLIP_MS between the
 * ends. Returns null when the video is shorter than the minimum clip.
 */
export function normalizeRange(total: number, start: number, end: number): Range | null {
  if (!(total >= MIN_CLIP_MS)) return null;
  let s = clamp(Math.round(start), 0, total - MIN_CLIP_MS);
  let e = clamp(Math.round(end), MIN_CLIP_MS, total);
  if (e - s < MIN_CLIP_MS) {
    // keep the start where the user put it when possible
    e = Math.min(total, s + MIN_CLIP_MS);
    s = e - MIN_CLIP_MS;
  }
  return { start: s, end: e };
}

/** Move one end by `delta` without pushing the other end. */
export function nudgeRange(total: number, r: Range, which: "start" | "end", delta: number): Range | null {
  if (which === "start") {
    const s = clamp(r.start + delta, 0, r.end - MIN_CLIP_MS);
    return normalizeRange(total, s, r.end);
  }
  const e = clamp(r.end + delta, r.start + MIN_CLIP_MS, total);
  return normalizeRange(total, r.start, e);
}

/** Move one end to an absolute position (dragging a thumb); the ends never swap. */
export function setEnd(total: number, r: Range, which: "start" | "end", pos: number): Range | null {
  return nudgeRange(total, r, which, Math.round(pos) - (which === "start" ? r.start : r.end));
}

/** Which thumb a click on the track at `pos` should move: the nearer one. */
export function nearerEnd(r: Range, pos: number): "start" | "end" {
  const ds = Math.abs(pos - r.start);
  const de = Math.abs(pos - r.end);
  if (ds === de) return pos < r.start ? "start" : "end";
  return ds < de ? "start" : "end";
}

/** Proportional byte estimate for a range of a tier of known size (BigInt-safe). */
export function estimateBytes(sizeBytes: string | undefined, total: number, r: Range): string | undefined {
  if (!sizeBytes || !/^\d+$/.test(sizeBytes) || total <= 0) return undefined;
  const size = BigInt(sizeBytes);
  const len = BigInt(Math.max(0, r.end - r.start));
  const est = (size * len) / BigInt(Math.round(total));
  return (est > size ? size : est).toString();
}

/** Default selection when opening: the whole video, both handles visible at the ends. */
export function fullRange(total: number): Range | null {
  return normalizeRange(total, 0, total);
}
