import { useMemo, useState } from "preact/hooks";
import type { TaskState } from "../lib/contract";
import type { TransferAction, TransferTaskExt } from "../lib/contract-proposals";
import { ACTIVE_STATES, ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { ERROR_COPY, diagnosticsText } from "../lib/errors";
import { formatBytes, formatDateTime, formatDuration, formatEta, formatRelative, formatSpeed, groupDigits, parseU64, percent } from "../lib/format";
import { splitName } from "../lib/names";
import { useStore } from "../lib/store";
import { app, gate, updateSettings } from "../state/app";
import { navigate } from "../state/route";
import { loadSnapshot, transfers } from "../state/transfers";
import { openMenu, toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Btn, copyWithToast, Empty, ErrorPanel, Progress, Spinner } from "../components/ui";
import { PageFrame } from "../components/Frame";
import { newCloudTask } from "../components/dialogs";
import { useTicker } from "./common";

const STATE_LABEL: Record<TaskState, string> = {
  queued: "排队中", running: "进行中", paused: "已暂停", retrying: "重试等待",
  verifying: "校验中", completed: "已完成", failed: "失败", cancelled: "已取消",
};

const KIND_LABEL = { download: "下载", upload: "上传", cloud_download: "云下载", segment_download: "视频片段" } as const;
type Kind = TransferTaskExt["kind"];
type Category = "all" | Kind;

function stateText(t: TransferTaskExt): string {
  if (t.state !== "running") return STATE_LABEL[t.state];
  return t.kind === "download" ? "下载中" : t.kind === "upload" ? "上传中" : "云端下载中";
}

async function control(t: TransferTaskExt, action: TransferAction): Promise<void> {
  try {
    await api.transferControl(t.id, action);
  } catch (e) {
    const err = errorOf(e);
    toast(`${ERROR_COPY[err.code].title}：${err.message}`, "error", { error: err });
  }
}

function TaskActions({ t }: { t: TransferTaskExt }) {
  const desktop = useStore(app, s => s.runtime === "tauri");
  const cloud = t.kind === "cloud_download";
  const active = ACTIVE_STATES.has(t.state);
  const g = gate(cloud ? "cloud_task" : t.kind === "upload" ? "upload" : "download");
  const primary = !cloud && (t.state === "running" || t.state === "queued" || t.state === "retrying")
    ? <Btn tone="quiet" iconOnly icon="pause" label="暂停" onClick={() => void control(t, "pause")} />
    : t.state === "paused"
      ? <Btn tone="quiet" iconOnly icon="play" label="继续" onClick={() => void control(t, "resume")} />
      : t.state === "failed" || (t.state === "cancelled" && !desktop)
        ? <Btn tone="quiet" iconOnly icon="retry" label="重试" disabled={t.error?.code === "QUOTA_EXCEEDED" && !g.ok} reason="配额已用尽，重试不会成功；切换后端也不会恢复配额" onClick={() => void control(t, "retry")} />
        : <span class="act-slot" />;
  return (
    <div class="task-actions">
      {primary}
      <Btn tone="quiet" iconOnly icon="more" label="更多操作"
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(r.right - 240, r.bottom + 4, [
            { id: "cancel", label: "取消任务", icon: "x", disabled: !active || t.state === "verifying", reason: t.state === "verifying" ? "校验中不能取消" : "任务已结束", run: () => void control(t, "cancel") },
            { id: "open", label: cloud ? "在网盘中查看" : "打开所在文件夹", icon: "folder", disabled: !cloud || t.state !== "completed", reason: cloud ? "完成后可查看" : "需要后端提供“在资源管理器中显示”能力（提案 UI-001 §P9）", run: () => navigate({ page: "files", folder: ROOT_ID }) },
            { id: "diag", label: "复制诊断信息", icon: "copy", separatorBefore: true, disabled: !t.error, reason: "任务没有错误", run: () => t.error && void copyWithToast(diagnosticsText(t.error, `task=${t.id}`), "诊断信息") },
          ], t.name ?? t.id);
        }} />
    </div>
  );
}

export function TaskRow({ t, now }: { t: TransferTaskExt; now: number }) {
  const pct = percent(t.transferred_bytes, t.total_bytes);
  const eta = t.state === "running" ? formatEta(t.transferred_bytes, t.total_bytes, t.speed_bytes_per_sec) : null;
  const name = t.name ?? t.remote_id;
  const { base, ext } = splitName(name);
  const tone = t.state === "failed" ? "error" : t.state === "retrying" ? "warn" : t.state === "completed" ? "ok" : t.state === "paused" || t.state === "cancelled" || t.state === "queued" ? "muted" : undefined;
  const kindIcon = t.kind === "upload" ? "upload" : t.kind === "cloud_download" ? "cloudDown" : "download";
  return (
    <div class={`task state-${t.state}`} role="listitem" data-task={t.id} aria-label={`${KIND_LABEL[t.kind]} ${name}，${stateText(t)}`}>
      <span class={`tile kind-${t.kind === "cloud_download" ? "code" : t.kind === "upload" ? "image" : "doc"} task-kind`} title={KIND_LABEL[t.kind]}>
        <Icon name={kindIcon} size={20} />
      </span>
      <div class="task-main">
        <span class="name-text task-name" dir="auto" title={name}>
          <span class="name-base">{base}</span>{ext ? <span class="name-ext">{ext}</span> : null}
        </span>
        {t.error ? (
          <span class={`task-status task-error code-${t.error.code.toLowerCase()}`} title={t.error.message}>
            <strong>{stateText(t)}</strong>
            <span class="ellipsis">{ERROR_COPY[t.error.code].title}：{t.error.message}</span>
          </span>
        ) : (
          <span class={`task-status st-${t.state}`}>
            <span>{stateText(t)}</span>
            {t.state === "verifying" ? <span>{t.simulated ? "模拟校验阶段" : "正在校验"}</span> : null}
            <span class="dim" title={formatDateTime(t.updated_at)}>{formatRelative(t.updated_at, now)}</span>
          </span>
        )}
      </div>
      <div class="task-meter">
        <Progress value={t.state === "completed" ? 100 : pct} tone={tone} label={`${name} 进度`} />
        <span class="tabular">{t.state === "completed" ? "100%" : pct !== null ? `${pct.toFixed(1)}%` : "—"}</span>
      </div>
      <span class="task-size tabular">
        {formatBytes(t.transferred_bytes)} / {t.total_bytes ? formatBytes(t.total_bytes) : "总大小未提供"}
      </span>
      <span class="task-speed tabular">
        {t.state === "running" ? (t.kind === "cloud_download" ? "速率未提供" : formatSpeed(t.speed_bytes_per_sec)) : ""}
        {eta ? <span class="dim">剩余 {eta}</span> : null}
      </span>
      <span class="task-dest dim" dir="auto" title={t.destination}>{t.destination ?? ""}</span>
      <TaskActions t={t} />
    </div>
  );
}

const SECTIONS: { id: string; label: string; states: ReadonlySet<TaskState>; collapsed?: boolean }[] = [
  { id: "attention", label: "需要处理", states: new Set<TaskState>(["failed"]) },
  { id: "active", label: "进行中", states: ACTIVE_STATES },
  { id: "done", label: "已完成", states: new Set<TaskState>(["completed"]) },
  { id: "cancelled", label: "已取消", states: new Set<TaskState>(["cancelled"]), collapsed: true },
];

function Section({ label, tasks, now, initiallyCollapsed }: { label: string; tasks: TransferTaskExt[]; now: number; initiallyCollapsed?: boolean }) {
  const [open, setOpen] = useState(!initiallyCollapsed);
  const [limit, setLimit] = useState(200);
  if (!tasks.length) return null;
  return (
    <section class="task-section" aria-label={label}>
      <button type="button" class="task-section-head" aria-expanded={open ? "true" : "false"} onClick={() => setOpen(!open)}>
        <span>{label}</span>
        <span class="section-count">{groupDigits(String(tasks.length))}</span>
        <Icon name={open ? "chevronUp" : "chevronDown"} size={20} class="push" />
      </button>
      {open ? (
        <div role="list" class="task-list">
          {tasks.slice(0, limit).map((t) => <TaskRow key={t.id} t={t} now={now} />)}
          {tasks.length > limit ? <button type="button" class="link-btn more-btn" onClick={() => setLimit(limit + 500)}>显示更多（还有 {groupDigits(String(tasks.length - limit))} 项）</button> : null}
        </div>
      ) : null}
    </section>
  );
}

const LIMITS = [0, 1024, 5 * 1024, 10 * 1024];

function TransfersFooter({ tasks }: { tasks: TransferTaskExt[] }) {
  const s = useStore(app, (x) => x);
  let down = 0;
  let up = 0;
  let remaining = 0n;
  for (const t of tasks) {
    if (t.state !== "running" || typeof t.speed_bytes_per_sec !== "number") continue;
    if (t.kind === "download") {
      down += t.speed_bytes_per_sec;
      const all = parseU64(t.total_bytes ?? null);
      const cur = parseU64(t.transferred_bytes);
      if (all !== null && cur !== null && all > cur) remaining += all - cur;
    } else if (t.kind === "upload") up += t.speed_bytes_per_sec;
  }
  const limit = s.settings?.bandwidth_limit_kib_s ?? 0;
  const daily = s.quota?.items.find((i) => i.dimension === "downstream_daily");
  const monthly = s.quota?.items.find((i) => i.dimension === "downstream_monthly");
  const q = daily && daily.exactness !== "unavailable" ? daily : monthly && monthly.exactness !== "unavailable" ? monthly : undefined;
  return (
    <>
      <span class="foot-speed tabular" aria-label="当前速率">
        <Icon name="download" size={18} /><strong>{formatSpeed(down)}</strong>
        <Icon name="upload" size={18} /><strong>{formatSpeed(up)}</strong>
      </span>
      <button type="button" disabled={s.runtime === "tauri"} class={`chip${limit > 0 ? " on" : ""}`} aria-pressed={limit > 0 ? "true" : "false"} title={s.runtime === "tauri" ? "限速设置后端尚未实现" : "限制 NekoPak 自身的传输带宽"}
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(r.left, r.top - 8 - 4 * 36, LIMITS.map((v) => ({
            id: String(v), label: v === 0 ? "不限速" : `${v / 1024} MB/s`, icon: v === limit ? "check" : undefined,
            run: () => void updateSettings({ bandwidth_limit_kib_s: v }).then(() => toast(v === 0 ? "已取消限速" : `已限速 ${v / 1024} MB/s`, "success"), (er) => toast(errorOf(er).message, "error")),
          })), "限速");
        }}>
        <Icon name="speed" size={18} />{s.runtime === "tauri" ? "限速未启用" : limit > 0 ? `限速 ${(limit / 1024).toFixed(limit % 1024 ? 1 : 0)} MB/s` : "不限速"}
      </button>
      <span class="push" />
      {down > 0 && remaining > 0n ? <span class="dim foot-eta">下载剩余约 {formatDuration(Number(remaining) / down)}</span> : null}
      {q ? (
        <span class="foot-meter" title={q.note}>
          <span class="dim">{q.dimension === "downstream_daily" ? "今日下行" : "本月下行"}</span>
          <Progress value={percent(q.used, q.limit)} tone={q.state === "exhausted" ? "error" : q.state === "near_limit" ? "warn" : undefined} label="下行配额已用" />
          <span class="tabular dim">{formatBytes(q.used)} / {q.limit ? formatBytes(q.limit) : "?"}</span>
        </span>
      ) : <span class="dim">配额不可获取</span>}
    </>
  );
}

export function TransfersPage({ kind }: { tab?: string; kind?: Kind }) {
  const st = useStore(transfers, (s) => s);
  const [cat, setCat] = useState<Category>(kind ?? "all");
  const [busy, setBusy] = useState(false);
  const tick = useTicker(15_000);
  const g = gate("cloud_task");

  const all = useMemo(() => st.order.map((id) => st.tasks.get(id)!).filter(Boolean), [st.order, st.tasks]);
  const now = useMemo(() => Date.now(), [tick, all]);
  const counts = useMemo(() => {
    const c: Record<Category, number> = { all: all.length, download: 0, upload: 0, cloud_download: 0, segment_download: 0 };
    for (const t of all) c[t.kind]++;
    return c;
  }, [all]);
  const shown = all.filter((t) => cat === "all" || t.kind === cat);
  const pausable = shown.filter((t) => t.kind !== "cloud_download" && (t.state === "running" || t.state === "queued" || t.state === "retrying"));
  const resumable = shown.filter((t) => t.state === "paused");

  const bulk = async (list: TransferTaskExt[], action: TransferAction) => {
    setBusy(true);
    let fail = 0;
    for (const t of list) {
      try {
        await api.transferControl(t.id, action);
      } catch {
        fail++;
      }
    }
    setBusy(false);
    toast(fail ? `${list.length - fail} 项成功，${fail} 项失败` : `已${action === "pause" ? "暂停" : "继续"} ${list.length} 项`, fail ? "warning" : "success");
  };

  const cats: { id: Category; label: string }[] = [
    { id: "all", label: "全部" }, { id: "download", label: "下载" }, { id: "upload", label: "上传" }, { id: "cloud_download", label: "云下载" },
  ];

  return (
    <PageFrame
      label="任务"
      side
      caption={<>
        <h1 id="page-title" class="sr-only">任务</h1>
        <div class="pill-tabs" role="tablist" aria-label="任务类别">
          {cats.map((c) => (
            <button type="button" role="tab" aria-selected={cat === c.id ? "true" : "false"} class={`pill-tab${cat === c.id ? " on" : ""}`} onClick={() => setCat(c.id)}>
              {c.label}<span class="pill-n">{counts[c.id]}</span>
            </button>
          ))}
        </div>
      </>}
      commands={<>
        <div class="cmd-group">
          <Btn tone="ghost" icon="play" label="全部继续" busy={busy} disabled={!resumable.length} reason="没有已暂停的任务" onClick={() => void bulk(resumable, "resume")} />
          <Btn tone="ghost" icon="pause" label="全部暂停" busy={busy} disabled={!pausable.length} reason="没有可暂停的本地任务" onClick={() => void bulk(pausable, "pause")} />
          <Btn tone="ghost" icon="trashSweep" label="清除已完成" disabled reason="契约中尚无清理任务历史的命令（提案 UI-001 §P9）" />
          <Btn tone="quiet" iconOnly icon="refresh" label="重新读取任务快照" busy={st.phase === "loading"} onClick={() => void loadSnapshot()} />
        </div>
        <div class="cmd-group cmd-end">
          <span class="cmd-meta" title={st.snapshot_at}>
            {st.snapshot_at ? `快照读取于 ${formatRelative(st.snapshot_at)}` : ""}
            {st.dropped_events ? `，已丢弃 ${groupDigits(String(st.dropped_events))} 条过期事件` : ""}
          </span>
          <Btn tone="primary" icon="bolt" label="添加链接" disabled={!g.ok} reason={g.reason} onClick={() => void newCloudTask(ROOT_ID, "/")} />
        </div>
      </>}
      footer={<TransfersFooter tasks={all} />}
    >
      {st.phase === "error" && st.error ? (
        <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={() => void loadSnapshot()} context="transfers_list" /></div>
      ) : st.phase === "loading" || st.phase === "idle" ? (
        <div class="canvas-pad"><Spinner label="正在读取任务快照" /></div>
      ) : shown.length === 0 ? (
        <Empty title={cat === "all" ? "还没有任务" : `没有${cats.find((c) => c.id === cat)!.label}任务`}>在网盘中选择文件下载，或点“添加链接”让服务端保存磁力或 http(s) 资源。</Empty>
      ) : (
        <div class="task-scroll">
          {SECTIONS.map((sec) => (
            <Section key={`${sec.id}-${cat}`} label={sec.label} now={now} initiallyCollapsed={sec.collapsed} tasks={shown.filter((t) => sec.states.has(t.state))} />
          ))}
          <p class="task-note dim">任务状态以 Rust 核心持久化的快照为准，进度事件只做增量更新。</p>
        </div>
      )}
    </PageFrame>
  );
}
