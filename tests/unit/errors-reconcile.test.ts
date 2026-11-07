import { describe, expect, it } from "vitest";
import { diagnosticsText, redact, toClientError } from "../../src/lib/errors";
import { applyChanged, applyProgress, compareOrder, mergeSnapshot } from "../../src/state/reconcile";
import type { TransferTaskExt } from "../../src/lib/contract-proposals";
import { computeRange, scrollIntoViewOffset } from "../../src/lib/virtual";
import { parseHash, routeHash } from "../../src/state/route";

describe("redaction (06_SECURITY §1)", () => {
  it("strips signed URL queries, bearer tokens, key=value secrets and JWTs", () => {
    const s = redact("GET https://dl.example.com/f/abc.mp4?sig=XYZ&expires=1 failed; Authorization: Bearer abcdefghijklmnop; refresh_token=r123; jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c");
    expect(s).not.toContain("XYZ");
    expect(s).not.toContain("abcdefghijklmnop");
    expect(s).not.toContain("r123");
    expect(s).not.toContain("SflKxw");
    expect(s).toContain("https://dl.example.com/f/abc.mp4?<已隐藏>");
  });

  it("normalises unknown Tauri commands into UNSUPPORTED", () => {
    const e = toClientError("command files_search not found");
    expect(e.code).toBe("UNSUPPORTED");
    expect(e.details).toEqual({ command: "files_search" });
  });

  it("re-redacts ClientError from the backend and its details", () => {
    const e = toClientError({ code: "INTERNAL", message: "boom https://x.io/a?token=1", retryable: false, details: { access_token: "t", url: "https://x.io/b?sig=2" } });
    expect(e.message).not.toContain("token=1");
    expect(e.details).toEqual({ access_token: "<已隐藏>", url: "https://x.io/b?<已隐藏>" });
    expect(diagnosticsText(e)).not.toMatch(/sig=2|token=1/);
  });

  it("wraps arbitrary throwables as INTERNAL without Debug dumps", () => {
    expect(toClientError(new Error("x")).code).toBe("INTERNAL");
    expect(toClientError(42).message).toBe("未知错误");
  });
});

const base: TransferTaskExt = {
  id: "t", remote_id: "r", kind: "download", state: "running", transferred_bytes: "100", total_bytes: "1000",
  backend: "mock", created_at: "2026-11-07T00:00:00Z", updated_at: "2026-11-07T00:00:10Z", revision: 5,
};

describe("event reconciliation (03_CONTRACTS §4)", () => {
  it("drops older and duplicate revisions", () => {
    expect(applyProgress(base, { task_id: "t", transferred_bytes: "90", state: "running", revision: 4 })).toBeNull();
    expect(applyProgress(base, { task_id: "t", transferred_bytes: "100", state: "running", revision: 5 })).toBeNull();
    const next = applyProgress(base, { task_id: "t", transferred_bytes: "200", state: "running", revision: 6, speed_bytes_per_sec: 10 });
    expect(next?.transferred_bytes).toBe("200");
    expect(next?.revision).toBe(6);
  });

  it("falls back to updated_at when revision is missing", () => {
    const t = { ...base, revision: undefined };
    expect(compareOrder(t, { updated_at: "2026-11-07T00:00:09Z" })).toBe(-1);
    expect(applyChanged(t, { task_id: "t", state: "paused", updated_at: "2026-11-07T00:00:11Z" })?.state).toBe("paused");
  });

  it("without ordering info: ignores backwards progress and never revives terminal tasks", () => {
    const t = { ...base, revision: undefined, updated_at: "" };
    expect(applyProgress(t, { task_id: "t", transferred_bytes: "50", state: "running" })).toBeNull();
    expect(applyProgress({ ...t, state: "completed" }, { task_id: "t", transferred_bytes: "999", state: "running" })).toBeNull();
    expect(applyProgress(t, { task_id: "t", transferred_bytes: "150", state: "running" })?.transferred_bytes).toBe("150");
  });

  it("changed events clear speed and carry errors", () => {
    const n = applyChanged({ ...base, speed_bytes_per_sec: 9 }, { task_id: "t", state: "failed", revision: 7, error: { code: "DISK_FULL", message: "x", retryable: true } });
    expect(n?.speed_bytes_per_sec).toBeUndefined();
    expect(n?.error?.code).toBe("DISK_FULL");
  });

  it("snapshot wins unless the local row is provably newer", () => {
    expect(mergeSnapshot({ ...base, revision: 9 }, { ...base, revision: 7 }).revision).toBe(9);
    expect(mergeSnapshot({ ...base, revision: 7 }, { ...base, revision: 9 }).revision).toBe(9);
    expect(mergeSnapshot(undefined, base)).toBe(base);
  });
});

describe("virtual list math", () => {
  it("renders only the visible window plus overscan", () => {
    const r = computeRange(36 * 5000, 600, 36, 12000, 8);
    expect(r.start).toBe(4992);
    expect(r.end - r.start).toBeLessThan(50);
    expect(r.total).toBe(36 * 12000);
    expect(computeRange(0, 600, 36, 0).end).toBe(0);
  });

  it("scrolls the focused row into view below a sticky header", () => {
    expect(scrollIntoViewOffset(0, 360, 600, 36, 32)).toBe(0);
    expect(scrollIntoViewOffset(30, 0, 600, 36, 32)).toBe(31 * 36 - (600 - 32));
    expect(scrollIntoViewOffset(5, 0, 600, 36, 32)).toBeNull();
  });
});

describe("routes", () => {
  it("round-trips hashes including unicode ids", () => {
    for (const h of ["#/files", "#/files/d-%E4%B8%AD", "#/transfers/failed", "#/settings/quota", "#/search/%E7%89%87%E6%AE%B5", "#/sync/cfg-1", "#/mount"]) {
      expect(routeHash(parseHash(h))).toBe(h);
    }
    expect(parseHash("#/nope")).toEqual({ page: "files", folder: "root" });
  });
});
