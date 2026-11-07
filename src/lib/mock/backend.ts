/**
 * MockTransport — a deterministic, offline stand-in for Astra's Rust
 * MockProvider. It answers every command in CommandMap with fixture data
 * and emits the §4 events from a 250 ms simulation tick.
 *
 * It never performs network I/O. When Astra's Rust mock is ready the UI
 * switches to TauriTransport without touching business code.
 */
import type {
  BackendId, CapabilityReport, ClientError, ClientErrorCode, Operation, Page,
  RemoteEntry, SyncAction, SyncPlan, TaskState,
} from "../contract";
import { EVENTS } from "../contract";
import type {
  AppStatus, AuthFlow, AuthState, CommandArgs, CommandName, CommandResult,
  MountCapabilities, MutateOp, OperationResult, QuotaItem, QuotaReport, Settings,
  SettingsPatch, SyncConfig, SyncEvent, TransferTaskExt,
} from "../contract-proposals";
import { PROPOSED_EVENTS, ROOT_ID } from "../contract-proposals";
import type { AllEvents, AnyEventName, Transport, Unlisten } from "../transport/types";
import {
  buildTasks, buildTrash, buildTree, DEFAULT_FAULTS, SPECIAL,
  type FaultFlags, type ScenarioId,
} from "./fixtures";

const TICK_MS = 250;
const GiB = 1024n ** 3n;

type Handler = (payload: unknown) => void;

interface TaskSim {
  task: TransferTaskExt;
  baseSpeed: number;
  phase: number;
  dropped37: boolean;
  retryTicks: number;
  attempts: number;
  verifyTicks: number;
  metaTicks: number;
  emitted: number;
  lastPayload?: AllEvents["nekopak://transfer/progress"];
}

interface TrashRecord {
  entry: RemoteEntry;
  original_parent: string;
}

function hashStr(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

const clone = <T>(v: T): T => (v === undefined ? v : (JSON.parse(JSON.stringify(v)) as T));

function escapeWindowsName(name: string): string {
  let n = name.replace(/[<>:"\\|?*\u0000-\u001f/]/g, "_").replace(/[ .]+$/g, (m) => "_".repeat(m.length));
  if (/^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i.test(n)) n = `_${n}`;
  return n.normalize("NFC");
}

export class MockTransport implements Transport {
  readonly kind = "mock" as const;

  scenario: ScenarioId;
  faults: FaultFlags;

  private entries = new Map<string, RemoteEntry>();
  private children = new Map<string, string[]>();
  private sortedCache = new Map<string, string[]>();
  private trash = new Map<string, TrashRecord>();
  private tasks = new Map<string, TaskSim>();
  private idem = new Map<string, unknown>();
  private auth!: AuthState;
  private flow: AuthFlow | null = null;
  private settings!: Settings;
  private syncConfigs = new Map<string, SyncConfig>();
  private plans = new Map<string, SyncPlan & { created_at: number; config_id: string }>();
  private history: SyncEvent[] = [];
  private listeners = new Map<string, Set<Handler>>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private tick = 0;
  private reqSeq = 0;
  private taskSeq = 100;
  private dailyUsed = 0n;
  private scans = new Map<string, { config_id: string; ticks: number; files: number }>();
  private jobs = new Map<string, { config_id: string; ticks: number; plan_id: string }>();

  constructor(scenario: ScenarioId = "free", faults: Partial<FaultFlags> = {}) {
    this.scenario = scenario;
    this.faults = { ...DEFAULT_FAULTS, ...faults };
    this.reset(scenario);
  }

  /* ---------------------------------------------------------------- */
  /* Mock-only controls (exposed to the dev panel, never to Tauri)     */
  /* ---------------------------------------------------------------- */

  reset(scenario: ScenarioId): void {
    this.scenario = scenario;
    this.entries.clear();
    this.children.clear();
    this.sortedCache.clear();
    this.trash.clear();
    this.tasks.clear();
    this.idem.clear();
    this.plans.clear();
    this.scans.clear();
    this.jobs.clear();
    this.history = [];
    this.flow = null;
    this.tick = 0;

    for (const e of buildTree(scenario)) this.insert(e);
    for (const e of buildTrash()) this.trash.set(e.id, { entry: e, original_parent: e.parent_id ?? ROOT_ID });

    const now = Date.now();
    for (const t of buildTasks(now)) this.tasks.set(t.id, this.sim(t));

    const tier = scenario === "premium" ? "premium" : "free";
    this.auth =
      scenario === "unauthorized"
        ? { state: "signed_out", checked_at: new Date().toISOString() }
        : {
            state: "signed_in",
            backend: "mock",
            account: { display_name: tier === "premium" ? "k***@example.com (Premium)" : "k***@example.com", tier },
            checked_at: new Date().toISOString(),
          };

    this.dailyUsed = scenario === "quota_exhausted" ? 5n * GiB : (12n * GiB) / 10n;

    this.settings = {
      schema_version: 1,
      theme: "system",
      download_dir: "%USERPROFILE%\\Downloads\\NekoPak",
      max_concurrent_transfers: 3,
      bandwidth_limit_kib_s: 0,
      cache: { dir: "%LOCALAPPDATA%\\NekoPak\\cache", max_bytes: String(10n * GiB) },
      experimental_network: {
        enabled: false,
        proxy_mode: "system",
        user_proxy_configured: false,
        custom_dns_mode: false,
        connection_diagnostics: true,
      },
      telemetry: false,
    };

    this.syncConfigs.clear();
    const cfgs: SyncConfig[] = [
      { config_id: "cfg-courses", name: "课程资料备份", mode: "upload_only", local_path: "D:\\Study\\课程资料", remote_id: SPECIAL.courses, remote_path: "/我的资料/2026 课程", exclude_globs: ["Thumbs.db", "~$*", "*.tmp"], allow_deletes: false, first_run: true },
      { config_id: "cfg-movies", name: "影视 云端 → 本地", mode: "download_only", local_path: "D:\\Media\\影视", remote_id: SPECIAL.movies, remote_path: "/影视", exclude_globs: [], allow_deletes: true, first_run: false, last_applied_at: new Date(now - 86_400_000 * 2).toISOString() },
      { config_id: "cfg-work", name: "工作区备份", mode: "upload_only", local_path: "D:\\Work", remote_id: SPECIAL.projects, remote_path: "/项目 Projects", exclude_globs: [".git/**", "node_modules/**", "target/**"], allow_deletes: false, first_run: false, last_applied_at: new Date(now - 86_400_000 * 6).toISOString() },
    ];
    for (const c of cfgs) this.syncConfigs.set(c.config_id, c);
    this.history = [
      { id: "h1", config_id: "cfg-movies", at: new Date(now - 86_400_000 * 2).toISOString(), kind: "apply", summary: "下载 3 项，删除 0 项" },
      { id: "h2", config_id: "cfg-work", at: new Date(now - 86_400_000 * 6).toISOString(), kind: "apply", summary: "上传 12 项" },
      { id: "h3", config_id: "cfg-work", at: new Date(now - 86_400_000 * 3).toISOString(), kind: "error", summary: "扫描失败，未生成任何操作", error: { code: "TRANSIENT_NETWORK", message: "连接超时", retryable: true, backend: "mock", request_id: "mock-req-77e0" } },
    ];

    this.ensureTimer();
    this.emit(EVENTS.backendStatus, {
      backend: "mock",
      status: this.mockStatus(),
      checked_at: new Date().toISOString(),
    });
    this.emit(PROPOSED_EVENTS.authChanged, { state: this.auth.state });
  }

  setFaults(patch: Partial<FaultFlags>): void {
    this.faults = { ...this.faults, ...patch };
  }

  /** Resolve the pending mock authorization (replaces the external browser). */
  completeAuth(outcome: "success" | "cancel" | "network"): void {
    if (!this.flow) return;
    const flow_id = this.flow.flow_id;
    this.flow = null;
    const now = new Date().toISOString();
    if (outcome === "success") {
      const tier = this.scenario === "premium" ? "premium" : "free";
      this.faults.auth_expired = false;
      this.auth = { state: "signed_in", backend: "mock", account: { display_name: "k***@example.com", tier }, checked_at: now };
      this.emit(PROPOSED_EVENTS.authChanged, { state: "signed_in", flow_id });
    } else {
      this.auth = { state: "signed_out", checked_at: now };
      this.emit(PROPOSED_EVENTS.authChanged, {
        state: "signed_out",
        flow_id,
        error: outcome === "cancel"
          ? this.err("CANCELLED", "用户取消了授权")
          : this.err("TRANSIENT_NETWORK", "授权过程中网络中断", true),
      });
    }
    this.emit(EVENTS.backendStatus, { backend: "mock", status: this.mockStatus(), checked_at: now });
  }

  /**
   * Simulates killing the app: running work is persisted as queued/paused by
   * the core, and no events are sent. The UI must recover from snapshot.
   */
  simulateRestart(): void {
    for (const s of this.tasks.values()) {
      const t = s.task;
      if (t.state === "running" || t.state === "retrying" || t.state === "verifying") {
        t.state = "queued";
        t.speed_bytes_per_sec = undefined;
        t.revision = (BigInt(t.revision ?? 0) + 1n).toString();
        t.updated_at = new Date().toISOString();
      }
    }
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.listeners.clear();
  }

  /* ---------------------------------------------------------------- */
  /* Transport                                                         */
  /* ---------------------------------------------------------------- */

  async invoke<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
    const extra = this.scenario === "huge_dir" && (cmd === "files_list" || cmd === "files_search") ? 380 : 0;
    await new Promise((r) => setTimeout(r, Math.max(0, this.faults.latency_ms + extra)));
    const handler = (this as unknown as Record<string, (a: unknown) => unknown>)[`cmd_${cmd}`];
    if (typeof handler !== "function") {
      throw `command ${cmd} not found`;
    }
    return clone(handler.call(this, args)) as CommandResult<K>;
  }

  async listen<E extends AnyEventName>(event: E, handler: (payload: AllEvents[E]) => void): Promise<Unlisten> {
    let set = this.listeners.get(event);
    if (!set) this.listeners.set(event, (set = new Set()));
    const h = handler as Handler;
    set.add(h);
    return () => set!.delete(h);
  }

  /* ---------------------------------------------------------------- */
  /* helpers                                                           */
  /* ---------------------------------------------------------------- */

  private err(code: ClientErrorCode, message: string, retryable = false, details?: Record<string, unknown>): ClientError {
    this.reqSeq++;
    return {
      code, message, retryable, backend: "mock",
      request_id: `mock-req-${(0x1000 + this.reqSeq).toString(16)}`,
      ...(details ? { details } : {}),
    };
  }

  private emit<E extends AnyEventName>(event: E, payload: AllEvents[E]): void {
    const set = this.listeners.get(event);
    if (!set || set.size === 0) return;
    const data = clone(payload);
    for (const h of [...set]) setTimeout(() => h(clone(data)), 0);
  }

  private ensureTimer(): void {
    if (this.timer || typeof setInterval === "undefined") return;
    this.timer = setInterval(() => this.step(), TICK_MS);
  }

  private mockStatus(): CapabilityReport["status"] {
    return this.auth?.state === "signed_in" ? "ready" : "needs_login";
  }

  /** Gate for every call that would touch the remote service. */
  private remote(): void {
    if (this.auth.state !== "signed_in") {
      throw this.err("AUTH_EXPIRED", this.auth.state === "expired" ? "授权已过期，请重新授权" : "尚未授权，请先登录");
    }
    if (this.faults.auth_expired) {
      this.auth = { ...this.auth, state: "expired", checked_at: new Date().toISOString() };
      this.emit(PROPOSED_EVENTS.authChanged, { state: "expired" });
      this.emit(EVENTS.backendStatus, { backend: "mock", status: "needs_login", checked_at: new Date().toISOString() });
      throw this.err("AUTH_EXPIRED", "访问令牌已过期（模拟）");
    }
    if (this.scenario === "network_down") {
      throw this.err("TRANSIENT_NETWORK", "无法连接到远端服务（模拟断网）", true);
    }
  }

  private insert(e: RemoteEntry): void {
    this.entries.set(e.id, e);
    const p = e.parent_id ?? ROOT_ID;
    let arr = this.children.get(p);
    if (!arr) this.children.set(p, (arr = []));
    arr.push(e.id);
    this.sortedCache.delete(p);
  }

  private detach(id: string): void {
    const e = this.entries.get(id);
    if (!e) return;
    const p = e.parent_id ?? ROOT_ID;
    const arr = this.children.get(p);
    if (arr) this.children.set(p, arr.filter((x) => x !== id));
    this.sortedCache.delete(p);
  }

  private siblingsOf(parent: string): RemoteEntry[] {
    return (this.children.get(parent) ?? []).map((id) => this.entries.get(id)!).filter(Boolean);
  }

  private nameTaken(parent: string, name: string, exceptId?: string): boolean {
    const nfc = name.normalize("NFC");
    return this.siblingsOf(parent).some((s) => s.id !== exceptId && s.name.normalize("NFC") === nfc);
  }

  private isUnder(id: string, ancestor: string): boolean {
    let cur: string | undefined = id;
    let guard = 0;
    while (cur && cur !== ROOT_ID && guard++ < 64) {
      if (cur === ancestor) return true;
      cur = this.entries.get(cur)?.parent_id;
    }
    return ancestor === ROOT_ID;
  }

  private readOnly(parent: string | undefined): boolean {
    return !!parent && parent !== ROOT_ID && this.isUnder(parent, SPECIAL.readOnlyShare);
  }

  private sortedChildren(parent: string): string[] {
    let ids = this.sortedCache.get(parent);
    if (!ids) {
      const coll = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });
      ids = [...(this.children.get(parent) ?? [])].sort((a, b) => {
        const x = this.entries.get(a)!;
        const y = this.entries.get(b)!;
        if (x.kind !== y.kind) return x.kind === "folder" ? -1 : 1;
        return coll.compare(x.name, y.name) || (a < b ? -1 : 1);
      });
      this.sortedCache.set(parent, ids);
    }
    return ids;
  }

  private page<T>(items: T[], cursor: string | undefined, limit: number | undefined): Page<T> {
    const lim = Math.min(Math.max(limit ?? 200, 1), 1000);
    const offset = cursor && /^o:\d+$/.test(cursor) ? Number(cursor.slice(2)) : 0;
    const slice = items.slice(offset, offset + lim);
    const next = offset + lim < items.length ? `o:${offset + lim}` : undefined;
    return next ? { items: slice, next_cursor: next } : { items: slice };
  }

  private sim(task: TransferTaskExt): TaskSim {
    const h = hashStr(task.id);
    return {
      task,
      baseSpeed: 4_000_000 + (h % 9) * 1_000_000,
      phase: (h % 628) / 100,
      dropped37: false,
      retryTicks: 0,
      attempts: 0,
      verifyTicks: 0,
      metaTicks: task.kind === "cloud_download" && !task.total_bytes ? 8 : 0,
      emitted: 0,
    };
  }

  private touch(s: TaskSim): void {
    s.task.revision = (BigInt(s.task.revision ?? 0) + 1n).toString();
    s.task.updated_at = new Date().toISOString();
  }

  private changed(s: TaskSim): void {
    this.touch(s);
    this.emit(EVENTS.transferChanged, {
      task_id: s.task.id,
      state: s.task.state,
      ...(s.task.error ? { error: s.task.error } : {}),
      revision: s.task.revision,
      updated_at: s.task.updated_at,
    });
  }

  private setState(s: TaskSim, state: TaskState, error?: ClientError): void {
    s.task.state = state;
    if (error) s.task.error = error;
    else delete s.task.error;
    if (state !== "running") s.task.speed_bytes_per_sec = undefined;
    this.changed(s);
  }

  /* ---------------------------------------------------------------- */
  /* Simulation tick                                                   */
  /* ---------------------------------------------------------------- */

  private step(): void {
    this.tick++;
    const sims = [...this.tasks.values()];

    // scheduler: local transfers obey max_concurrent_transfers
    const isLocal = (s: TaskSim) => s.task.kind !== "cloud_download";
    let runningLocal = sims.filter((s) => isLocal(s) && (s.task.state === "running" || s.task.state === "verifying" || s.task.state === "retrying")).length;
    const queued = sims
      .filter((s) => s.task.state === "queued")
      .sort((a, b) => a.task.created_at.localeCompare(b.task.created_at));
    for (const s of queued) {
      if (isLocal(s) && runningLocal >= this.settings.max_concurrent_transfers) continue;
      if (isLocal(s)) runningLocal++;
      this.setState(s, "running");
    }

    const limit = this.settings.bandwidth_limit_kib_s * 1024;
    const runningLocalSims = sims.filter((s) => isLocal(s) && s.task.state === "running");
    const share = limit > 0 && runningLocalSims.length > 0 ? limit / runningLocalSims.length : Infinity;

    for (const s of sims) {
      const t = s.task;
      if (t.state === "retrying") {
        if (this.scenario === "network_down") {
          if (this.tick % 12 === 0) {
            s.attempts++;
            t.error = this.err("TRANSIENT_NETWORK", `网络不可用，第 ${s.attempts} 次重试等待中`, true);
            this.changed(s);
          }
          continue;
        }
        if (--s.retryTicks <= 0) this.setState(s, "running");
        continue;
      }
      if (t.state === "verifying") {
        if (--s.verifyTicks <= 0) {
          if (this.faults.integrity_fail && t.kind === "download") {
            this.setState(s, "failed", this.err("INTEGRITY_FAILED", "SHA-1 与远端记录不一致，已保留 .part 文件", true, { part_file_kept: true }));
          } else {
            this.setState(s, "completed");
          }
        }
        continue;
      }
      if (t.state !== "running") continue;

      if (this.scenario === "network_down" && isLocal(s)) {
        s.attempts = 1;
        this.setState(s, "retrying", this.err("TRANSIENT_NETWORK", "网络不可用，第 1 次重试等待中", true));
        continue;
      }
      if (this.scenario === "quota_exhausted" || (t.kind === "download" && this.dailyExhausted())) {
        if (t.kind === "download") {
          this.setState(s, "failed", this.err("QUOTA_EXCEEDED", "Connected Apps 每日下行配额已用尽", false, { dimension: "downstream_daily" }));
          continue;
        }
        if (t.kind === "cloud_download" && this.scenario === "quota_exhausted") {
          // already-running cloud tasks keep running server-side
        }
      }

      if (s.metaTicks > 0) {
        s.metaTicks--;
        if (s.metaTicks === 0) {
          t.total_bytes = String(700_000_000n + BigInt(hashStr(t.id) % 3_000_000_000));
          this.changed(s);
        }
        continue;
      }

      const total = t.total_bytes ? BigInt(t.total_bytes) : null;
      let cur = BigInt(t.transferred_bytes);
      const speed = t.kind === "cloud_download"
        ? 24_000_000
        : Math.min(share, s.baseSpeed * (0.85 + 0.15 * Math.sin(this.tick / 6 + s.phase)));
      const delta = BigInt(Math.max(0, Math.round((speed * TICK_MS) / 1000)));
      const before = cur;
      cur += delta;
      if (total !== null && cur >= total) cur = total;

      if (this.faults.drop_at_37 && !s.dropped37 && total && t.kind === "download") {
        const mark = (total * 37n) / 100n;
        if (before < mark && cur >= mark) {
          s.dropped37 = true;
          s.attempts = 1;
          s.retryTicks = 10;
          t.transferred_bytes = mark.toString();
          this.setState(s, "retrying", this.err("TRANSIENT_NETWORK", "连接中断（模拟：37% 处断网），第 1/3 次重试", true));
          continue;
        }
      }

      if (t.kind === "download") this.dailyUsed += cur - before;
      t.transferred_bytes = cur.toString();
      t.speed_bytes_per_sec = t.kind === "cloud_download" ? undefined : Math.round(speed);
      this.touch(s);
      const payload: AllEvents["nekopak://transfer/progress"] = {
        task_id: t.id,
        transferred_bytes: t.transferred_bytes,
        ...(t.total_bytes ? { total_bytes: t.total_bytes } : {}),
        ...(t.speed_bytes_per_sec !== undefined ? { speed_bytes_per_sec: t.speed_bytes_per_sec } : {}),
        state: t.state,
        revision: t.revision,
        updated_at: t.updated_at,
      };
      this.emit(EVENTS.transferProgress, payload);
      s.emitted++;
      if (this.faults.out_of_order && s.lastPayload && s.emitted % 4 === 0) {
        // deliberately late, older event: UI must discard by revision
        this.emit(EVENTS.transferProgress, s.lastPayload);
      }
      s.lastPayload = payload;

      if (total !== null && cur >= total) {
        if (t.kind === "cloud_download") {
          this.setState(s, "completed");
        } else {
          s.verifyTicks = 6;
          this.setState(s, "verifying");
        }
      }
    }

    if (this.tick % 8 === 0 && this.auth.state === "signed_in" && this.scenario !== "network_down") {
      this.emit(EVENTS.quotaChanged, { quota: this.quota(), checked_at: new Date().toISOString() });
    }

    this.stepSync();
  }

  private dailyExhausted(): boolean {
    return this.scenario !== "premium" && this.dailyUsed >= 5n * GiB;
  }

  private stepSync(): void {
    for (const [scan_id, sc] of this.scans) {
      sc.ticks++;
      sc.files += 37 + (sc.ticks % 5) * 11;
      const warnings = sc.config_id === "cfg-work" && sc.ticks > 3 ? ["3 个目录无权读取，已跳过"] : [];
      this.emit(EVENTS.syncScanProgress, { scan_id, files_scanned: sc.files, warnings });
      if (this.scenario === "network_down" && sc.ticks >= 4) {
        this.scans.delete(scan_id);
        const error = this.err("TRANSIENT_NETWORK", "扫描远端目录时网络中断；未生成任何操作", true);
        this.history.unshift({ id: `h-${scan_id}`, config_id: sc.config_id, at: new Date().toISOString(), kind: "error", summary: "扫描失败，未生成任何操作", error });
        this.emit(EVENTS.syncChanged, { job_id: scan_id, state: "failed", error });
        continue;
      }
      if (sc.ticks >= 8) {
        this.scans.delete(scan_id);
        const plan = this.buildPlan(sc.config_id);
        this.plans.set(sc.config_id, plan);
        const counts = plan.actions.reduce<Record<string, number>>((m, a) => ((m[a.op] = (m[a.op] ?? 0) + 1), m), {});
        const summary = Object.entries(counts).map(([k, v]) => `${k}:${v}`).join(", ");
        this.history.unshift({ id: `h-${plan.plan_id}`, config_id: sc.config_id, at: new Date().toISOString(), kind: "plan", summary: `生成计划：${plan.actions.length} 项（${summary}）` });
        this.emit(EVENTS.syncPlanReady, { plan_id: plan.plan_id, summary, config_id: sc.config_id });
      }
    }
    for (const [job_id, job] of this.jobs) {
      job.ticks++;
      if (job.ticks === 1) this.emit(EVENTS.syncChanged, { job_id, state: "running" });
      if (job.ticks >= 8) {
        this.jobs.delete(job_id);
        const cfg = this.syncConfigs.get(job.config_id);
        const plan = this.plans.get(job.config_id);
        if (cfg) {
          cfg.first_run = false;
          cfg.last_applied_at = new Date().toISOString();
        }
        this.plans.delete(job.config_id);
        const n = plan?.actions.filter((a) => a.op !== "skip").length ?? 0;
        this.history.unshift({ id: `h-${job_id}`, config_id: job.config_id, at: new Date().toISOString(), kind: "apply", summary: `已执行 ${n} 项操作（Mock，未写入任何真实文件）` });
        this.emit(EVENTS.syncChanged, { job_id, state: "completed" });
      }
    }
  }

  private buildPlan(config_id: string): SyncPlan & { created_at: number; config_id: string } {
    const cfg = this.syncConfigs.get(config_id)!;
    const a = (op: SyncAction["op"], path: string, reason: string, destructive = false): SyncAction => ({
      id: `${config_id}-${hashStr(op + path).toString(16)}`, op, path, reason, destructive,
    });
    let actions: SyncAction[] = [];
    let warnings: string[] = [];
    let conflicts = false;
    if (config_id === "cfg-courses") {
      actions = [
        a("mkdir", "实验数据/", "本地新增文件夹"),
        a("upload", "实验数据/示波器截图 01.png", "本地新增"),
        a("upload", "实验数据/示波器截图 02.png", "本地新增"),
        a("upload", "量子信息导论 课堂笔记.md", "本地新增"),
        a("upload", "微积分 期中复习.pdf", "本地较新：大小与修改时间均不同"),
        a("keep_both", "中国近现代史纲要 小组展示.pptx", "两端都有修改：云端副本另存为“… (冲突 2026-11-07).pptx”，不覆盖"),
        a("skip", "Thumbs.db", "已忽略：匹配排除规则 Thumbs.db"),
        a("skip", "~$实验报告.docx", "已忽略：匹配排除规则 ~$*"),
        a("skip", "线性代数笔记.pdf（仅云端存在）", cfg.first_run ? "首次同步不执行删除" : "删除未开启"),
      ];
      warnings = cfg.first_run ? ["首次同步，不会执行任何删除。"] : [];
      warnings.push("1 个文件两端都有修改，将保留两份。");
    } else if (config_id === "cfg-movies") {
      actions = [
        a("download", "纪录片 S01E02.mp4", "云端新增"),
        a("download", "原声带 Disc 1.flac", "云端新增"),
        a("skip", "4K 演示 — HDR10 (2160p).mkv", "未变化"),
        ...(cfg.allow_deletes && !cfg.first_run
          ? [
              a("delete", "旧片单.txt", "上次同步后已在云端删除", true),
              a("delete", "纪录片 S00 预告.mp4", "上次同步后已在云端删除", true),
            ]
          : []),
      ];
      if (actions.some((x) => x.destructive)) warnings.push("此计划会删除本地 2 个文件，需要单独确认。");
    } else {
      actions = [
        a("skip", "设计稿/", "无权读取，未扫描"),
        a("skip", ".git/objects/", "已忽略：匹配排除规则 .git/**"),
        a("upload", "README.md", "本地较新"),
        a("skip", "NekoPak 设计稿 v0.1.fig", "冲突未解决：两端都有修改且无法比较版本"),
      ];
      warnings = ["扫描不完整：3 个目录无权读取。扫描不完整时不会生成删除操作。", "存在未解决的冲突，需要先处理才能执行。"];
      conflicts = true;
    }
    return {
      plan_id: `plan-${config_id}-${this.tick}`,
      base_revision: `rev-${hashStr(config_id + this.tick).toString(16)}`,
      source: cfg.mode === "download_only" ? cfg.remote_path : cfg.local_path,
      destination: cfg.mode === "download_only" ? cfg.local_path : cfg.remote_path,
      mode: cfg.mode,
      actions,
      warnings,
      has_unresolved_conflicts: conflicts,
      has_deletes: actions.some((x) => x.op === "delete"),
      created_at: Date.now(),
      config_id,
    };
  }

  private quota(): QuotaReport {
    const tier = this.scenario === "premium" ? "premium" : "free";
    const nextSgMidnight = () => {
      const now = new Date();
      const sg = new Date(now.getTime() + 8 * 3600_000);
      sg.setUTCHours(24, 0, 0, 0);
      return new Date(sg.getTime() - 8 * 3600_000).toISOString();
    };
    const items: QuotaItem[] = [];
    if (tier === "free") {
      const limit = 5n * GiB;
      const used = this.dailyUsed > limit ? limit : this.dailyUsed;
      items.push({
        dimension: "downstream_daily", scope: "connected_apps", unit: "bytes",
        limit: limit.toString(), used: used.toString(), remaining: (limit - used).toString(),
        exactness: "exact",
        state: used >= limit ? "exhausted" : used * 10n >= limit * 8n ? "near_limit" : "ok",
        resets_at: nextSgMidnight(),
        note: "Free 账号 Connected Apps 每日下行上限 5 GB，新加坡时间 00:00 重置",
      });
      items.push({ dimension: "downstream_monthly", scope: "connected_apps", unit: "bytes", exactness: "unavailable", state: "unknown", note: "后端未提供可靠的剩余值" });
      items.push({ dimension: "upload_monthly", scope: "connected_apps", unit: "bytes", exactness: "unavailable", state: "unknown", note: "后端未提供可靠的剩余值" });
      items.push(
        this.scenario === "quota_exhausted"
          ? { dimension: "cloud_download_monthly", scope: "connected_apps", unit: "count", limit: "5", used: "5", remaining: "0", exactness: "exact", state: "exhausted", note: "本月 Connected Apps 云下载次数已用完" }
          : { dimension: "cloud_download_monthly", scope: "connected_apps", unit: "count", exactness: "unavailable", state: "unknown", note: "后端未提供可靠的剩余值" },
      );
    } else {
      items.push({ dimension: "downstream_monthly", scope: "connected_apps", unit: "bytes", limit: String(1536n * GiB), used: String(220n * GiB), remaining: String(1316n * GiB), exactness: "exact", state: "ok", resets_at: "2026-12-01T00:00:00Z" });
      items.push({ dimension: "upload_monthly", scope: "connected_apps", unit: "bytes", used: String(12n * GiB), exactness: "estimated", state: "unknown", note: "仅为本机估算，不代表服务端余额" });
      items.push({ dimension: "cloud_download_monthly", scope: "connected_apps", unit: "count", limit: "500", used: "37", remaining: "463", exactness: "exact", state: "ok", resets_at: "2026-12-01T00:00:00Z" });
    }
    return { backend: "mock", account_tier: tier, items, checked_at: new Date().toISOString() };
  }

  /* ---------------------------------------------------------------- */
  /* Commands                                                          */
  /* ---------------------------------------------------------------- */

  protected cmd_app_status(): AppStatus {
    return {
      version: "0.1.0-m0",
      contract_version: "0.1",
      runtime: "mock",
      storage: { status: "ok" },
      feature_flags: { upload: false, stream_url: false, sync_bidirectional: false, mount: false },
    };
  }

  protected cmd_backend_discover(): CapabilityReport[] {
    const now = new Date().toISOString();
    const ops: Partial<Record<Operation, boolean>> = {
      list: true, stat: true, search: true, mkdir: true, rename: true, move: true, copy: true,
      trash: true, download: true, cloud_task: true, quota: true,
      upload: false, stream_url: false, read_range: false,
    };
    const tier = this.auth.account?.tier ?? "unknown";
    return [
      {
        backend: "mock", status: this.mockStatus(), operations: ops,
        account_tier: this.auth.state === "signed_in" ? tier : "unknown",
        version: "mock-0.1",
        limitations: [
          "离线演示数据，不连接任何真实服务",
          "上传、流媒体链接与随机读取（read_range）未提供",
          ...(this.scenario === "network_down" ? ["远端不可达（模拟断网）"] : []),
        ],
        checked_at: now,
      },
      { backend: "official_cli", status: "not_installed", operations: {}, limitations: ["未检测到 PikPak 官方 CLI，需要用户自行安装", "JSON 输出、断点与随机读取能力尚未实测", "NekoPak 不捆绑官方 CLI"], checked_at: now },
      { backend: "rust_compat", status: "unavailable", operations: {}, limitations: ["社区兼容层（非官方接口）尚未审计，M0 不启用"], checked_at: now },
      { backend: "webdav", status: this.scenario === "premium" ? "needs_login" : "unavailable", operations: {}, account_tier: tier, limitations: ["PikPak WebDAV 为 Premium 功能，需要 WebDAV 凭据", "与其他 Connected Apps 共享配额"], checked_at: now },
      { backend: "rclone", status: "not_installed", operations: {}, limitations: ["NekoPak 不捆绑 Rclone，需要用户自行安装"], checked_at: now },
    ];
  }

  protected cmd_auth_begin(args: CommandArgs<"auth_begin">): AuthFlow {
    if (args.backend !== "mock") throw this.err("UNSUPPORTED", "该后端尚未接入授权流程");
    if (this.scenario === "network_down") throw this.err("TRANSIENT_NETWORK", "无法打开授权流程：网络不可用", true);
    this.flow = {
      flow_id: `flow-${++this.reqSeq}`,
      backend: "mock",
      kind: "external_browser",
      instructions: "Mock 模式不会打开浏览器。用下方的模拟按钮完成或取消授权。",
      expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
    };
    this.auth = { state: "pending", backend: "mock", checked_at: new Date().toISOString() };
    this.emit(PROPOSED_EVENTS.authChanged, { state: "pending", flow_id: this.flow.flow_id });
    return this.flow;
  }

  protected cmd_auth_cancel(args: CommandArgs<"auth_cancel">): void {
    if (this.flow?.flow_id === args.flow_id) this.completeAuth("cancel");
  }

  protected cmd_auth_status(): AuthState {
    return { ...this.auth, checked_at: new Date().toISOString() };
  }

  protected cmd_auth_logout(args: CommandArgs<"auth_logout">): void {
    if (args.backend !== "mock") throw this.err("UNSUPPORTED", "该后端未登录");
    this.auth = { state: "signed_out", checked_at: new Date().toISOString() };
    this.emit(PROPOSED_EVENTS.authChanged, { state: "signed_out" });
    this.emit(EVENTS.backendStatus, { backend: "mock", status: "needs_login", checked_at: new Date().toISOString() });
  }

  protected cmd_files_list(args: CommandArgs<"files_list">): Page<RemoteEntry> {
    this.remote();
    const parent = args.parent_id || ROOT_ID;
    if (parent === SPECIAL.vanished) throw this.err("NOT_FOUND", "文件夹已不存在（可能在其他设备上被删除）");
    if (parent !== ROOT_ID && this.entries.get(parent)?.kind !== "folder") throw this.err("NOT_FOUND", "文件夹不存在");
    const ids = this.sortedChildren(parent);
    const p = this.page(ids, args.cursor, args.limit);
    return { items: p.items.map((id) => this.entries.get(id)!), ...(p.next_cursor ? { next_cursor: p.next_cursor } : {}) };
  }

  protected cmd_files_stat(args: CommandArgs<"files_stat">): RemoteEntry {
    this.remote();
    const e = this.entries.get(args.id) ?? this.trash.get(args.id)?.entry;
    if (!e) throw this.err("NOT_FOUND", "项目不存在");
    return e;
  }

  protected cmd_files_path(args: CommandArgs<"files_path">): RemoteEntry[] {
    this.remote();
    if (args.id === ROOT_ID) return [];
    const chain: RemoteEntry[] = [];
    let cur = this.entries.get(args.id);
    if (!cur) throw this.err("NOT_FOUND", "项目不存在");
    let guard = 0;
    while (cur && guard++ < 64) {
      chain.unshift(cur);
      cur = cur.parent_id && cur.parent_id !== ROOT_ID ? this.entries.get(cur.parent_id) : undefined;
    }
    return chain;
  }

  protected cmd_files_search(args: CommandArgs<"files_search">): Page<RemoteEntry> {
    this.remote();
    const q = args.query.normalize("NFC").toLowerCase().trim();
    if (!q) return { items: [] };
    const hits: RemoteEntry[] = [];
    for (const e of this.entries.values()) {
      if (args.parent_id && !this.isUnder(e.id, args.parent_id)) continue;
      if (e.name.normalize("NFC").toLowerCase().includes(q)) hits.push(e);
      if (hits.length >= 5000) break;
    }
    return this.page(hits, args.cursor, args.limit ?? 200);
  }

  protected cmd_files_recent(args: CommandArgs<"files_recent">): Page<RemoteEntry> {
    this.remote();
    const files = [...this.entries.values()]
      .filter((e) => e.kind === "file" && !e.id.startsWith("h-"))
      .sort((a, b) => (b.modified_at ?? "").localeCompare(a.modified_at ?? ""))
      .slice(0, 60);
    return this.page(files, args.cursor, args.limit ?? 100);
  }

  protected cmd_trash_list(args: CommandArgs<"trash_list">): Page<RemoteEntry> {
    this.remote();
    return this.page([...this.trash.values()].map((t) => t.entry), args.cursor, args.limit);
  }

  protected cmd_files_mutate(args: CommandArgs<"files_mutate">): OperationResult {
    if (!args.idempotency_key) throw this.err("INTERNAL", "缺少 idempotency_key");
    const prior = this.idem.get(args.idempotency_key) as OperationResult | undefined;
    if (prior) return { ...prior, replayed: true };
    this.remote();

    const op: MutateOp = args.op;
    const res: OperationResult = { op, succeeded: [], failed: [], replayed: false };
    const fail = (id: string, e: ClientError) => res.failed.push({ id, error: e });
    const target = args.target ?? {};

    if (op === "mkdir") {
      const parent = target.parent_id ?? ROOT_ID;
      const name = (target.name ?? "").trim();
      if (this.readOnly(parent) || parent === SPECIAL.readOnlyShare) throw this.err("PERMISSION_DENIED", "目标文件夹为只读共享");
      if (!name) throw this.err("CONFLICT", "名称不能为空");
      if (this.nameTaken(parent, name)) throw this.err("CONFLICT", `“${name}”已存在`);
      const e: RemoteEntry = { id: `n-${++this.reqSeq}`, parent_id: parent, name, kind: "folder", size_bytes: "0", modified_at: new Date().toISOString() };
      this.insert(e);
      res.succeeded.push(e.id);
      res.created = [e];
    } else {
      for (const id of args.ids) {
        if (op === "restore" || op === "delete_forever") {
          const rec = this.trash.get(id);
          if (!rec) { fail(id, this.err("NOT_FOUND", "回收站中没有此项")); continue; }
          if (op === "delete_forever") {
            this.trash.delete(id);
            res.succeeded.push(id);
            continue;
          }
          const parent = this.entries.has(rec.original_parent) ? rec.original_parent : ROOT_ID;
          if (this.nameTaken(parent, rec.entry.name)) { fail(id, this.err("CONFLICT", `原位置已有“${rec.entry.name}”`)); continue; }
          this.trash.delete(id);
          this.insert({ ...rec.entry, parent_id: parent });
          res.succeeded.push(id);
          continue;
        }

        const e = this.entries.get(id);
        if (!e) { fail(id, this.err("NOT_FOUND", "项目不存在")); continue; }
        if (this.readOnly(e.parent_id)) { fail(id, this.err("PERMISSION_DENIED", "只读共享中的项目不能修改")); continue; }

        if (op === "rename") {
          const name = (target.name ?? "").trim();
          if (!name) { fail(id, this.err("CONFLICT", "名称不能为空")); continue; }
          if (this.nameTaken(e.parent_id ?? ROOT_ID, name, id)) { fail(id, this.err("CONFLICT", `“${name}”已存在`)); continue; }
          e.name = name;
          e.modified_at = new Date().toISOString();
          this.sortedCache.delete(e.parent_id ?? ROOT_ID);
          res.succeeded.push(id);
        } else if (op === "move" || op === "copy") {
          const dest = target.parent_id ?? ROOT_ID;
          if (this.readOnly(dest) || dest === SPECIAL.readOnlyShare) { fail(id, this.err("PERMISSION_DENIED", "目标文件夹为只读共享")); continue; }
          if (dest !== ROOT_ID && this.entries.get(dest)?.kind !== "folder") { fail(id, this.err("NOT_FOUND", "目标文件夹不存在")); continue; }
          if (e.kind === "folder" && (dest === id || (dest !== ROOT_ID && this.isUnder(dest, id)))) { fail(id, this.err("CONFLICT", "不能移动或复制到自身或其子文件夹")); continue; }
          if (op === "move") {
            if ((e.parent_id ?? ROOT_ID) === dest) { fail(id, this.err("CONFLICT", "已在目标文件夹中")); continue; }
            if (this.nameTaken(dest, e.name)) { fail(id, this.err("CONFLICT", `目标位置已有“${e.name}”`)); continue; }
            this.detach(id);
            e.parent_id = dest;
            this.insert(e);
            this.entries.set(id, e);
            res.succeeded.push(id);
          } else {
            if (e.id === SPECIAL.huge) { fail(id, this.err("UNSUPPORTED", "Mock 不支持复制超大文件夹")); continue; }
            let name = e.name;
            if (this.nameTaken(dest, name)) {
              if ((e.parent_id ?? ROOT_ID) !== dest) { fail(id, this.err("CONFLICT", `目标位置已有“${e.name}”`)); continue; }
              const dot = name.lastIndexOf(".");
              name = e.kind === "file" && dot > 0 ? `${name.slice(0, dot)} (副本)${name.slice(dot)}` : `${name} (副本)`;
              let n = 2;
              while (this.nameTaken(dest, name)) name = `${e.name} (副本 ${n++})`;
            }
            const copy: RemoteEntry = { ...e, id: `c-${++this.reqSeq}`, parent_id: dest, name, modified_at: new Date().toISOString() };
            this.insert(copy);
            if (e.kind === "folder") {
              for (const child of this.siblingsOf(e.id)) this.insert({ ...child, id: `c-${++this.reqSeq}`, parent_id: copy.id });
            }
            res.succeeded.push(id);
            (res.created ??= []).push(copy);
          }
        } else if (op === "trash") {
          if (id === SPECIAL.readOnlyShare) { fail(id, this.err("PERMISSION_DENIED", "共享文件夹不能移至回收站，请在官方应用中退出共享")); continue; }
          this.detach(id);
          this.entries.delete(id);
          this.trash.set(id, { entry: { ...e }, original_parent: e.parent_id ?? ROOT_ID });
          res.succeeded.push(id);
        }
      }
    }
    this.idem.set(args.idempotency_key, res);
    return res;
  }

  protected cmd_cloud_task_create(args: CommandArgs<"cloud_task_create">): { task_id: string } {
    const prior = this.idem.get(args.idempotency_key) as { task_id: string } | undefined;
    if (prior) return prior;
    this.remote();
    const src = args.source_url.trim();
    const isMagnet = /^magnet:\?xt=urn:btih:[a-z0-9]{32,40}/i.test(src);
    const isHttp = /^https?:\/\/[^\s/]+\.[^\s]+/i.test(src);
    if (!isMagnet && !isHttp) throw this.err("UNSUPPORTED", "不支持的链接格式：仅支持 magnet: 与 http(s) 链接", false, { reason: "invalid_source" });
    if (this.scenario === "quota_exhausted") throw this.err("QUOTA_EXCEEDED", "本月 Connected Apps 云下载次数已用完", false, { dimension: "cloud_download_monthly" });
    const parent = args.parent_id || ROOT_ID;
    if (this.readOnly(parent) || parent === SPECIAL.readOnlyShare) throw this.err("PERMISSION_DENIED", "目标文件夹为只读共享");

    let name = "云下载任务";
    if (isMagnet) {
      const dn = /[?&]dn=([^&]+)/i.exec(src);
      name = dn ? decodeURIComponent(dn[1].replace(/\+/g, " ")) : "magnet:?xt=urn:btih:<已隐藏>";
    } else {
      try {
        const u = new URL(src);
        name = decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() ?? u.hostname);
      } catch {
        name = "URL 任务";
      }
    }
    const now = new Date().toISOString();
    const id = `task-${++this.taskSeq}`;
    const path = parent === ROOT_ID ? "/" : `/${this.cmd_files_path({ id: parent }).map((e) => e.name).join("/")}`;
    const task: TransferTaskExt = {
      id, remote_id: `cloud-${id}`, kind: "cloud_download", state: "queued", name,
      transferred_bytes: "0", backend: "mock", destination: path, created_at: now, updated_at: now, revision: 1,
    };
    const s = this.sim(task);
    s.metaTicks = 8;
    this.tasks.set(id, s);
    this.emit(EVENTS.transferChanged, { task_id: id, state: "queued", revision: 1, updated_at: now });
    const ref = { task_id: id };
    this.idem.set(args.idempotency_key, ref);
    return ref;
  }

  protected cmd_transfers_list(args: CommandArgs<"transfers_list">): Page<TransferTaskExt> {
    const all = [...this.tasks.values()].map((s) => s.task).sort((a, b) => b.created_at.localeCompare(a.created_at));
    return this.page(all, args.cursor, args.limit ?? 500);
  }

  protected cmd_transfer_start(args: CommandArgs<"transfer_start">): { task_id: string } {
    this.remote();
    const e = this.entries.get(args.remote_id);
    if (!e) throw this.err("NOT_FOUND", "远端文件不存在");
    if (e.kind === "folder") throw this.err("UNSUPPORTED", "M0 暂不支持下载整个文件夹");
    if (this.scenario === "quota_exhausted" || this.dailyExhausted()) {
      throw this.err("QUOTA_EXCEEDED", "Connected Apps 每日下行配额已用尽", false, { dimension: "downstream_daily" });
    }
    const dir = args.destination.replace(/[\\/]+$/, "");
    const dest = `${dir}\\${escapeWindowsName(e.name)}`;
    const clash = [...this.tasks.values()].find(
      (s) => s.task.kind === "download" && s.task.state !== "cancelled" && s.task.state !== "failed" &&
        (s.task.destination ?? "").toLowerCase() === dest.toLowerCase(),
    );
    if (clash) throw this.err("CONFLICT", `目标位置已有同名文件或任务：${dest}`, false, { existing_task: clash.task.id });
    const now = new Date().toISOString();
    const id = `task-${++this.taskSeq}`;
    const task: TransferTaskExt = {
      id, remote_id: e.id, kind: "download", state: "queued", name: e.name,
      transferred_bytes: "0", total_bytes: e.size_bytes, backend: "mock",
      destination: dest, created_at: now, updated_at: now, revision: 1,
    };
    this.tasks.set(id, this.sim(task));
    this.emit(EVENTS.transferChanged, { task_id: id, state: "queued", revision: 1, updated_at: now });
    return { task_id: id };
  }

  protected cmd_transfer_control(args: CommandArgs<"transfer_control">): void {
    const s = this.tasks.get(args.task_id);
    if (!s) throw this.err("NOT_FOUND", "任务不存在");
    const st = s.task.state;
    const cloud = s.task.kind === "cloud_download";
    switch (args.action) {
      case "pause":
        if (cloud) throw this.err("UNSUPPORTED", "云下载由服务端执行，不能暂停");
        if (st !== "running" && st !== "queued" && st !== "retrying") throw this.err("CONFLICT", "当前状态不能暂停");
        this.setState(s, "paused");
        return;
      case "resume":
        if (st !== "paused") throw this.err("CONFLICT", "只有已暂停的任务可以继续");
        this.setState(s, "queued");
        return;
      case "cancel":
        if (st === "completed" || st === "cancelled") throw this.err("CONFLICT", "任务已结束");
        this.setState(s, "cancelled");
        return;
      case "retry":
        if (st !== "failed" && st !== "cancelled") throw this.err("CONFLICT", "只有失败或已取消的任务可以重试");
        if (s.task.error?.code === "INTEGRITY_FAILED") s.task.transferred_bytes = "0";
        s.dropped37 = true;
        this.setState(s, "queued");
        return;
    }
  }

  protected cmd_quota_get(): QuotaReport {
    this.remote();
    return this.quota();
  }

  protected cmd_sync_configs_list(): SyncConfig[] {
    return [...this.syncConfigs.values()];
  }

  protected cmd_sync_config_save(args: CommandArgs<"sync_config_save">): SyncConfig {
    const c = args.config;
    if (c.mode === "bidirectional") throw this.err("UNSUPPORTED", "双向同步将在 M3 开放");
    if (!/^[a-zA-Z]:\\|^%[A-Z]+%\\|^\//.test(c.local_path)) throw this.err("CONFLICT", "本地路径必须是绝对路径");
    const existing = this.syncConfigs.get(c.config_id);
    const first_run = existing ? existing.first_run : true;
    if (c.allow_deletes && first_run) throw this.err("CONFLICT", "首次同步不能开启删除");
    const saved: SyncConfig = { ...c, first_run, config_id: c.config_id || `cfg-${++this.reqSeq}` };
    this.syncConfigs.set(saved.config_id, saved);
    this.plans.delete(saved.config_id);
    return saved;
  }

  protected cmd_sync_scan(args: CommandArgs<"sync_scan">): { scan_id: string } {
    if (!this.syncConfigs.has(args.config_id)) throw this.err("NOT_FOUND", "同步配置不存在");
    if (this.auth.state !== "signed_in") throw this.err("AUTH_EXPIRED", "尚未授权，请先登录");
    const scan_id = `scan-${++this.reqSeq}`;
    this.plans.delete(args.config_id);
    this.scans.set(scan_id, { config_id: args.config_id, ticks: 0, files: 0 });
    return { scan_id };
  }

  protected cmd_sync_plan(args: CommandArgs<"sync_plan">): SyncPlan {
    const p = this.plans.get(args.config_id);
    if (!p) throw this.err("NOT_FOUND", "尚未生成计划，请先扫描");
    const { created_at: _c, config_id: _id, ...plan } = p;
    return plan;
  }

  protected cmd_sync_apply(args: CommandArgs<"sync_apply">): { task_id: string } {
    const p = [...this.plans.values()].find((x) => x.plan_id === args.plan_id);
    if (!p) throw this.err("CONFLICT", "计划已失效（可能已执行或被新的扫描替换），请重新扫描");
    if (Date.now() - p.created_at > 10 * 60_000) throw this.err("CONFLICT", "计划已过期（超过 10 分钟），请重新扫描以确认最新状态");
    if (p.has_unresolved_conflicts) throw this.err("CONFLICT", "计划含未解决的冲突，不能执行");
    if (p.has_deletes && !args.confirm_deletes) throw this.err("CONFLICT", "计划包含删除操作，需要 confirm_deletes");
    this.remote();
    const job_id = `job-${++this.reqSeq}`;
    this.jobs.set(job_id, { config_id: p.config_id, ticks: 0, plan_id: p.plan_id });
    return { task_id: job_id };
  }

  protected cmd_sync_history(args: CommandArgs<"sync_history">): Page<SyncEvent> {
    return this.page(this.history.filter((h) => h.config_id === args.config_id), args.cursor, 50);
  }

  protected cmd_mount_capabilities(): MountCapabilities {
    const ready = this.auth.state === "signed_in";
    return {
      platform: "windows",
      checked_at: new Date().toISOString(),
      modes: [
        {
          mode: "cloud_files_sync_root",
          available: false,
          checks: [
            { id: "os", label: "Windows 10 1709 或更高版本", status: "pass", detail: "模拟环境：Windows 11 23H2" },
            { id: "backend_list", label: "后端可列目录与读取元数据", status: ready ? "pass" : "fail", detail: ready ? "mock 后端声明 list / stat" : "需要先登录" },
            { id: "provider", label: "NekoPak Cloud Files 提供程序", status: "fail", detail: "尚未实现（路线图 M4）" },
          ],
        },
        {
          mode: "winfsp_drive",
          available: false,
          checks: [
            { id: "winfsp", label: "WinFsp 运行时", status: "fail", detail: "未检测到 WinFsp", requirement: "用户自行安装 WinFsp 2.x（GPLv3 + FLOSS 例外）。NekoPak 不会自动安装驱动。" },
            { id: "read_range", label: "后端随机读取（read_range）", status: "fail", detail: "当前后端未声明 read_range；官方 CLI 的随机读取能力尚未实测" },
            { id: "driver", label: "只读挂载实现", status: "fail", detail: "尚未实现（路线图 M4）" },
          ],
        },
      ],
    };
  }

  protected cmd_mount_start(): never {
    throw this.err("UNSUPPORTED", "挂载尚未实现：前置检查未全部通过");
  }

  protected cmd_mount_stop(): never {
    throw this.err("NOT_FOUND", "没有正在运行的挂载");
  }

  protected cmd_settings_get(): Settings {
    return this.settings;
  }

  protected cmd_settings_update(args: CommandArgs<"settings_update">): Settings {
    const p: SettingsPatch = args.patch ?? {};
    if (p.max_concurrent_transfers !== undefined && (p.max_concurrent_transfers < 1 || p.max_concurrent_transfers > 8)) {
      throw this.err("CONFLICT", "同时传输数需在 1–8 之间");
    }
    if (p.bandwidth_limit_kib_s !== undefined && p.bandwidth_limit_kib_s < 0) throw this.err("CONFLICT", "带宽限制不能为负数");
    if ((p.experimental_network as { custom_dns_mode?: boolean } | undefined)?.custom_dns_mode) {
      throw this.err("UNSUPPORTED", "自定义 DNS 尚未审计，不能开启");
    }
    this.settings = {
      ...this.settings,
      ...p,
      schema_version: 1,
      telemetry: false,
      cache: { ...this.settings.cache, ...(p.cache ?? {}) },
      experimental_network: {
        ...this.settings.experimental_network,
        ...(p.experimental_network ?? {}),
        custom_dns_mode: false,
        user_proxy_configured: this.settings.experimental_network.user_proxy_configured,
      },
    } as Settings;
    return this.settings;
  }
}

export type MockControls = Pick<MockTransport, "reset" | "setFaults" | "completeAuth" | "simulateRestart" | "scenario" | "faults">;
export type { BackendId };
