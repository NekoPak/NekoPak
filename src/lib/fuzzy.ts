/** Fuzzy matching for the command palette. Pure, unit-tested. */

/** Subsequence match with a bonus for word starts and contiguous runs; 0 = no match. */
export function score(text: string, q: string): number {
  if (!q) return 1;
  const t = text.toLowerCase();
  const s = q.toLowerCase().replace(/\s+/g, "");
  if (t.includes(s)) return 100 - t.indexOf(s);
  let ti = 0;
  let pts = 0;
  let run = 0;
  for (const ch of s) {
    const at = t.indexOf(ch, ti);
    if (at < 0) return 0;
    run = at === ti ? run + 1 : 0;
    pts += 1 + run * 2 + (at === 0 || /[\s/·_-]/.test(t[at - 1] ?? "") ? 3 : 0);
    ti = at + 1;
  }
  return pts;
}
