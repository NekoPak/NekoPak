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
import { language } from "../i18n";
import type {
  AppStatus, ArchiveEntry, ArchiveListing, SignInResult, AuthFlow, AuthState, BatchRef, BulkResult, ClipAlignment,
  CommandArgs, CommandName, CommandResult, DuplicateEntry, DuplicateGroup, DuplicateReport, EntryExt,
  ExtOperation, LinkPreview, MediaProbe, MountCapabilities, MutateOp, OperationResult, PreviewFrame,
  QualityCap, QualityOption, QuotaItem, QuotaReport, ServerJob, Settings, SettingsPatch, ShareInfo,
  SpaceNode, SpaceSummary, SyncConfig, SyncEvent, TransferOptions, TransferTaskExt,
} from "../contract-proposals";
import { PROPOSED_EVENTS, ROOT_ID } from "../contract-proposals";
import { clipLabel, formatClock, MIN_CLIP_MS } from "../clip";
import { fileKind, splitName } from "../names";
import type { AllEvents, AnyEventName, Transport, Unlisten } from "../transport/types";
import {
  ARCHIVES, buildTasks, buildTrash, buildTree, DEFAULT_FAULTS, previewFilesFor, SPECIAL,
  type FaultFlags, type ScenarioId,
} from "./fixtures";

const TICK_MS = 250;
const GiB = 1024n ** 3n;
const GOP_MS = 2002;
const TIER_HEIGHT: Record<Exclude<QualityCap, "original">, number> = { "2160p": 2160, "1080p": 1080, "720p": 720, "480p": 480 };
const TIER_BITRATE: Record<Exclude<QualityCap, "original">, number> = { "2160p": 16_000_000, "1080p": 5_000_000, "720p": 2_500_000, "480p": 1_100_000 };
const FOLDER_DOWNLOAD_MAX = 1000;

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
  private dupScans = new Map<string, { report: DuplicateReport; ticks: number; files: number; folders: number }>();
  private serverJobs = new Map<string, ServerJob & { ticks: number; file_id: string; paths: string[] | "all" }>();
  private passwordRefs = new Map<string, string>();
  private previews = new Map<string, LinkPreview & { source: string }>();
  private shares = new Map<string, ShareInfo>();

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
    this.dupScans.clear();
    this.serverJobs.clear();
    this.passwordRefs.clear();
    this.previews.clear();
    this.shares.clear();
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
      language: language.get().preference,
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
        block_access_domains: false,
      },
      telemetry: false,
      download: { default_quality: "original", connections_per_task: 4 },
    };

    const shareSeed: ShareInfo[] = [
      { share_id: "sh-1", title: "量子信息导论 第1讲.mp4", item_count: 1, url: "https://mock.invalid/s/VN-qi01", code: "k7m2", created_at: new Date(now - 86_400_000 * 3).toISOString(), expires_at: new Date(now + 86_400_000 * 4).toISOString(), state: "active", view_count: 12, save_count: 3 },
      { share_id: "sh-2", title: "照片 📷 2026", item_count: 1, url: "https://mock.invalid/s/VN-ph26", created_at: new Date(now - 86_400_000 * 40).toISOString(), expires_at: new Date(now - 86_400_000 * 10).toISOString(), state: "expired", view_count: 41, save_count: 0 },
    ];
    for (const sh of shareSeed) this.shares.set(sh.share_id, sh);

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
    const lim = limit ?? 200;
    if (!Number.isInteger(lim) || lim < 1 || lim > 500) throw this.err("CONFLICT", "每页数量必须为 1–500");
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
          this.setState(s, "failed", this.err("QUOTA_EXCEEDED", "今日下载额度已用完", false, { dimension: "downstream_daily" }));
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
    this.stepDuplicates();
    this.stepServerJobs();
  }

  private stepDuplicates(): void {
    for (const [scan_id, sc] of this.dupScans) {
      const r = sc.report;
      if (r.state !== "running") continue;
      sc.ticks++;
      const frac = Math.min(1, sc.ticks / 6);
      r.files_scanned = Math.round(sc.files * frac);
      r.folders_scanned = Math.round(sc.folders * frac);
      if (this.scenario === "network_down" && sc.ticks >= 3) {
        r.state = "failed";
        r.error = this.err("TRANSIENT_NETWORK", "扫描时网络中断，未生成报告", true);
      } else if (frac >= 1) {
        r.state = "completed";
        r.completed_at = new Date().toISOString();
      }
      this.emit(PROPOSED_EVENTS.duplicatesProgress, {
        scan_id, state: r.state, files_scanned: r.files_scanned, folders_scanned: r.folders_scanned,
        ...(r.error ? { error: r.error } : {}),
      });
    }
  }

  private stepServerJobs(): void {
    for (const job of this.serverJobs.values()) {
      if (job.state === "completed" || job.state === "failed" || job.state === "cancelled") continue;
      job.ticks++;
      if (job.state === "queued") job.state = "running";
      job.progress = Math.min(100, Math.round((job.ticks / 10) * 100));
      if (job.progress >= 100) {
        this.finishExtract(job);
        job.state = "completed";
      }
      job.revision = (BigInt(job.revision) + 1n).toString();
      job.updated_at = new Date().toISOString();
      const { ticks: _t, file_id: _f, paths: _p, ...row } = job;
      this.emit(PROPOSED_EVENTS.jobChanged, row);
    }
  }

  private finishExtract(job: ServerJob & { file_id: string; paths: string[] | "all" }): void {
    const src = this.entries.get(job.file_id);
    const fx = src ? ARCHIVES[src.name] : undefined;
    const parent = job.target_parent_id ?? ROOT_ID;
    if (!src || !fx) return;
    let folderName = splitName(src.name).base;
    let n = 2;
    while (this.nameTaken(parent, folderName)) folderName = `${splitName(src.name).base} (${n++})`;
    const root: RemoteEntry = { id: `x-${++this.reqSeq}`, parent_id: parent, name: folderName, kind: "folder", size_bytes: "0", modified_at: new Date().toISOString() };
    this.insert(root);
    const dirs = new Map<string, string>([["", root.id]]);
    const ensureDir = (path: string): string => {
      if (dirs.has(path)) return dirs.get(path)!;
      const cut = path.lastIndexOf("/");
      const parentId = ensureDir(cut < 0 ? "" : path.slice(0, cut));
      const e: RemoteEntry = { id: `x-${++this.reqSeq}`, parent_id: parentId, name: path.slice(cut + 1), kind: "folder", size_bytes: "0", modified_at: new Date().toISOString() };
      this.insert(e);
      dirs.set(path, e.id);
      return e.id;
    };
    const wanted = job.paths === "all" ? null : job.paths;
    for (const f of fx.files) {
      if (wanted && !wanted.some((p) => f.path === p || f.path.startsWith(`${p}/`))) continue;
      const cut = f.path.lastIndexOf("/");
      const pid = ensureDir(cut < 0 ? "" : f.path.slice(0, cut));
      this.insert({ id: `x-${++this.reqSeq}`, parent_id: pid, name: f.path.slice(cut + 1), kind: "file", size_bytes: f.size.toString(), modified_at: new Date().toISOString() });
    }
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
        note: "免费账号每日下载 5 GB，北京时间 00:00 重置",
      });
      items.push({ dimension: "downstream_monthly", scope: "connected_apps", unit: "bytes", exactness: "unavailable", state: "unknown", note: "后端未提供可靠的剩余值" });
      items.push({ dimension: "upload_monthly", scope: "connected_apps", unit: "bytes", exactness: "unavailable", state: "unknown", note: "后端未提供可靠的剩余值" });
      items.push(
        this.scenario === "quota_exhausted"
          ? { dimension: "cloud_download_monthly", scope: "connected_apps", unit: "count", limit: "5", used: "5", remaining: "0", exactness: "exact", state: "exhausted", note: "本月云下载次数已用完" }
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
      contract_version: "0.2",
      runtime: "mock",
      storage: { status: "ok" },
      feature_flags: { upload: false, stream_url: false, sync_bidirectional: false, mount: false },
    };
  }

  protected cmd_backend_discover(): CapabilityReport[] {
    const now = new Date().toISOString();
    const ext: Record<ExtOperation, boolean> = {
      media_probe: true, clip_download: true, quality_download: true, folder_download: true, star: true,
      duplicates: true, space_summary: true, archive: true, link_preview: true, external_player: true, share: true,
    };
    // Additive keys (PROPOSAL UI-001 §P13) ride along in the same map.
    const ops = {
      list: true, stat: true, search: true, mkdir: true, rename: true, move: true, copy: true,
      trash: true, download: true, cloud_task: true, quota: true,
      upload: false, stream_url: false, read_range: false,
      ...ext,
    } as Partial<Record<Operation, boolean>>;
    const tier = this.auth.account?.tier ?? "unknown";
    return [
      {
        backend: "mock", status: this.mockStatus(), operations: ops,
        account_tier: this.auth.state === "signed_in" ? tier : "unknown",
        version: "mock-0.1",
        limitations: [
          "离线演示数据，不连接任何真实服务",
          "上传与按需读取暂不支持",
          "视频预览为演示画面，不是视频内容",
          ...(this.scenario === "network_down" ? ["远端不可达（模拟断网）"] : []),
        ],
        checked_at: now,
      },
      // rust_compat is the desktop primary; the browser preview cannot reach it.
      { backend: "rust_compat", status: "unavailable", operations: {}, limitations: ["浏览器预览中不可用，桌面版提供", "非官方接口，PikPak 改版时可能需要更新"], checked_at: now },
      { backend: "webdav", status: this.scenario === "premium" ? "needs_login" : "unavailable", operations: {}, account_tier: tier, limitations: ["PikPak WebDAV 为 Premium 功能，需要 WebDAV 凭据", "与其他第三方应用共享额度"], checked_at: now },
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
      instructions: "演示模式不会打开浏览器。用开发者面板完成或取消登录。",
      expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
    };
    this.auth = { state: "pending", backend: "mock", checked_at: new Date().toISOString() };
    this.emit(PROPOSED_EVENTS.authChanged, { state: "pending", flow_id: this.flow.flow_id });
    return this.flow;
  }

  /* §P23 native sign-in (demo rules: password "wrong" fails; a phone number asks for a code). */
  private challenge: { id: string; code: string } | null = null;

  private signedIn(): { state: "signed_in" } {
    const tier = this.scenario === "premium" ? "premium" : "free";
    const now = new Date().toISOString();
    this.faults.auth_expired = false;
    this.auth = { state: "signed_in", backend: "mock", account: { display_name: "k***@example.com", tier }, checked_at: now };
    this.emit(PROPOSED_EVENTS.authChanged, { state: "signed_in" });
    this.emit(EVENTS.backendStatus, { backend: "mock", status: this.mockStatus(), checked_at: now });
    return { state: "signed_in" };
  }

  protected cmd_auth_sign_in(args: CommandArgs<"auth_sign_in">): SignInResult {
    if (this.scenario === "network_down") throw this.err("TRANSIENT_NETWORK", "无法连接 PikPak，请检查网络", true);
    const user = args.username.trim();
    if (!user || !args.password) throw this.err("CONFLICT", "请输入账号和密码");
    if (args.password === "wrong") throw this.err("PERMISSION_DENIED", "账号或密码不正确", false, { reason: "invalid_credentials" });
    if (args.password === "captcha") {
      this.challenge = { id: `ch-${++this.reqSeq}`, code: "" };
      return { state: "challenge", challenge: { challenge_id: this.challenge.id, kind: "captcha", url: "https://mock.invalid/captcha" } };
    }
    if (/^\+?\d[\d\s-]{6,}$/.test(user)) {
      this.challenge = { id: `ch-${++this.reqSeq}`, code: "123456" };
      const digits = user.replace(/\D/g, "");
      return {
        state: "challenge",
        challenge: { challenge_id: this.challenge.id, kind: "verify_code", channel: "sms", target: `${digits.slice(0, 3)}****${digits.slice(-4)}`, code_length: 6, resend_after_s: 60 },
      };
    }
    return this.signedIn();
  }

  protected cmd_auth_verify(args: CommandArgs<"auth_verify">): SignInResult {
    if (!this.challenge || this.challenge.id !== args.challenge_id) throw this.err("CONFLICT", "验证已过期，请重新登录");
    if (args.code.trim() !== this.challenge.code) throw this.err("PERMISSION_DENIED", "验证码不正确", false, { reason: "invalid_code" });
    this.challenge = null;
    return this.signedIn();
  }

  protected cmd_auth_resend(args: CommandArgs<"auth_resend">): { resend_after_s: number } {
    if (!this.challenge || this.challenge.id !== args.challenge_id) throw this.err("CONFLICT", "验证已过期，请重新登录");
    return { resend_after_s: 60 };
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
        if (op === "star" || op === "unstar") {
          // Starring is a per-account flag; it works inside read-only shares too.
          (e as EntryExt).starred = op === "star";
          res.succeeded.push(id);
          continue;
        }
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
            delete (copy as EntryExt).starred;
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
    if (this.scenario === "quota_exhausted") throw this.err("QUOTA_EXCEEDED", "本月云下载次数已用完", false, { dimension: "cloud_download_monthly" });
    const parent = args.parent_id || ROOT_ID;
    if (this.readOnly(parent) || parent === SPECIAL.readOnlyShare) throw this.err("PERMISSION_DENIED", "目标文件夹为只读共享");

    let picked: { name: string; bytes: bigint } | null = null;
    if (args.preview_id) {
      const pv = this.previews.get(args.preview_id);
      if (!pv || pv.source !== src) throw this.err("CONFLICT", "预览已过期，请重新解析链接");
      const idx = args.file_indices ?? [];
      if (!idx.length) throw this.err("CONFLICT", "至少选择一个文件");
      if (idx.some((i) => !pv.files[i])) throw this.err("CONFLICT", "所选文件序号无效");
      const bytes = idx.reduce((sum, i) => sum + BigInt(pv.files[i].size_bytes), 0n);
      picked = { name: idx.length === pv.files.length ? pv.title : `${pv.title}（已选 ${idx.length}/${pv.files.length} 个文件）`, bytes };
    }
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
      id, remote_id: `cloud-${id}`, kind: "cloud_download", state: "queued", name: picked?.name ?? name,
      transferred_bytes: "0", backend: "mock", destination: path, created_at: now, updated_at: now, revision: 1,
      ...(picked ? { total_bytes: picked.bytes.toString() } : {}),
    };
    const s = this.sim(task);
    s.metaTicks = picked ? 0 : 8;
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
    if (e.kind === "folder") throw this.err("UNSUPPORTED", "文件夹请使用 transfer_start_batch 下载");
    if (this.scenario === "quota_exhausted" || this.dailyExhausted()) {
      throw this.err("QUOTA_EXCEEDED", "今日下载额度已用完", false, { dimension: "downstream_daily" });
    }
    const dir = args.destination.replace(/[\\/]+$/, "");
    return { task_id: this.queueDownload(e, dir, args.options) };
  }

  /** Create one download task, applying tier / clip options. Throws ClientError. */
  private queueDownload(e: RemoteEntry, dir: string, options?: TransferOptions, group_id?: string): string {
    const video = this.isVideo(e);
    let name = e.name;
    let total = e.size_bytes;
    let quality_label: string | undefined;
    let clip: { start_ms: number; end_ms: number } | undefined;
    const probe = video ? this.probe(e) : null;
    let tier: QualityOption | undefined;
    if (options?.quality_id && options.quality_id !== "original") {
      if (!probe) throw this.err("UNSUPPORTED", "只有视频可以选择画质");
      tier = probe.qualities.find((q) => q.id === options.quality_id);
      if (!tier) throw this.err("NOT_FOUND", "PikPak 已不提供所选画质");
      if (!tier.readable) throw this.err("UNSUPPORTED", tier.unreadable_reason ?? "所选画质暂不可读");
    } else if (!options?.quality_id && options?.quality_cap && options.quality_cap !== "original" && probe) {
      const cap = TIER_HEIGHT[options.quality_cap];
      const source = probe.height ?? 0;
      if (source > cap) tier = probe.qualities.filter((q) => q.id !== "original" && q.readable && (q.height ?? 0) <= cap).sort((a, b) => (b.height ?? 0) - (a.height ?? 0))[0];
    }
    if (tier) {
      quality_label = tier.label;
      total = tier.size_bytes ?? total;
      name = `${splitName(e.name).base} [${tier.label}].mp4`;
    }
    if (options?.clip) {
      if (!probe) throw this.err("UNSUPPORTED", "只能截取视频文件的片段");
      const { start_ms, end_ms } = options.clip;
      if (!(start_ms >= 0 && end_ms <= probe.duration_ms && end_ms - start_ms >= MIN_CLIP_MS)) {
        throw this.err("CONFLICT", `片段范围无效：需在 0–${formatClock(probe.duration_ms)} 之间且不短于 1 秒`);
      }
      const a = this.align(e, probe, tier?.id ?? "original", start_ms, end_ms);
      clip = { start_ms: a.start_ms, end_ms: a.end_ms };
      total = a.estimated_bytes ?? total;
      const { base, ext } = splitName(name);
      name = `${base} [${clipLabel(a.start_ms, a.end_ms)}]${ext}`;
    }
    const dest = `${dir}\\${escapeWindowsName(name)}`;
    const clash = [...this.tasks.values()].find(
      (s) => s.task.kind === "download" && s.task.state !== "cancelled" && s.task.state !== "failed" &&
        (s.task.destination ?? "").toLowerCase() === dest.toLowerCase(),
    );
    if (clash) throw this.err("CONFLICT", `目标位置已有同名文件或任务：${dest}`, false, { existing_task: clash.task.id });
    const now = new Date().toISOString();
    const id = `task-${++this.taskSeq}`;
    const task: TransferTaskExt = {
      id, remote_id: e.id, kind: "download", state: "queued", name,
      transferred_bytes: "0", total_bytes: total, backend: "mock",
      destination: dest, created_at: now, updated_at: now, revision: 1,
      ...(quality_label ? { quality_label } : {}),
      ...(clip ? { clip } : {}),
      ...(group_id ? { group_id } : {}),
    };
    this.tasks.set(id, this.sim(task));
    this.emit(EVENTS.transferChanged, { task_id: id, state: "queued", revision: 1, updated_at: now });
    return id;
  }

  protected cmd_transfer_start_batch(args: CommandArgs<"transfer_start_batch">): BatchRef {
    const prior = this.idem.get(args.idempotency_key) as BatchRef | undefined;
    if (prior) return prior;
    this.remote();
    if (this.scenario === "quota_exhausted" || this.dailyExhausted()) {
      throw this.err("QUOTA_EXCEEDED", "今日下载额度已用完", false, { dimension: "downstream_daily" });
    }
    const dir = args.destination.replace(/[\\/]+$/, "");
    const out: BatchRef = { task_ids: [], skipped: [], expanded_files: 0 };
    for (const id of args.remote_ids) {
      const e = this.entries.get(id);
      if (!e) { out.skipped.push({ id, reason: "项目不存在" }); continue; }
      try {
        if (e.kind === "file") {
          out.task_ids.push(this.queueDownload(e, dir, args.options));
          continue;
        }
        if (e.id === SPECIAL.vanished) { out.skipped.push({ id, name: e.name, reason: "文件夹已不存在" }); continue; }
        const files: { entry: RemoteEntry; rel: string[] }[] = [];
        const walk = (folderId: string, rel: string[]): boolean => {
          for (const cid of this.sortedChildren(folderId)) {
            const c = this.entries.get(cid)!;
            if (c.kind === "folder") {
              if (!walk(c.id, [...rel, c.name])) return false;
            } else {
              files.push({ entry: c, rel });
              if (files.length > FOLDER_DOWNLOAD_MAX) return false;
            }
          }
          return true;
        };
        if (!walk(e.id, [e.name])) {
          out.skipped.push({ id, name: e.name, reason: `文件夹内超过 ${FOLDER_DOWNLOAD_MAX.toLocaleString("en-US")} 个文件，请分批下载` });
          continue;
        }
        if (!files.length) { out.skipped.push({ id, name: e.name, reason: "文件夹是空的" }); continue; }
        const group = `grp-${++this.reqSeq}`;
        out.group_id ??= group;
        for (const f of files) {
          const sub = [dir, ...f.rel.map(escapeWindowsName)].join("\\");
          try {
            out.task_ids.push(this.queueDownload(f.entry, sub, args.options, group));
            out.expanded_files++;
          } catch (err) {
            out.skipped.push({ id: f.entry.id, name: f.entry.name, reason: (err as ClientError).message });
          }
        }
      } catch (err) {
        out.skipped.push({ id, name: e.name, reason: (err as ClientError).message ?? String(err) });
      }
    }
    this.idem.set(args.idempotency_key, out);
    return out;
  }

  /* ---------------------------------------------------------------- */
  /* §P14 media                                                        */
  /* ---------------------------------------------------------------- */

  private isVideo(e: RemoteEntry): boolean {
    return e.kind === "file" && fileKind(e.name, e.kind, e.mime_type) === "video";
  }

  private probe(e: RemoteEntry): MediaProbe {
    const size = BigInt(e.size_bytes);
    const is4k = /2160p|4K/i.test(e.name);
    const height = is4k ? 2160 : 1080;
    const width = is4k ? 3840 : 1920;
    const bitrate = is4k ? 60_000_000 : 2_200_000;
    const duration_ms = Math.max(MIN_CLIP_MS * 5, Number((size * 8n * 1000n) / BigInt(bitrate)));
    const qualities: QualityOption[] = [{ id: "original", label: "原画", width, height, size_bytes: e.size_bytes, container: "original", readable: true }];
    // A just-uploaded lecture has no transcodes yet.
    const transcoded = !e.name.includes("第2讲");
    if (transcoded) {
      for (const t of ["2160p", "1080p", "720p", "480p"] as const) {
        if (TIER_HEIGHT[t] >= height) continue;
        const bytes = BigInt(Math.round((duration_ms / 1000) * (TIER_BITRATE[t] / 8)));
        const blocked = is4k && t === "1080p";
        qualities.push({
          id: t, label: t.toUpperCase(), width: Math.round((TIER_HEIGHT[t] * 16) / 9), height: TIER_HEIGHT[t],
          size_bytes: bytes.toString(), container: "mp4", readable: !blocked,
          ...(blocked ? { unreadable_reason: "PikPak 的 1080P 转码文件暂不可读，请选择其他画质" } : {}),
        });
      }
    }
    const mkv = /\.mkv$/i.test(e.name);
    return {
      remote_id: e.id, duration_ms, container: mkv ? "matroska" : "mp4", video_codec: is4k ? "hevc" : "h264",
      width, height, qualities,
      keyframes: mkv ? "estimated" : "indexed",
      preview: "available",
      probed_at: new Date().toISOString(),
    };
  }

  private keyframeAtOrBefore(seed: number, ms: number): number {
    let k = Math.floor(ms / GOP_MS);
    for (; k >= 0; k--) {
      const kf = k * GOP_MS + ((seed ^ Math.imul(k + 1, 2654435761)) >>> 0) % 600;
      if (kf <= ms) return kf;
    }
    return 0;
  }

  private align(e: RemoteEntry, probe: MediaProbe, quality_id: string, start_ms: number, end_ms: number): ClipAlignment {
    const tier = probe.qualities.find((q) => q.id === quality_id);
    if (!tier) throw this.err("NOT_FOUND", "PikPak 已不提供所选画质");
    if (!tier.readable) throw this.err("UNSUPPORTED", tier.unreadable_reason ?? "所选画质暂不可读");
    const s = Math.max(0, Math.min(Math.round(start_ms), probe.duration_ms - MIN_CLIP_MS));
    const en = Math.max(s + MIN_CLIP_MS, Math.min(Math.round(end_ms), probe.duration_ms));
    const kf = this.keyframeAtOrBefore(hashStr(e.id), s);
    const size = BigInt(tier.size_bytes ?? e.size_bytes);
    const est = (size * BigInt(en - kf)) / BigInt(probe.duration_ms);
    return {
      quality_id, requested_start_ms: s, start_ms: kf, end_ms: en,
      estimated_bytes: est.toString(),
      exactness: probe.keyframes === "indexed" ? "exact" : "estimated",
    };
  }

  private videoEntry(id: string): RemoteEntry {
    const e = this.entries.get(id);
    if (!e) throw this.err("NOT_FOUND", "远端文件不存在");
    if (!this.isVideo(e)) throw this.err("UNSUPPORTED", "不是视频文件");
    return e;
  }

  protected cmd_media_probe(args: CommandArgs<"media_probe">): MediaProbe {
    this.remote();
    return this.probe(this.videoEntry(args.remote_id));
  }

  protected cmd_media_align(args: CommandArgs<"media_align">): ClipAlignment {
    this.remote();
    const e = this.videoEntry(args.remote_id);
    return this.align(e, this.probe(e), args.quality_id, args.start_ms, args.end_ms);
  }

  protected cmd_media_preview_frame(args: CommandArgs<"media_preview_frame">): PreviewFrame {
    this.remote();
    const e = this.videoEntry(args.remote_id);
    const p = this.probe(e);
    const t = Math.max(0, Math.min(args.position_ms, p.duration_ms));
    const f = t / p.duration_ms;
    const hue = (hashStr(e.id) % 360 + f * 140) % 360;
    const sunX = 40 + f * 240;
    const sunY = 64 + Math.sin(f * Math.PI) * -30;
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 180" width="320" height="180">`
      + `<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hue.toFixed(0)} 45% 32%)"/><stop offset="1" stop-color="hsl(${((hue + 40) % 360).toFixed(0)} 40% 14%)"/></linearGradient></defs>`
      + `<rect width="320" height="180" fill="url(#g)"/>`
      + `<circle cx="${sunX.toFixed(1)}" cy="${sunY.toFixed(1)}" r="16" fill="hsl(${((hue + 180) % 360).toFixed(0)} 70% 70%)" opacity="0.85"/>`
      + `<path d="M0 132 L60 108 L118 126 L182 96 L250 122 L320 104 L320 180 L0 180 Z" fill="hsl(${hue.toFixed(0)} 30% 9%)" opacity="0.9"/>`
      + `<text x="160" y="94" text-anchor="middle" font-family="ui-monospace, Consolas, monospace" font-size="30" fill="#fff" opacity="0.92">${formatClock(t)}</text>`
      + `<text x="10" y="170" font-family="system-ui, sans-serif" font-size="11" fill="#fff" opacity="0.7">演示画面</text>`
      + `</svg>`;
    const b64 = typeof btoa === "function"
      ? btoa(unescape(encodeURIComponent(svg)))
      : (globalThis as unknown as { Buffer: { from(s: string): { toString(enc: string): string } } }).Buffer.from(svg).toString("base64");
    return { position_ms: t, mime: "image/svg+xml", data_base64: b64, synthetic: true };
  }

  protected cmd_media_open_external(args: CommandArgs<"media_open_external">): { launched: boolean; note: string } {
    this.remote();
    const e = this.entries.get(args.remote_id);
    if (!e) throw this.err("NOT_FOUND", "远端文件不存在");
    const k = fileKind(e.name, e.kind, e.mime_type);
    if (k !== "video" && k !== "audio") throw this.err("UNSUPPORTED", "只有音视频可以用外部播放器打开");
    return { launched: false, note: "Mock 模式不会启动播放器。接入真实后端后，由核心把播放流交给系统默认播放器，链接不经过界面。" };
  }

  /* ---------------------------------------------------------------- */
  /* §P16 starred                                                      */
  /* ---------------------------------------------------------------- */

  protected cmd_files_starred(args: CommandArgs<"files_starred">): Page<RemoteEntry> {
    this.remote();
    const items = [...this.entries.values()].filter((e) => (e as EntryExt).starred)
      .sort((a, b) => (b.modified_at ?? "").localeCompare(a.modified_at ?? ""));
    return this.page(items, args.cursor, args.limit);
  }

  /* ---------------------------------------------------------------- */
  /* §P17 duplicates                                                   */
  /* ---------------------------------------------------------------- */

  private pathOf(id: string): string {
    if (id === ROOT_ID) return "/";
    return `/${this.cmd_files_path({ id }).map((e) => e.name).join("/")}`;
  }

  protected cmd_duplicates_scan(args: CommandArgs<"duplicates_scan">): { scan_id: string } {
    this.remote();
    const root = args.folder_id || ROOT_ID;
    if (root !== ROOT_ID && this.entries.get(root)?.kind !== "folder") throw this.err("NOT_FOUND", "文件夹不存在");
    const scan_id = `dup-${++this.reqSeq}`;
    const groups = new Map<string, DuplicateEntry[]>();
    const skipped: DuplicateReport["skipped"] = [];
    let files = 0;
    let folders = 0;
    let unhashed = 0;
    const stack = [root];
    while (stack.length) {
      const fid = stack.pop()!;
      if (fid === SPECIAL.vanished) { skipped.push({ path: this.pathOf(fid), reason: "文件夹已不存在" }); continue; }
      folders++;
      for (const cid of this.children.get(fid) ?? []) {
        const c = this.entries.get(cid);
        if (!c) continue;
        if (c.kind === "folder") { stack.push(c.id); continue; }
        files++;
        if (!c.hash) { unhashed++; continue; }
        const key = `${c.hash.algorithm}:${c.hash.value}|${c.size_bytes}`;
        let g = groups.get(key);
        if (!g) groups.set(key, (g = []));
        g.push({ ...(c as EntryExt), path: `${this.pathOf(fid).replace(/\/$/, "")}/${c.name}`, ...(this.readOnly(c.parent_id) ? { read_only: true } : {}) });
      }
    }
    const out: DuplicateGroup[] = [];
    let reclaim = 0n;
    for (const [key, entries] of groups) {
      if (entries.length < 2) continue;
      entries.sort((a, b) => (a.modified_at ?? "").localeCompare(b.modified_at ?? "") || a.path.length - b.path.length);
      reclaim += BigInt(entries[0].size_bytes) * BigInt(entries.length - 1);
      out.push({ group_id: `g-${hashStr(key).toString(16)}`, match: "hash", hash: entries[0].hash, size_bytes: entries[0].size_bytes, entries });
    }
    out.sort((a, b) => {
      const x = BigInt(a.size_bytes) * BigInt(a.entries.length - 1);
      const y = BigInt(b.size_bytes) * BigInt(b.entries.length - 1);
      return x < y ? 1 : x > y ? -1 : 0;
    });
    const report: DuplicateReport = {
      scan_id, folder_id: root, folder_path: this.pathOf(root), state: "running", groups: out,
      files_scanned: 0, folders_scanned: 0, reclaimable_bytes: reclaim.toString(), unhashed_files: unhashed, skipped,
    };
    this.dupScans.set(scan_id, { report, ticks: 0, files, folders });
    return { scan_id };
  }

  protected cmd_duplicates_report(args: CommandArgs<"duplicates_report">): DuplicateReport {
    const sc = this.dupScans.get(args.scan_id);
    if (!sc) throw this.err("NOT_FOUND", "扫描不存在或已过期");
    const r = sc.report;
    if (r.state !== "completed") return { ...r, groups: [], reclaimable_bytes: "0" };
    // Re-validate against the live tree: trashed/moved copies drop out.
    const groups = r.groups
      .map((g) => ({ ...g, entries: g.entries.filter((e) => this.entries.has(e.id)) }))
      .filter((g) => g.entries.length > 1);
    const reclaim = groups.reduce((s, g) => s + BigInt(g.size_bytes) * BigInt(g.entries.length - 1), 0n);
    return { ...r, groups, reclaimable_bytes: reclaim.toString() };
  }

  protected cmd_duplicates_cancel(args: CommandArgs<"duplicates_cancel">): void {
    const sc = this.dupScans.get(args.scan_id);
    if (!sc || sc.report.state !== "running") return;
    sc.report.state = "cancelled";
    this.emit(PROPOSED_EVENTS.duplicatesProgress, { scan_id: args.scan_id, state: "cancelled", files_scanned: sc.report.files_scanned, folders_scanned: sc.report.folders_scanned });
  }

  /* ---------------------------------------------------------------- */
  /* §P18 space                                                        */
  /* ---------------------------------------------------------------- */

  protected cmd_files_space_summary(args: CommandArgs<"files_space_summary">): SpaceSummary {
    this.remote();
    const root = args.folder_id || ROOT_ID;
    if (root !== ROOT_ID && this.entries.get(root)?.kind !== "folder") throw this.err("NOT_FOUND", "文件夹不存在");
    let unreadable = 0;
    const measure = (id: string): { bytes: bigint; files: number; folders: number } => {
      if (id === SPECIAL.vanished) { unreadable++; return { bytes: 0n, files: 0, folders: 0 }; }
      let bytes = 0n;
      let files = 0;
      let folders = 0;
      for (const cid of this.children.get(id) ?? []) {
        const c = this.entries.get(cid)!;
        if (c.kind === "folder") {
          const m = measure(c.id);
          bytes += m.bytes;
          files += m.files;
          folders += m.folders + 1;
        } else {
          bytes += BigInt(c.size_bytes);
          files++;
        }
      }
      return { bytes, files, folders };
    };
    const children: SpaceNode[] = [];
    let total = 0n;
    let fileCount = 0;
    let folderCount = 0;
    for (const cid of this.sortedChildren(root)) {
      const c = this.entries.get(cid)!;
      if (c.kind === "folder") {
        const m = measure(c.id);
        children.push({ id: c.id, name: c.name, kind: "folder", size_bytes: m.bytes.toString(), file_count: m.files });
        total += m.bytes;
        fileCount += m.files;
        folderCount += m.folders + 1;
      } else {
        children.push({ id: c.id, name: c.name, kind: "file", size_bytes: c.size_bytes, file_count: 1, ...(c.mime_type ? { mime_type: c.mime_type } : {}) });
        total += BigInt(c.size_bytes);
        fileCount++;
      }
    }
    return {
      folder_id: root, total_bytes: total.toString(), file_count: fileCount, folder_count: folderCount, children,
      exactness: unreadable ? "partial" : "exact",
      ...(unreadable ? { note: `${unreadable} 个文件夹无法读取，大小为下限` } : {}),
      computed_at: new Date().toISOString(),
    };
  }

  /* ---------------------------------------------------------------- */
  /* §P19 archives + jobs                                              */
  /* ---------------------------------------------------------------- */

  private archiveOf(file_id: string) {
    const e = this.entries.get(file_id);
    if (!e || e.kind !== "file") throw this.err("NOT_FOUND", "压缩包不存在");
    return { e, fx: ARCHIVES[e.name] };
  }

  protected cmd_archive_list(args: CommandArgs<"archive_list">): ArchiveListing {
    this.remote();
    const { e, fx } = this.archiveOf(args.file_id);
    const ext = splitName(e.name).ext.slice(1).toLowerCase();
    const format = ext === "zip" || ext === "rar" || ext === "7z" ? ext : "unknown";
    const path = (args.path ?? "").replace(/^\/+|\/+$/g, "");
    const base: ArchiveListing = { file_id: e.id, path, format, supported: true, encrypted: false, needs_password: false, entries: [] };
    if (!fx) return { ...base, supported: false, reason: format === "unknown" ? "不是受支持的压缩包格式" : "服务端无法读取此压缩包的目录" };
    if (fx.multiVolume) return { ...base, supported: false, reason: "分卷压缩包不支持在线浏览与解压，请下载全部分卷后在本地解压" };
    if (fx.encrypted && (!args.password_ref || this.passwordRefs.get(args.password_ref) !== e.id)) {
      return { ...base, encrypted: true, needs_password: true };
    }
    const prefix = path ? `${path}/` : "";
    const seen = new Map<string, ArchiveEntry>();
    for (const f of fx.files) {
      if (!f.path.startsWith(prefix)) continue;
      const rest = f.path.slice(prefix.length);
      const cut = rest.indexOf("/");
      if (cut < 0) {
        seen.set(rest, { path: f.path, name: rest, kind: "file", size_bytes: f.size.toString(), modified_at: e.modified_at });
      } else {
        const name = rest.slice(0, cut);
        const prev = seen.get(name);
        const size = (prev ? BigInt(prev.size_bytes) : 0n) + f.size;
        seen.set(name, { path: `${prefix}${name}`, name, kind: "folder", size_bytes: size.toString() });
      }
    }
    const entries = [...seen.values()].sort((a, b) => (a.kind !== b.kind ? (a.kind === "folder" ? -1 : 1) : a.name.localeCompare(b.name, "zh-CN", { numeric: true })));
    return { ...base, encrypted: fx.encrypted, entries };
  }

  protected cmd_archive_unlock(args: CommandArgs<"archive_unlock">): { password_ref: string } {
    this.remote();
    const { e, fx } = this.archiveOf(args.file_id);
    if (!fx?.encrypted) throw this.err("CONFLICT", "此压缩包没有加密");
    if (args.password !== fx.password) throw this.err("PERMISSION_DENIED", "密码不正确", false, { reason: "wrong_password" });
    const ref = `pw-${++this.reqSeq}`;
    this.passwordRefs.set(ref, e.id);
    return { password_ref: ref };
  }

  protected cmd_archive_extract(args: CommandArgs<"archive_extract">): { job_id: string } {
    const prior = this.idem.get(args.idempotency_key) as { job_id: string } | undefined;
    if (prior) return prior;
    this.remote();
    const { e, fx } = this.archiveOf(args.file_id);
    if (!fx || fx.multiVolume) throw this.err("UNSUPPORTED", "此压缩包不支持云端解压");
    if (fx.encrypted && (!args.password_ref || this.passwordRefs.get(args.password_ref) !== e.id)) throw this.err("PERMISSION_DENIED", "需要正确的解压密码");
    const parent = args.target_parent_id || ROOT_ID;
    if (this.readOnly(parent) || parent === SPECIAL.readOnlyShare) throw this.err("PERMISSION_DENIED", "目标文件夹为只读共享");
    if (parent !== ROOT_ID && this.entries.get(parent)?.kind !== "folder") throw this.err("NOT_FOUND", "目标文件夹不存在");
    if (Array.isArray(args.paths) && !args.paths.length) throw this.err("CONFLICT", "没有选择要解压的项目");
    const now = new Date().toISOString();
    const job_id = `job-x-${++this.reqSeq}`;
    const job = {
      job_id, kind: "archive_extract" as const, state: "queued" as const, title: `解压“${e.name}”`,
      progress: 0, target_parent_id: parent, target_path: this.pathOf(parent),
      created_at: now, updated_at: now, revision: 1, ticks: 0, file_id: e.id, paths: args.paths,
    };
    this.serverJobs.set(job_id, job);
    const { ticks: _t, file_id: _f, paths: _p, ...row } = job;
    this.emit(PROPOSED_EVENTS.jobChanged, row);
    const ref = { job_id };
    this.idem.set(args.idempotency_key, ref);
    return ref;
  }

  protected cmd_jobs_list(args: CommandArgs<"jobs_list">): Page<ServerJob> {
    const rows = [...this.serverJobs.values()].map(({ ticks: _t, file_id: _f, paths: _p, ...row }) => row)
      .sort((a, b) => b.created_at.localeCompare(a.created_at));
    return this.page(rows, args.cursor, args.limit);
  }

  /* ---------------------------------------------------------------- */
  /* §P20 link preview                                                 */
  /* ---------------------------------------------------------------- */

  protected cmd_cloud_task_preview(args: CommandArgs<"cloud_task_preview">): LinkPreview {
    this.remote();
    const src = args.source_url.trim();
    const isMagnet = /^magnet:\?xt=urn:btih:[a-z0-9]{32,40}/i.test(src);
    const isHttp = /^https?:\/\/[^\s/]+\.[^\s]+/i.test(src);
    if (!isMagnet && !isHttp) throw this.err("UNSUPPORTED", "不支持的链接格式：仅支持 magnet: 与 http(s) 链接", false, { reason: "invalid_source" });
    let title: string;
    if (isMagnet) {
      const dn = /[?&]dn=([^&]+)/i.exec(src);
      title = dn ? decodeURIComponent(dn[1].replace(/\+/g, " ")) : "未命名资源";
    } else {
      try {
        const u = new URL(src);
        title = decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() ?? u.hostname);
      } catch {
        title = "URL 资源";
      }
    }
    const files = previewFilesFor(title, isMagnet ? "magnet" : "http").map((f, index) => ({ index, path: f.path, size_bytes: f.size.toString() }));
    const total = files.reduce((s, f) => s + BigInt(f.size_bytes), 0n);
    const pv: LinkPreview & { source: string } = {
      preview_id: `pv-${++this.reqSeq}`, source_kind: isMagnet ? "magnet" : "http", title, total_bytes: total.toString(), files,
      expires_at: new Date(Date.now() + 15 * 60_000).toISOString(), source: src,
    };
    this.previews.set(pv.preview_id, pv);
    const { source: _s, ...out } = pv;
    return out;
  }

  /* ---------------------------------------------------------------- */
  /* §P22 shares                                                       */
  /* ---------------------------------------------------------------- */

  protected cmd_share_create(args: CommandArgs<"share_create">): ShareInfo {
    const prior = this.idem.get(args.idempotency_key) as ShareInfo | undefined;
    if (prior) return prior;
    this.remote();
    if (!args.ids.length) throw this.err("CONFLICT", "没有选择要分享的项目");
    const items = args.ids.map((id) => this.entries.get(id));
    if (items.some((e) => !e)) throw this.err("NOT_FOUND", "部分项目已不存在");
    if (items.some((e) => e!.id === SPECIAL.readOnlyShare || this.readOnly(e!.parent_id))) throw this.err("PERMISSION_DENIED", "共享给你的项目不能再次分享");
    const now = Date.now();
    const id = `sh-${++this.reqSeq}`;
    const code = args.with_code ? hashStr(id).toString(36).slice(0, 4).padEnd(4, "x") : undefined;
    const sh: ShareInfo = {
      share_id: id,
      title: items.length === 1 ? items[0]!.name : `${items[0]!.name} 等 ${items.length} 项`,
      item_count: items.length,
      url: `https://mock.invalid/s/VN-${hashStr(id + now).toString(36)}`,
      ...(code ? { code } : {}),
      created_at: new Date(now).toISOString(),
      ...(args.expires_in_days ? { expires_at: new Date(now + args.expires_in_days * 86_400_000).toISOString() } : {}),
      state: "active", view_count: 0, save_count: 0,
    };
    this.shares.set(id, sh);
    this.idem.set(args.idempotency_key, sh);
    return sh;
  }

  protected cmd_shares_list(args: CommandArgs<"shares_list">): Page<ShareInfo> {
    this.remote();
    const rows = [...this.shares.values()].sort((a, b) => b.created_at.localeCompare(a.created_at));
    return this.page(rows, args.cursor, args.limit);
  }

  protected cmd_shares_cancel(args: CommandArgs<"shares_cancel">): BulkResult {
    const prior = this.idem.get(args.idempotency_key) as BulkResult | undefined;
    if (prior) return { ...prior, replayed: true };
    this.remote();
    const res: BulkResult = { succeeded: [], failed: [], replayed: false };
    for (const id of args.share_ids) {
      const sh = this.shares.get(id);
      if (!sh) { res.failed.push({ id, error: this.err("NOT_FOUND", "分享不存在") }); continue; }
      sh.state = "cancelled";
      res.succeeded.push(id);
    }
    this.idem.set(args.idempotency_key, res);
    return res;
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
            { id: "backend_list", label: "已登录并可读取云盘目录", status: ready ? "pass" : "fail", detail: ready ? "可以列出目录" : "需要先登录" },
            { id: "provider", label: "同步文件夹组件", status: "fail", detail: "暂不支持" },
          ],
        },
        {
          mode: "winfsp_drive",
          available: false,
          checks: [
            { id: "winfsp", label: "WinFsp 运行时", status: "fail", detail: "未检测到 WinFsp", requirement: "安装 WinFsp 2.x 后重新检测。" },
            { id: "read_range", label: "按需读取文件片段", status: "fail", detail: "暂不支持" },
            { id: "driver", label: "只读挂载", status: "fail", detail: "暂不支持" },
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
    const c = p.download?.connections_per_task;
    if (c !== undefined && (!Number.isInteger(c) || c < 1 || c > 16)) throw this.err("CONFLICT", "每个任务的连接数需在 1–16 之间");
    const q = p.download?.default_quality;
    if (q !== undefined && !["original", "2160p", "1080p", "720p", "480p"].includes(q)) throw this.err("CONFLICT", "未知的画质档位");
    if ((p.experimental_network as { custom_dns_mode?: boolean } | undefined)?.custom_dns_mode) {
      throw this.err("UNSUPPORTED", "自定义 DNS 尚未审计，不能开启");
    }
    this.settings = {
      ...this.settings,
      ...p,
      schema_version: 1,
      telemetry: false,
      cache: { ...this.settings.cache, ...(p.cache ?? {}) },
      download: { ...this.settings.download!, ...(p.download ?? {}) },
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
