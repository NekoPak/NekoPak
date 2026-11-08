import { tr, tx } from "../lib/i18n";
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
import { Btn, copyWithToast, Empty, ErrorPanel, Progress, Spinner, VerificationRecovery } from "../components/ui";
import { PageFrame } from "../components/Frame";
import { newCloudTask } from "../components/dialogs";
import { useTicker } from "./common";
import { formatClock } from "../lib/clip";
import { jobs } from "../state/jobs";
import type { ServerJob } from "../lib/contract-proposals";

const STATE_LABEL: Record<TaskState, string> = {
  get queued() { return tr("排队中"); }, get running() { return tr("进行中"); }, get paused() { return tr("已暂停"); }, get retrying() { return tr("重试等待"); },
  get verifying() { return tr("校验中"); }, get completed() { return tr("已完成"); }, get failed() { return tr("失败"); }, get cancelled() { return tr("已取消"); },
};

const KIND_LABEL = { get download() { return tr("下载"); }, get upload() { return tr("上传"); }, get cloud_download() { return tr("云下载"); }, get segment_download() { return tr("视频片段"); } } as const;
type Kind = TransferTaskExt["kind"];
type Category = "all" | Kind;

function stateText(t: TransferTaskExt): string {
  if (t.state !== "running") return STATE_LABEL[t.state];
  return t.kind === "download" ? tr("下载中") : t.kind === "upload" ? tr("上传中") : t.kind === "segment_download" ? tr("正在转封装") : tr("云端下载中");
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
    ? <Btn tone="quiet" iconOnly icon="pause" label={tr("暂停")} onClick={() => void control(t, "pause")} />
    : !cloud && t.state === "paused"
      ? <Btn tone="quiet" iconOnly icon="play" label={tr("继续")} onClick={() => void control(t, "resume")} />
      : t.state === "failed" || (t.state === "cancelled" && !desktop)
        ? <Btn tone="quiet" iconOnly icon="retry" label={tr("重试")} disabled={t.error?.code === "QUOTA_EXCEEDED" && !g.ok} reason={tr("配额已用尽，重试不会成功，等待配额重置后再试")} onClick={() => void control(t, "retry")} />
        : <span class="act-slot" />;
  return (
    <div class="task-actions">
      {primary}
      <Btn tone="quiet" iconOnly icon="more" label={tr("更多操作")}
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(r.right - 240, r.bottom + 4, [
            { id: "cancel", label: tr("取消任务"), icon: "x", disabled: !active, reason: tr("任务已结束"), run: () => void control(t, "cancel") },
            { id: "open", label: cloud ? tr("在网盘中查看") : tr("打开所在文件夹"), icon: "folder", disabled: cloud ? t.state !== "completed" : !desktop || t.simulated || !t.destination, reason: cloud ? tr("完成后可查看") : tr("此任务没有本地下载目录"), run: () => cloud ? navigate({ page: "files", folder: ROOT_ID }) : void api.transferOpenDirectory(t.id).catch(e=>toast(errorOf(e).message,"error")) },
            { id: "diag", label: tr("复制诊断信息"), icon: "copy", separatorBefore: true, disabled: !t.error, reason: tr("任务没有错误"), run: () => t.error && void copyWithToast(diagnosticsText(t.error, `task=${t.id}`), tr("诊断信息")) },
          ], t.name ?? t.id);
        }} />
    </div>
  );
}

export function TaskRow({ t, now }: { t: TransferTaskExt; now: number }) {
  const pct = t.kind === "cloud_download" && t.progress_percent !== undefined ? Math.max(0, Math.min(100, t.progress_percent)) : percent(t.transferred_bytes, t.total_bytes);
  const eta = t.state === "running" ? formatEta(t.transferred_bytes, t.total_bytes, t.speed_bytes_per_sec) : null;
  const name = t.name ?? t.remote_id;
  const { base, ext } = splitName(name);
  const tone = t.state === "failed" ? "error" : t.state === "retrying" ? "warn" : t.state === "completed" ? "ok" : t.state === "paused" || t.state === "cancelled" || t.state === "queued" ? "muted" : undefined;
  const kindIcon = t.kind === "upload" ? "upload" : t.kind === "cloud_download" ? "cloudDown" : "download";
  const kindLabel = t.kind === "segment_download" && !t.clip ? tr("转码下载") : KIND_LABEL[t.kind];
  return (
    <div class={`task state-${t.state}`} role="listitem" data-task={t.id} aria-label={`${kindLabel} ${name}，${stateText(t)}`}>
      <span class={`tile kind-${t.kind === "cloud_download" ? "code" : t.kind === "upload" ? "image" : "doc"} task-kind`} title={kindLabel}>
        <Icon name={kindIcon} size={20} />
      </span>
      <div class="task-main">
        <span class="task-title">
          <span class="name-text task-name" dir="auto" title={name}>
            <span class="name-base">{base}</span>{ext ? <span class="name-ext">{ext}</span> : null}
          </span>
          {t.clip ? <span class="task-tag" title={tr("只下载这一段")}><Icon name="clip" size={14} />{formatClock(t.clip.start_ms)}–{formatClock(t.clip.end_ms)}</span> : null}
          {t.quality_label ? <span class="task-tag">{t.quality_label}</span> : null}
        </span>
        {t.error ? (
          <span class={`task-status task-error code-${t.error.code.toLowerCase()}`} title={t.error.message}>
            <strong>{stateText(t)}</strong>
            <span class="ellipsis">{ERROR_COPY[t.error.code].title}：{t.error.message}</span>
          </span>
        ) : (
          <span class={`task-status st-${t.state}`}>
            <span>{stateText(t)}</span>
            {t.state === "verifying" ? <span>{t.simulated ? tr("模拟校验阶段") : tr("正在校验")}</span> : null}
            <span class="dim" title={formatDateTime(t.updated_at)}>{formatRelative(t.updated_at, now)}</span>
          </span>
        )}
      </div>
      <div class="task-meter">
        <Progress value={t.state === "completed" ? 100 : pct} tone={tone} label={tx("{0} 进度", name)} />
        <span class="tabular">{t.state === "completed" ? "100%" : pct !== null ? `${pct.toFixed(1)}%` : "—"}</span>
      </div>
      <span class="task-size tabular">
        {t.kind === "cloud_download" && !t.total_bytes ? tr("大小未知") : <>{formatBytes(t.transferred_bytes)} / {t.total_bytes ? formatBytes(t.total_bytes) : tr("大小未知")}</>}
      </span>
      <span class="task-speed tabular">
        {t.state === "running" && t.kind !== "cloud_download" ? formatSpeed(t.speed_bytes_per_sec) : ""}
        {eta ? <span class="dim">{tr("剩余")} {eta}</span> : null}
      </span>
      <span class="task-dest dim" dir="auto" title={t.destination}>{t.destination ?? ""}</span>
      <TaskActions t={t} />
      {t.error?.code === "VERIFICATION_REQUIRED" && typeof t.error.details?.verification_id === "string"
        ? <div style={{gridColumn:"1 / -1"}}><VerificationRecovery id={t.error.details.verification_id} onContinue={() => void control(t,"retry")} /></div> : null}
    </div>
  );
}

const SECTIONS: { id: string; label: string; states: ReadonlySet<TaskState>; collapsed?: boolean }[] = [
  { id: "attention", get label() { return tr("需要处理"); }, states: new Set<TaskState>(["failed"]) },
  { id: "active", get label() { return tr("进行中"); }, states: ACTIVE_STATES },
  { id: "done", get label() { return tr("已完成"); }, states: new Set<TaskState>(["completed"]) },
  { id: "cancelled", get label() { return tr("已取消"); }, states: new Set<TaskState>(["cancelled"]), collapsed: true },
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
          {tasks.length > limit ? <button type="button" class="link-btn more-btn" onClick={() => setLimit(limit + 500)}>{tr("显示更多（还有")}{groupDigits(String(tasks.length - limit))} {tr("项）")}</button> : null}
        </div>
      ) : null}
    </section>
  );
}

const JOB_STATE: Record<ServerJob["state"], string> = { get queued() { return tr("排队中"); }, get running() { return tr("处理中"); }, get completed() { return tr("已完成"); }, get failed() { return tr("失败"); }, get cancelled() { return tr("已取消"); } };

/** Work the server does on its own (cloud extraction), shown above transfers. */
function JobsSection({ jobs: rows, now }: { jobs: ServerJob[]; now: number }) {
  return (
    <section class="task-section" aria-label={tr("云端处理")}>
      <div class="task-section-head static"><span>{tr("云端处理")}</span><span class="section-count">{rows.length}</span></div>
      <div role="list" class="task-list">
        {rows.map((j) => (
          <div class={`task state-${j.state === "running" ? "running" : j.state}`} role="listitem" aria-label={`${j.title}，${JOB_STATE[j.state]}`}>
            <span class="tile kind-archive task-kind"><Icon name="unarchive" size={20} /></span>
            <div class="task-main">
              <span class="name-text task-name" dir="auto" title={j.title}>{j.title}</span>
              <span class={`task-status st-${j.state}`}>
                <span>{JOB_STATE[j.state]}</span>
                {j.target_path ? <span class="dim ellipsis" dir="auto">{tr("到")} {j.target_path}</span> : null}
                <span class="dim">{formatRelative(j.updated_at, now)}</span>
              </span>
            </div>
            <div class="task-meter">
              <Progress value={j.state === "completed" ? 100 : j.progress ?? null} tone={j.state === "failed" ? "error" : j.state === "completed" ? "ok" : undefined} label={tx("{0} 进度", j.title)} />
              <span class="tabular">{j.state === "completed" ? "100%" : j.progress !== undefined ? `${j.progress}%` : "—"}</span>
            </div>
            <span class="task-size" />
            <span class="task-speed" />
            <span class="task-dest" />
            <span class="task-actions">
              {j.state === "completed" && j.target_parent_id ? <Btn tone="quiet" iconOnly icon="folderOpen" label={tr("打开")} onClick={() => navigate({ page: "files", folder: j.target_parent_id! })} /> : <span class="act-slot" />}
            </span>
          </div>
        ))}
      </div>
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
      <span class="foot-speed tabular" aria-label={tr("当前速率")}>
        <Icon name="download" size={18} /><strong>{formatSpeed(down)}</strong>
        <Icon name="upload" size={18} /><strong>{formatSpeed(up)}</strong>
      </span>
      <button type="button" class={`chip${limit > 0 ? " on" : ""}`} aria-pressed={limit > 0 ? "true" : "false"} title={tr("限制 NekoPak 自身的传输带宽")}
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(r.left, r.top - 8 - 4 * 36, LIMITS.map((v) => ({
            id: String(v), label: v === 0 ? tr("不限速") : `${v / 1024} MB/s`, icon: v === limit ? "check" : undefined,
            run: () => void updateSettings({ bandwidth_limit_kib_s: v }).then(() => toast(v === 0 ? tr("已取消限速") : tx("已限速 {0} MB/s", v / 1024), "success"), (er) => toast(errorOf(er).message, "error")),
          })), tr("限速"));
        }}>
        <Icon name="speed" size={18} />{limit > 0 ? tx("限速 {0} MB/s", (limit / 1024).toFixed(limit % 1024 ? 1 : 0)) : tr("不限速")}
      </button>
      <span class="push" />
      {down > 0 && remaining > 0n ? <span class="dim foot-eta">{tr("下载剩余约")}{formatDuration(Number(remaining) / down)}</span> : null}
      {q ? (
        <span class="foot-meter" title={q.note}>
          <span class="dim">{q.dimension === "downstream_daily" ? tr("今日下行") : tr("本月下行")}</span>
          <Progress value={percent(q.used, q.limit)} tone={q.state === "exhausted" ? "error" : q.state === "near_limit" ? "warn" : undefined} label={tr("下行配额已用")} />
          <span class="tabular dim">{formatBytes(q.used)} / {q.limit ? formatBytes(q.limit) : "?"}</span>
        </span>
      ) : <span class="dim">{tr("配额不可获取")}</span>}
    </>
  );
}

export function TransfersPage({ kind }: { tab?: string; kind?: Kind }) {
  const st = useStore(transfers, (s) => s);
  const jobMap = useStore(jobs, (j) => j.jobs);
  const jobRows = useMemo(() => [...jobMap.values()].sort((a, b) => b.created_at.localeCompare(a.created_at)), [jobMap]);
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
  const resumable = shown.filter((t) => t.kind !== "cloud_download" && t.state === "paused");

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
    toast(fail ? tx("{0} 项成功，{1} 项失败", list.length - fail, fail) : tx("已{0} {1} 项", action === "pause" ? tr("暂停") : tr("继续"), list.length), fail ? "warning" : "success");
  };

  const cats: { id: Category; label: string }[] = [
    { id: "all", label: tr("全部") }, { id: "download", label: tr("下载") }, { id: "upload", label: tr("上传") }, { id: "cloud_download", label: tr("云下载") },
  ];

  return (
    <PageFrame
      label={tr("下载")}
      side
      caption={<>
        <h1 id="page-title" class="sr-only">{tr("下载")}</h1>
        <div class="pill-tabs" role="tablist" aria-label={tr("任务类别")}>
          {cats.map((c) => (
            <button type="button" role="tab" aria-selected={cat === c.id ? "true" : "false"} class={`pill-tab${cat === c.id ? " on" : ""}`} onClick={() => setCat(c.id)}>
              {c.label}<span class="pill-n">{counts[c.id]}</span>
            </button>
          ))}
        </div>
      </>}
      commands={<>
        <div class="cmd-group">
          <Btn tone="ghost" icon="play" label={tr("全部继续")} busy={busy} disabled={!resumable.length} reason={tr("没有已暂停的任务")} onClick={() => void bulk(resumable, "resume")} />
          <Btn tone="ghost" icon="pause" label={tr("全部暂停")} busy={busy} disabled={!pausable.length} reason={tr("没有可暂停的本地任务")} onClick={() => void bulk(pausable, "pause")} />
          <Btn tone="ghost" icon="trashSweep" label={tr("清除已完成")} disabled reason={tr("暂不支持")} />
          <Btn tone="quiet" iconOnly icon="refresh" label={tr("刷新（F5）")} busy={st.phase === "loading"} onClick={() => void loadSnapshot()} />
        </div>
        <div class="cmd-group cmd-end">
          <Btn tone="primary" icon="bolt" label={tr("添加链接")} disabled={!g.ok} reason={g.reason} onClick={() => void newCloudTask(ROOT_ID, "/")} />
        </div>
      </>}
      footer={<TransfersFooter tasks={all} />}
    >
      {st.phase === "error" && st.error ? (
        <div class="canvas-pad"><ErrorPanel error={st.error} onRetry={() => void loadSnapshot()} context="transfers_list" /></div>
      ) : st.phase === "loading" || st.phase === "idle" ? (
        <div class="canvas-pad"><Spinner label={tr("正在读取任务快照")} /></div>
      ) : shown.length === 0 && !(cat === "all" || cat === "cloud_download" ? jobRows.length : 0) ? (
        <Empty title={cat === "all" ? tr("还没有任务") : tx("没有{0}任务", cats.find((c) => c.id === cat)!.label)}>{tr("在网盘中选择文件下载，或点“添加链接”让服务端保存磁力或 http(s) 资源。")}</Empty>
      ) : (
        <div class="task-scroll">
          {(cat === "all" || cat === "cloud_download") && jobRows.length ? <JobsSection jobs={jobRows} now={now} /> : null}
          {SECTIONS.map((sec) => (
            <Section key={`${sec.id}-${cat}`} label={sec.label} now={now} initiallyCollapsed={sec.collapsed} tasks={shown.filter((t) => sec.states.has(t.state))} />
          ))}
        </div>
      )}
    </PageFrame>
  );
}
