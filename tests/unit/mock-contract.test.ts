import { afterEach, describe, expect, it } from "vitest";
import { MockTransport } from "../../src/lib/mock/backend";
import { SPECIAL, type ScenarioId } from "../../src/lib/mock/fixtures";
import { ROOT_ID } from "../../src/lib/contract-proposals";
import type { ClientError } from "../../src/lib/contract";
import { sortEntries } from "../../src/components/FileTable";

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

describe("MockTransport honours the contract", () => {
  it("lists with cursor paging and stable ids", async () => {
    const m = mk();
    const p1 = await m.invoke("files_list", { parent_id: SPECIAL.huge, limit: 1000 });
    expect(p1.items).toHaveLength(1000);
    expect(p1.next_cursor).toBeTruthy();
    const p2 = await m.invoke("files_list", { parent_id: SPECIAL.huge, cursor: p1.next_cursor, limit: 1000 });
    expect(new Set([...p1.items, ...p2.items].map((e) => e.id)).size).toBe(2000);
    for (const e of p1.items) expect(e.size_bytes).toMatch(/^\d+$/);
  });

  it("files_mutate is idempotent per key", async () => {
    const m = mk();
    const r1 = await m.invoke("files_mutate", { op: "mkdir", ids: [], target: { parent_id: ROOT_ID, name: "幂等测试" }, idempotency_key: "k1" });
    const r2 = await m.invoke("files_mutate", { op: "mkdir", ids: [], target: { parent_id: ROOT_ID, name: "幂等测试" }, idempotency_key: "k1" });
    expect(r1.replayed).toBe(false);
    expect(r2.replayed).toBe(true);
    const list = await m.invoke("files_list", { parent_id: ROOT_ID, limit: 1000 });
    expect(list.items.filter((e) => e.name === "幂等测试")).toHaveLength(1);
  });

  it("cloud_task_create never duplicates on retry with the same key", async () => {
    const m = mk();
    const src = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test";
    const a = await m.invoke("cloud_task_create", { source_url: src, parent_id: ROOT_ID, idempotency_key: "same" });
    const b = await m.invoke("cloud_task_create", { source_url: src, parent_id: ROOT_ID, idempotency_key: "same" });
    expect(a.task_id).toBe(b.task_id);
    const tasks = await m.invoke("transfers_list", {});
    expect(tasks.items.filter((t) => t.id === a.task_id)).toHaveLength(1);
    expect((await err(m.invoke("cloud_task_create", { source_url: "ftp://x", parent_id: ROOT_ID, idempotency_key: "k2" }))).code).toBe("UNSUPPORTED");
  });

  it("returns snapshots by value", async () => {
    const m = mk();
    const s1 = await m.invoke("transfers_list", {});
    s1.items[0].state = "completed";
    const s2 = await m.invoke("transfers_list", {});
    expect(s2.items[0].state).not.toBe("completed");
  });

  it("maps scenarios to the right error codes", async () => {
    expect((await err(mk("network_down").invoke("files_list", { parent_id: ROOT_ID }))).code).toBe("TRANSIENT_NETWORK");
    expect((await err(mk("network_down").invoke("files_list", { parent_id: ROOT_ID }))).retryable).toBe(true);
    expect((await err(mk("unauthorized").invoke("files_list", { parent_id: ROOT_ID }))).code).toBe("AUTH_EXPIRED");
    expect((await err(mk("quota_exhausted").invoke("transfer_start", { remote_id: "f-root-9", destination: "C:\\d" }))).code).toBe("QUOTA_EXCEEDED");
    expect((await err(mk().invoke("files_list", { parent_id: SPECIAL.vanished }))).code).toBe("NOT_FOUND");
    expect((await err(mk().invoke("mount_start", { config_id: "x", read_only: true }))).code).toBe("UNSUPPORTED");
  });

  it("read-only share rejects writes with PERMISSION_DENIED", async () => {
    const m = mk();
    const e = await err(m.invoke("files_mutate", { op: "mkdir", ids: [], target: { parent_id: SPECIAL.readOnlyShare, name: "x" }, idempotency_key: "ro" }));
    expect(e.code).toBe("PERMISSION_DENIED");
  });

  it("download destinations collide case-insensitively (Windows)", async () => {
    const m = mk();
    const list = await m.invoke("files_list", { parent_id: ROOT_ID, limit: 1000 });
    const upper = list.items.find((e) => e.name === "Report.pdf")!;
    const lower = list.items.find((e) => e.name === "report.pdf")!;
    await m.invoke("transfer_start", { remote_id: upper.id, destination: "C:\\Downloads" });
    expect((await err(m.invoke("transfer_start", { remote_id: lower.id, destination: "C:\\Downloads" }))).code).toBe("CONFLICT");
  });

  it("escapes Windows reserved names in download destinations", async () => {
    const m = mk();
    const list = await m.invoke("files_list", { parent_id: ROOT_ID, limit: 1000 });
    const con = list.items.find((e) => e.name === "CON")!;
    const ref = await m.invoke("transfer_start", { remote_id: con.id, destination: "C:\\Downloads\\" });
    const t = (await m.invoke("transfers_list", {})).items.find((x) => x.id === ref.task_id)!;
    expect(t.destination).toBe("C:\\Downloads\\_CON");
  });

  it("sync apply requires confirm_deletes for destructive plans and blocks conflicts", async () => {
    const m = mk();
    await m.invoke("sync_scan", { config_id: "cfg-movies" });
    await new Promise((r) => setTimeout(r, 2400));
    const plan = await m.invoke("sync_plan", { config_id: "cfg-movies" });
    expect(plan.has_deletes).toBe(true);
    expect(plan.actions.filter((a) => a.destructive).every((a) => a.op === "delete")).toBe(true);
    expect((await err(m.invoke("sync_apply", { plan_id: plan.plan_id, confirm_deletes: false }))).code).toBe("CONFLICT");
    const job = await m.invoke("sync_apply", { plan_id: plan.plan_id, confirm_deletes: true });
    expect(job.task_id).toMatch(/^job-/);

    await m.invoke("sync_scan", { config_id: "cfg-work" });
    await new Promise((r) => setTimeout(r, 2400));
    const w = await m.invoke("sync_plan", { config_id: "cfg-work" });
    expect(w.has_unresolved_conflicts).toBe(true);
    expect(w.has_deletes).toBe(false);
    expect((await err(m.invoke("sync_apply", { plan_id: w.plan_id, confirm_deletes: false }))).code).toBe("CONFLICT");
  }, 10_000);

  it("first sync never contains deletes; bidirectional is refused", async () => {
    const m = mk();
    await m.invoke("sync_scan", { config_id: "cfg-courses" });
    await new Promise((r) => setTimeout(r, 2400));
    const p = await m.invoke("sync_plan", { config_id: "cfg-courses" });
    expect(p.has_deletes).toBe(false);
    const cfgs = await m.invoke("sync_configs_list", undefined);
    const c = cfgs.find((x) => x.config_id === "cfg-courses")!;
    expect((await err(m.invoke("sync_config_save", { config: { ...c, mode: "bidirectional" } }))).code).toBe("UNSUPPORTED");
    expect((await err(m.invoke("sync_config_save", { config: { ...c, allow_deletes: true } }))).code).toBe("CONFLICT");
  }, 10_000);

  it("emits progress events with monotonically increasing revisions", async () => {
    const m = mk();
    const seen: bigint[] = [];
    await m.listen("nekopak://transfer/progress", (ev) => {
      if (ev.task_id === "task-1" && ev.revision !== undefined) seen.push(BigInt(ev.revision));
    });
    await new Promise((r) => setTimeout(r, 1300));
    expect(seen.length).toBeGreaterThan(2);
    for (let i = 1; i < seen.length; i++) expect(seen[i]).toBeGreaterThan(seen[i - 1]);
  });

  it("unknown command rejects the way Tauri does", async () => {
    const m = mk();
    await expect(m.invoke("nope" as never, undefined as never)).rejects.toBe("command nope not found");
  });

  it("settings never return a proxy secret and refuse unaudited DNS", async () => {
    const m = mk();
    const s = await m.invoke("settings_get", undefined);
    expect(s.experimental_network.enabled).toBe(false);
    expect(JSON.stringify(s)).not.toMatch(/proxy_url|password/);
    expect((await err(m.invoke("settings_update", { patch: { experimental_network: { custom_dns_mode: true as never } } }))).code).toBe("UNSUPPORTED");
  });
});

describe("sortEntries", () => {
  it("sorts sizes beyond 2^53 exactly and keeps folders first", () => {
    const e = (id: string, kind: "file" | "folder", size: string) => ({ id, name: id, kind, size_bytes: size });
    const out = sortEntries([e("a", "file", "9007199254740993"), e("b", "file", "9007199254740992"), e("d", "folder", "0")], { key: "size", dir: "asc" });
    expect(out.map((x) => x.id)).toEqual(["d", "b", "a"]);
  });
});
