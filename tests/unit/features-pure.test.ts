import { describe, expect, it } from "vitest";
import { clipLabel, estimateBytes, formatClock, fullRange, MIN_CLIP_MS, nearerEnd, normalizeRange, nudgeRange, parseClock, setEnd } from "../../src/lib/clip";
import { DEFAULT_RULES, planRename, renameOne, type RenameRules } from "../../src/lib/rename";
import { squarify } from "../../src/lib/treemap";
import { score } from "../../src/lib/fuzzy";

describe("clip range math", () => {
  it("formats and parses clock times", () => {
    expect(formatClock(0)).toBe("00:00");
    expect(formatClock(364_000)).toBe("06:04");
    expect(formatClock(3_723_400, true)).toBe("1:02:03.4");
    expect(parseClock("1:02:03")).toBe(3_723_000);
    expect(parseClock("06:04")).toBe(364_000);
    expect(parseClock("75")).toBe(75_000);
    expect(parseClock("01:02.5")).toBe(62_500);
    expect(parseClock("1:75")).toBeNull();
    expect(parseClock("abc")).toBeNull();
    expect(parseClock("6：04")).toBe(364_000); // full-width colon
  });

  it("makes Windows-safe labels", () => {
    expect(clipLabel(364_000, 1_019_000)).toBe("06.04-16.59");
    expect(clipLabel(0, 3_723_000)).not.toMatch(/[:]/);
  });

  it("keeps the range inside the video with a minimum length", () => {
    expect(normalizeRange(500, 0, 500)).toBeNull();
    expect(normalizeRange(10_000, -5, 99_999)).toEqual({ start: 0, end: 10_000 });
    const r = normalizeRange(10_000, 9_800, 9_900)!;
    expect(r.end - r.start).toBe(MIN_CLIP_MS);
    expect(r.end).toBeLessThanOrEqual(10_000);
    expect(fullRange(90_000)).toEqual({ start: 0, end: 90_000 });
  });

  it("nudges one end without pushing the other", () => {
    const total = 60_000;
    let r = { start: 10_000, end: 11_000 };
    r = nudgeRange(total, r, "start", 1000)!; // would collide → stays
    expect(r).toEqual({ start: 10_000, end: 11_000 });
    r = nudgeRange(total, r, "end", 5000)!;
    expect(r).toEqual({ start: 10_000, end: 16_000 });
    r = nudgeRange(total, r, "start", -20_000)!;
    expect(r.start).toBe(0);
    r = setEnd(total, r, "end", 99_000)!;
    expect(r.end).toBe(total);
    r = setEnd(total, r, "start", total)!; // ends never swap
    expect(r.start).toBe(total - MIN_CLIP_MS);
  });

  it("picks the nearer thumb and estimates bytes with BigInt", () => {
    expect(nearerEnd({ start: 10, end: 100 }, 20)).toBe("start");
    expect(nearerEnd({ start: 10, end: 100 }, 90)).toBe("end");
    expect(estimateBytes("9007199254740993", 1000, { start: 0, end: 500 })).toBe("4503599627370496");
    expect(estimateBytes(undefined, 1000, { start: 0, end: 500 })).toBeUndefined();
    expect(estimateBytes("100", 1000, { start: 0, end: 5000 })).toBe("100"); // never above the file
  });
});

describe("batch rename", () => {
  const items = [
    { id: "a", name: "Lecture 1.mp4", kind: "file" as const },
    { id: "b", name: "Lecture 2.mp4", kind: "file" as const },
    { id: "c", name: "notes", kind: "folder" as const },
  ];
  const rules = (r: Partial<RenameRules>): RenameRules => ({ ...DEFAULT_RULES, ...r });

  it("applies find/replace, numbering, prefix/suffix and case, keeping the extension", () => {
    expect(renameOne(items[0], 0, rules({ find: "Lecture", replace: "第" }), /Lecture/giu)).toBe("第 1.mp4");
    expect(renameOne(items[0], 4, rules({ numbering: "prefix", start: 1, digits: 3, separator: "-" }), null)).toBe("005-Lecture 1.mp4");
    expect(renameOne(items[0], 0, rules({ prefix: "[A] ", suffix: " end" }), null)).toBe("[A] Lecture 1 end.mp4");
    expect(renameOne(items[0], 0, rules({ case: "upper" }), null)).toBe("LECTURE 1.mp4");
    expect(renameOne(items[2], 0, rules({ case: "title" }), null)).toBe("Notes");
  });

  it("marks unchanged rows and counts changes", () => {
    const plan = planRename(items, items.map((i) => i.name), rules({ find: "Lecture", replace: "L" }));
    expect(plan.changed).toBe(2);
    expect(plan.rows.find((r) => r.id === "c")!.status).toBe("unchanged");
    expect(plan.errors).toBe(0);
  });

  it("detects duplicate targets, clashes with untouched siblings and swaps", () => {
    const dup = planRename(items.slice(0, 2), ["Lecture 1.mp4", "Lecture 2.mp4"], rules({ find: "\\d", replace: "", regex: true }));
    expect(dup.errors).toBe(2);
    expect(dup.rows[0].error).toContain("新名称相同");

    const clash = planRename([items[0]], ["Lecture 1.mp4", "Intro 1.mp4"], rules({ find: "Lecture", replace: "Intro" }));
    expect(clash.rows[0].error).toContain("已有同名");

    const swap = planRename(items.slice(0, 2), ["Lecture 1.mp4", "Lecture 2.mp4"], rules({ find: "(\\d)", replace: "x$1", regex: true, numbering: "off" }));
    expect(swap.errors).toBe(0);
  });

  it("reports invalid regular expressions instead of throwing", () => {
    const plan = planRename(items, [], rules({ find: "(", regex: true }));
    expect(plan.ruleError).toMatch(/正则表达式无效/);
    expect(plan.changed).toBe(0);
  });

  it("rejects names Windows or PikPak cannot hold", () => {
    const plan = planRename([items[0]], [], rules({ find: "Lecture", replace: "a/b" }));
    expect(plan.rows[0].status).toBe("error");
  });
});

describe("squarified treemap", () => {
  it("tiles the box exactly, largest first, skipping zeros", () => {
    const values = [500, 300, 120, 50, 20, 10, 0];
    const tiles = squarify(values, (v) => v, { x: 0, y: 0, w: 600, h: 400 });
    expect(tiles).toHaveLength(6);
    const area = tiles.reduce((s, t) => s + t.w * t.h, 0);
    expect(area).toBeCloseTo(600 * 400, 3);
    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(-1e-6);
      expect(t.y).toBeGreaterThanOrEqual(-1e-6);
      expect(t.x + t.w).toBeLessThanOrEqual(600 + 1e-6);
      expect(t.y + t.h).toBeLessThanOrEqual(400 + 1e-6);
      // area proportional to value
      expect((t.w * t.h) / (600 * 400)).toBeCloseTo(t.value / 1000, 3);
    }
    expect(tiles[0].value).toBe(500);
  });

  it("keeps aspect ratios reasonable", () => {
    const tiles = squarify(Array.from({ length: 30 }, (_, i) => 30 - i), (v) => v, { x: 0, y: 0, w: 800, h: 500 });
    const worst = Math.max(...tiles.map((t) => Math.max(t.w / t.h, t.h / t.w)));
    expect(worst).toBeLessThan(6);
  });

  it("handles empty input and empty boxes", () => {
    expect(squarify([], (v: number) => v, { x: 0, y: 0, w: 10, h: 10 })).toEqual([]);
    expect(squarify([1], (v) => v, { x: 0, y: 0, w: 0, h: 10 })).toEqual([]);
  });
});

describe("palette fuzzy score", () => {
  it("prefers substrings and word starts, rejects non-matches", () => {
    expect(score("在当前文件夹查找重复", "重复")).toBeGreaterThan(0);
    expect(score("settings shezhi", "sz")).toBeGreaterThan(0);
    expect(score("网盘", "下载")).toBe(0);
    expect(score("下载所选", "下载")).toBeGreaterThan(score("批量下载所选", "下载"));
    expect(score("anything", "")).toBe(1);
  });
});
