import { afterEach, describe, expect, it } from "vitest";
import { MockTransport } from "../../src/lib/mock/backend";
import { SPECIAL, type ScenarioId } from "../../src/lib/mock/fixtures";
import { PROPOSED_EVENTS, ROOT_ID, type ServerJob } from "../../src/lib/contract-proposals";
import type { ClientError, RemoteEntry } from "../../src/lib/contract";

const live: MockTransport[] = [];
function mk(s: ScenarioId = "free") {
  const m = new MockTransport(s, { latency_ms: 0 });
  live.push(m);
  return m;
}
afterEach(() => {
  while (live.length) live.pop()!.dispose();
});

async function err(p: Promise<unknown>): Promise<ClientError> {
  try {
    await p;
  } catch (e) {
    return e as ClientError;
  }
  throw new Error("expected rejection");
}

async function find(m: MockTransport, parent: string, name: string): Promise<RemoteEntry> {
  const list = await m.invoke("files_list", { parent_id: parent, limit: 500 });
  const e = list.items.find((x) => x.name === name);
  if (!e) throw new Error(`missing ${name}`);
  return e;
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("sign-in (§P23)", () => {
  it("signs in with a password, reports wrong passwords and asks for codes", async () => {
    const m = mk("unauthorized");
    expect((await m.invoke("auth_status", undefined)).state).toBe("signed_out");
    const bad = await err(m.invoke("auth_sign_in", { username: "k@example.com", password: "wrong" }));
    expect(bad.code).toBe("PERMISSION_DENIED");
    expect(bad.details?.reason).toBe("invalid_credentials");

    const ch = await m.invoke("auth_sign_in", { username: "+86 138 0000 1234", password: "x" });
    expect(ch.state).toBe("challenge");
    expect(ch.challenge?.kind).toBe("verify_code");
    expect((await err(m.invoke("auth_verify", { challenge_id: ch.challenge!.challenge_id, code: "000000" }))).code).toBe("PERMISSION_DENIED");
    expect((await m.invoke("auth_verify", { challenge_id: ch.challenge!.challenge_id, code: "123456" })).state).toBe("signed_in");
    expect((await m.invoke("auth_status", undefined)).state).toBe("signed_in");
  });

  it("models PikPak's human check as a captcha challenge", async () => {
    const m = mk("unauthorized");
    const ch = await m.invoke("auth_sign_in", { username: "k@example.com", password: "captcha" });
    expect(ch.challenge?.kind).toBe("captcha");
    expect((await m.invoke("auth_verify", { challenge_id: ch.challenge!.challenge_id, code: "" })).state).toBe("signed_in");
  });
});

describe("media + clip download (§P14)", () => {
  it("probes tiers, aligns to a previous keyframe and queues a clip task", async () => {
    const m = mk();
    const v = await find(m, ROOT_ID, "片段.mp4");
    const probe = await m.invoke("media_probe", { remote_id: v.id });
    expect(probe.duration_ms).toBeGreaterThan(60_000);
    expect(probe.qualities[0].id).toBe("original");
    expect(probe.keyframes).toBe("indexed");

    const a = await m.invoke("media_align", { remote_id: v.id, quality_id: "original", start_ms: 364_321, end_ms: 1_019_000 });
    expect(a.start_ms).toBeLessThanOrEqual(364_321);
    expect(364_321 - a.start_ms).toBeLessThan(2_700);
    expect(a.exactness).toBe("exact");
    expect(BigInt(a.estimated_bytes!)).toBeLessThan(BigInt(v.size_bytes));

    const ref = await m.invoke("transfer_start", { remote_id: v.id, destination: "C:\\Down", options: { quality_id: "720p", clip: { start_ms: 364_321, end_ms: 1_019_000, align: "previous_keyframe" } } });
    const tasks = await m.invoke("transfers_list", {});
    const t = tasks.items.find((x) => x.id === ref.task_id)!;
    expect(t.clip?.start_ms).toBe(a.start_ms);
    expect(t.quality_label).toBe("720P");
    expect(t.name).toMatch(/\[720P\] \[06\.0\d-16\.59\]\.mp4$/);
    expect(t.name).not.toMatch(/:/);
  });

  it("refuses unreadable tiers, bad ranges and non-videos", async () => {
    const m = mk();
    const k4 = await find(m, SPECIAL.movies, "4K 演示 — HDR10 (2160p).mkv");
    const probe = await m.invoke("media_probe", { remote_id: k4.id });
    const blocked = probe.qualities.find((q) => q.id === "1080p")!;
    expect(blocked.readable).toBe(false);
    expect((await err(m.invoke("media_align", { remote_id: k4.id, quality_id: "1080p", start_ms: 0, end_ms: 5000 }))).code).toBe("UNSUPPORTED");
    expect((await err(m.invoke("transfer_start", { remote_id: k4.id, destination: "C:\\D", options: { clip: { start_ms: 5000, end_ms: 5200, align: "previous_keyframe" } } }))).code).toBe("CONFLICT");
    const doc = await find(m, ROOT_ID, "空文件.txt");
    expect((await err(m.invoke("media_probe", { remote_id: doc.id }))).code).toBe("UNSUPPORTED");
  });

  it("returns synthetic preview frames in the demo", async () => {
    const m = mk();
    const v = await find(m, ROOT_ID, "片段.mp4");
    const f = await m.invoke("media_preview_frame", { remote_id: v.id, position_ms: 61_000, max_width: 320 });
    expect(f.synthetic).toBe(true);
    expect(f.mime).toBe("image/svg+xml");
    expect(f.data_base64.length).toBeGreaterThan(100);
  });
});

describe("folder download (§P15)", () => {
  it("expands folders into tasks with relative destinations, skips oversized folders, is idempotent", async () => {
    const m = mk();
    const r = await m.invoke("transfer_start_batch", { remote_ids: [SPECIAL.courses, SPECIAL.huge], destination: "C:\\Down", idempotency_key: "b1" });
    expect(r.expanded_files).toBeGreaterThan(2);
    expect(r.skipped.some((s) => s.id === SPECIAL.huge && /1,000/.test(s.reason))).toBe(true);
    const again = await m.invoke("transfer_start_batch", { remote_ids: [SPECIAL.courses], destination: "C:\\Down", idempotency_key: "b1" });
    expect(again.task_ids).toEqual(r.task_ids);
    const tasks = (await m.invoke("transfers_list", {})).items.filter((t) => r.task_ids.includes(t.id));
    expect(tasks.every((t) => t.destination!.startsWith("C:\\Down\\") && t.group_id === r.group_id)).toBe(true);
  });

  it("applies the default quality cap to ordinary video downloads", async () => {
    const m = mk();
    const k4 = await find(m, SPECIAL.movies, "4K 演示 — HDR10 (2160p).mkv");
    const ref = await m.invoke("transfer_start", { remote_id: k4.id, destination: "C:\\D", options: { quality_cap: "1080p" } });
    const t = (await m.invoke("transfers_list", {})).items.find((x) => x.id === ref.task_id)!;
    // 1080P is unreadable for this file, so the best readable tier under the cap is 720P.
    expect(t.quality_label).toBe("720P");
  });
});

describe("star, duplicates, space (§P16–§P18)", () => {
  it("stars and unstars, including inside read-only shares", async () => {
    const m = mk();
    const before = (await m.invoke("files_starred", {})).items.length;
    const doc = await find(m, ROOT_ID, "空文件.txt");
    await m.invoke("files_mutate", { op: "star", ids: [doc.id], idempotency_key: "s1" });
    const after = await m.invoke("files_starred", {});
    expect(after.items.length).toBe(before + 1);
    expect((await m.invoke("files_stat", { id: doc.id }) as RemoteEntry & { starred?: boolean }).starred).toBe(true);
    await m.invoke("files_mutate", { op: "unstar", ids: [doc.id], idempotency_key: "s2" });
    expect((await m.invoke("files_starred", {})).items.length).toBe(before);
  });

  it("finds byte-identical files and drops trashed copies from the report", async () => {
    const m = mk();
    const events: string[] = [];
    m.listen(PROPOSED_EVENTS.duplicatesProgress, (e) => events.push(e.state));
    const { scan_id } = await m.invoke("duplicates_scan", { folder_id: ROOT_ID });
    expect((await m.invoke("duplicates_report", { scan_id })).state).toBe("running");
    await wait(1800);
    expect(events[events.length - 1]).toBe("completed");
    const r = await m.invoke("duplicates_report", { scan_id });
    expect(r.groups.length).toBeGreaterThanOrEqual(5);
    expect(r.unhashed_files).toBeGreaterThan(10_000);
    for (const g of r.groups) {
      expect(g.entries.length).toBeGreaterThan(1);
      expect(new Set(g.entries.map((e) => e.size_bytes)).size).toBe(1);
    }
    const g0 = r.groups[0];
    await m.invoke("files_mutate", { op: "trash", ids: g0.entries.slice(1).map((e) => e.id), idempotency_key: "t1" });
    const r2 = await m.invoke("duplicates_report", { scan_id });
    expect(r2.groups.find((g) => g.group_id === g0.group_id)).toBeUndefined();
    expect(BigInt(r2.reclaimable_bytes)).toBeLessThan(BigInt(r.reclaimable_bytes));
  });

  it("sums folder sizes recursively and flags unreadable folders", async () => {
    const m = mk();
    const s = await m.invoke("files_space_summary", { folder_id: ROOT_ID });
    const sum = s.children.reduce((acc, c) => acc + BigInt(c.size_bytes), 0n);
    expect(sum.toString()).toBe(s.total_bytes);
    expect(s.exactness).toBe("partial");
    const docs = s.children.find((c) => c.id === SPECIAL.docs)!;
    const inner = await m.invoke("files_space_summary", { folder_id: SPECIAL.docs });
    expect(inner.total_bytes).toBe(docs.size_bytes);
  });
});

describe("archives + jobs (§P19)", () => {
  it("lists levels, needs a password for encrypted archives and extracts via a job", async () => {
    const m = mk();
    const zip = await find(m, SPECIAL.docs, "C 语言程序设计 习题集.zip");
    const top = await m.invoke("archive_list", { file_id: zip.id });
    expect(top.supported).toBe(true);
    expect(top.entries.map((e) => e.name)).toContain("第1章 基础");
    const ch1 = await m.invoke("archive_list", { file_id: zip.id, path: "第1章 基础" });
    expect(ch1.entries).toHaveLength(8);

    const big = await find(m, SPECIAL.projects, "超大压缩包.zip");
    const locked = await m.invoke("archive_list", { file_id: big.id });
    expect(locked.needs_password).toBe(true);
    expect((await err(m.invoke("archive_unlock", { file_id: big.id, password: "nope" }))).code).toBe("PERMISSION_DENIED");
    const { password_ref } = await m.invoke("archive_unlock", { file_id: big.id, password: "nekopak" });
    expect((await m.invoke("archive_list", { file_id: big.id, password_ref })).entries.length).toBeGreaterThan(0);

    const jobs: ServerJob[] = [];
    m.listen(PROPOSED_EVENTS.jobChanged, (j) => jobs.push(j));
    const { job_id } = await m.invoke("archive_extract", { file_id: zip.id, paths: "all", target_parent_id: SPECIAL.empty, idempotency_key: "x1" });
    expect((await m.invoke("archive_extract", { file_id: zip.id, paths: "all", target_parent_id: SPECIAL.empty, idempotency_key: "x1" })).job_id).toBe(job_id);
    await wait(2900);
    const mine = jobs.filter((j) => j.job_id === job_id);
    const last = mine[mine.length - 1];
    expect(last.state).toBe("completed");
    const revs = jobs.filter((j) => j.job_id === job_id).map((j) => j.revision);
    expect([...revs].sort((a, b) => BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0)).toEqual(revs);
    const out = await m.invoke("files_list", { parent_id: SPECIAL.empty, limit: 50 });
    expect(out.items.map((e) => e.name)).toContain("C 语言程序设计 习题集");
    expect((await m.invoke("jobs_list", {})).items.some((j) => j.job_id === job_id)).toBe(true);
  });

  it("refuses multi-volume archives with a reason", async () => {
    const m = mk();
    const rar = await find(m, SPECIAL.courses, "课程录像.part1.rar");
    const l = await m.invoke("archive_list", { file_id: rar.id });
    expect(l.supported).toBe(false);
    expect(l.reason).toMatch(/分卷/);
  });
});

describe("link preview + shares (§P20, §P22)", () => {
  it("previews a magnet and saves only the picked files", async () => {
    const m = mk();
    const src = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Frieren";
    const pv = await m.invoke("cloud_task_preview", { source_url: src });
    expect(pv.title).toBe("Frieren");
    expect(pv.files.length).toBeGreaterThan(20);
    const pick = pv.files.filter((f) => f.path.endsWith(".mkv")).map((f) => f.index);
    const ref = await m.invoke("cloud_task_create", { source_url: src, parent_id: ROOT_ID, idempotency_key: "c1", preview_id: pv.preview_id, file_indices: pick });
    const t = (await m.invoke("transfers_list", {})).items.find((x) => x.id === ref.task_id)!;
    expect(t.name).toContain(`已选 ${pick.length}/${pv.files.length}`);
    expect(t.total_bytes).toBeTruthy();
    expect((await err(m.invoke("cloud_task_create", { source_url: src, parent_id: ROOT_ID, idempotency_key: "c2", preview_id: pv.preview_id, file_indices: [] }))).code).toBe("CONFLICT");
  });

  it("creates, lists and cancels shares; refuses re-sharing read-only items", async () => {
    const m = mk();
    const v = await find(m, ROOT_ID, "片段.mp4");
    const s = await m.invoke("share_create", { ids: [v.id], expires_in_days: 7, with_code: true, idempotency_key: "sh1" });
    expect(s.url).toMatch(/^https:\/\/mock\.invalid\//);
    expect(s.code).toHaveLength(4);
    expect((await m.invoke("share_create", { ids: [v.id], expires_in_days: 7, with_code: true, idempotency_key: "sh1" })).share_id).toBe(s.share_id);
    expect((await m.invoke("shares_list", {})).items[0].share_id).toBe(s.share_id);
    const c = await m.invoke("shares_cancel", { share_ids: [s.share_id], idempotency_key: "sc1" });
    expect(c.succeeded).toEqual([s.share_id]);
    expect((await m.invoke("shares_list", {})).items.find((x) => x.share_id === s.share_id)!.state).toBe("cancelled");
    expect((await err(m.invoke("share_create", { ids: [SPECIAL.readOnlyShare], expires_in_days: null, with_code: false, idempotency_key: "sh2" }))).code).toBe("PERMISSION_DENIED");
  });
});

describe("settings additions", () => {
  it("validates download settings", async () => {
    const m = mk();
    const s = await m.invoke("settings_update", { patch: { download: { connections_per_task: 8, default_quality: "720p" } } });
    expect(s.download).toEqual({ connections_per_task: 8, default_quality: "720p" });
    expect((await err(m.invoke("settings_update", { patch: { download: { connections_per_task: 64 } } }))).code).toBe("CONFLICT");
  });
});
