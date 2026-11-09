import { tr, tx } from "../lib/i18n";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ClientError, SyncAction, SyncPlan } from "../lib/contract";
import { EVENTS } from "../lib/contract";
import type { SyncConfig, SyncEvent } from "../lib/contract-proposals";
import { ROOT_ID } from "../lib/contract-proposals";
import { api, errorOf, on } from "../lib/api";
import { formatDateTime, formatRelative, groupDigits } from "../lib/format";
import { useStore } from "../lib/store";
import { app, signedIn } from "../state/app";
import { navigate } from "../state/route";
import { toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Badge, Btn, Empty, ErrorPanel, Field, Segmented, Spinner, Switch } from "../components/ui";
import { confirmDialog, pickFolder } from "../components/dialogs";
import { PageFrame, PageTitle } from "../components/Frame";

const MODE_LABEL: Record<SyncConfig["mode"], string> = {
  get upload_only() { return tr("本地 → 云盘"); },
  get download_only() { return tr("云盘 → 本地"); },
  get bidirectional() { return tr("双向"); },
};

const OP_META: Record<SyncAction["op"], { label: string; icon: string; tone: "neutral" | "accent" | "ok" | "warn" | "error" }> = {
  upload: { get label() { return tr("上传"); }, icon: "upload", tone: "accent" },
  download: { get label() { return tr("下载"); }, icon: "download", tone: "accent" },
  mkdir: { get label() { return tr("新建文件夹"); }, icon: "folderPlus", tone: "neutral" },
  keep_both: { get label() { return tr("保留两份"); }, icon: "copy", tone: "warn" },
  delete: { get label() { return tr("删除"); }, icon: "trash", tone: "error" },
  skip: { get label() { return tr("跳过"); }, icon: "minusCircle", tone: "neutral" },
};

const ABS_PATH = /^([a-zA-Z]:[\\/]|%[A-Z]+%\\|\/)/;

type Phase =
  | { k: "idle" }
  | { k: "scanning"; scan_id: string; files: number; warnings: string[] }
  | { k: "scan_failed"; error: ClientError }
  | { k: "plan"; plan: SyncPlan }
  | { k: "applying"; plan: SyncPlan; job_id: string; state: string }
  | { k: "applied"; plan: SyncPlan; ok: boolean; error?: ClientError };

function blankConfig(): SyncConfig {
  return { config_id: "", name: tr("新的同步"), mode: "upload_only", local_path: "", remote_id: ROOT_ID, remote_path: "/", exclude_globs: ["Thumbs.db", "~$*", "*.tmp"], allow_deletes: false, first_run: true, auto_enabled:false, interval_seconds:300 };
}

export function SyncPage({ config }: { config?: string }) {
  const desktop = useStore(app, s => s.runtime === "tauri");
  const enabled = useStore(app, s => !!s.app?.feature_flags.sync);
  if (desktop && !enabled) return <PageFrame label={tr("自动同步")} caption={<PageTitle>{tr("自动同步")}</PageTitle>}><div class="canvas-pad"><Empty title={tr("当前平台暂不支持同步")}>{tr("单向与双向同步仍在开发计划中，当前不能扫描或执行同步。")}</Empty><Btn label={tr("新建同步")} disabled reason={tr("当前平台暂不支持同步")} /></div></PageFrame>;
  return <BrowserSyncPage config={config} />;
}

function BrowserSyncPage({ config }: { config?: string }) {
  const signed = useStore(app, (s) => signedIn(s));
  const epoch = useStore(app, (s) => s.epoch);
  const [configs, setConfigs] = useState<SyncConfig[] | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [draft, setDraft] = useState<SyncConfig | null>(null);

  const load = () => {
    setError(null);
    api.syncConfigsList().then(setConfigs, (e) => setError(errorOf(e)));
  };
  useEffect(load, [epoch]);

  const selected = draft ?? configs?.find((c) => c.config_id === config) ?? configs?.[0] ?? null;

  return (
    <PageFrame label={tr("自动同步")} class="sync-page" caption={<PageTitle>{tr("自动同步")}</PageTitle>}
      commands={<>
        <div class="cmd-group"><span class="cmd-meta">{tr("同步先生成计划供你确认；扫描失败或不完整时不会产生删除。")}</span></div>
        <div class="cmd-group cmd-end"><Btn tone="primary" icon="plus" label={tr("新建同步")} onClick={() => setDraft(blankConfig())} /></div>
      </>}>
      <div class="sync-layout">
        <aside class="sync-list" aria-label={tr("同步配置")}>
          {error ? <ErrorPanel error={error} compact onRetry={load} />
            : !configs ? <Spinner />
              : configs.length === 0 && !draft ? <p class="muted canvas-pad">{tr("还没有同步配置")}</p>
                : (
                  <ul>
                    {draft ? (
                      <li><button type="button" class="sync-item on" aria-current="true"><strong>{draft.name || tr("新的同步")}</strong><span class="muted">{tr("尚未保存")}</span></button></li>
                    ) : null}
                    {configs.map((c) => (
                      <li>
                        <button type="button" class={`sync-item${!draft && selected?.config_id === c.config_id ? " on" : ""}`}
                          aria-current={!draft && selected?.config_id === c.config_id ? "true" : undefined}
                          onClick={() => { setDraft(null); navigate({ page: "sync", config: c.config_id }); }}>
                          <strong dir="auto">{c.name}</strong>
                          <span class="muted">{MODE_LABEL[c.mode]}{c.first_run ? tr("，未执行过") : c.last_applied_at ? tx("，{0}同步", formatRelative(c.last_applied_at)) : ""}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
        </aside>
        <div class="sync-detail">
          {!selected ? (
            <Empty title={tr("选择或新建一个同步")}>{tr("先选择方向与目录，扫描后预览将要发生的更改。")}</Empty>
          ) : (
            <SyncDetail key={selected.config_id || "draft"} cfg={selected} signed={signed}
              onSaved={(c) => { setDraft(null); load(); navigate({ page: "sync", config: c.config_id }); }}
              onDiscard={draft ? () => setDraft(null) : undefined} />
          )}
        </div>
      </div>
    </PageFrame>
  );
}

function SyncDetail({ cfg, signed, onSaved, onDiscard }: { cfg: SyncConfig; signed: boolean; onSaved: (c: SyncConfig) => void; onDiscard?: () => void }) {
  const desktop = useStore(app, s => s.runtime === "tauri");
  const bidirectional = useStore(app, s => !!s.app?.feature_flags.sync_bidirectional);
  const [form, setForm] = useState<SyncConfig>(cfg);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<ClientError | null>(null);
  const [phase, setPhase] = useState<Phase>({ k: "idle" });
  const [history, setHistory] = useState<SyncEvent[]>([]);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  // Only user-editable fields count; first_run/last_applied_at come from the backend.
  const editable = (c: SyncConfig) => JSON.stringify([c.name, c.mode, c.local_path, c.remote_id, c.exclude_globs, c.allow_deletes,c.auto_enabled??false,c.interval_seconds??300]);
  const dirty = editable(form) !== editable(cfg);
  useEffect(() => {
    setForm((f) => ({ ...f, config_id: cfg.config_id, first_run: cfg.first_run, last_applied_at: cfg.last_applied_at }));
  }, [cfg.config_id, cfg.first_run, cfg.last_applied_at]);
  const isNew = !cfg.config_id;
  const pathError = form.local_path && !ABS_PATH.test(form.local_path) ? tr("需要绝对路径，例如 D:\\Study\\课程资料") : undefined;

  const loadHistory = () => {
    if (!cfg.config_id) return;
    api.syncHistory(cfg.config_id).then((p) => setHistory(p.items), () => setHistory([]));
  };
  useEffect(loadHistory, [cfg.config_id]);

  useEffect(() => {
    const subs: (() => void)[] = [];
    let alive = true;
    void (async () => {
      const add = (u: () => void) => (alive ? subs.push(u) : u());
      add(await on(EVENTS.syncScanProgress, (ev) => {
        const p = phaseRef.current;
        if (p.k === "scanning" && p.scan_id === ev.scan_id) setPhase({ ...p, files: ev.files_scanned, warnings: ev.warnings });
      }));
      add(await on(EVENTS.syncPlanReady, (ev) => {
        if (ev.config_id && ev.config_id !== cfg.config_id) return;
        if (phaseRef.current.k === "applying") return;
        if (phaseRef.current.k !== "scanning" && !cfg.auto_enabled) return;
        api.syncPlan(cfg.config_id).then((plan) => { setPhase({ k: "plan", plan }); loadHistory(); }, (e) => setPhase({ k: "scan_failed", error: errorOf(e) }));
      }));
      add(await on(EVENTS.syncChanged, (ev) => {
        const p = phaseRef.current;
        if (p.k === "scanning" && ev.job_id === p.scan_id && ev.state === "failed") {
          setPhase({ k: "scan_failed", error: ev.error ?? { code: "INTERNAL", message: tr("扫描失败"), retryable: true } });
          loadHistory();
        }
        if (p.k === "applying" && ev.job_id === p.job_id) {
          if (ev.state === "completed" || ev.state === "failed") {
            setPhase({ k: "applied", plan: p.plan, ok: ev.state === "completed", error: ev.error });
            toast(ev.state === "completed" ? tr("同步计划已执行") : tr("同步执行失败"), ev.state === "completed" ? "success" : "error");
            loadHistory();
            onSaved(cfg);
          } else setPhase({ ...p, state: ev.state });
        }
      }));
    })();
    return () => {
      alive = false;
      subs.forEach((u) => u());
    };
  }, [cfg.config_id]);

  useEffect(() => {
    if (!desktop || !cfg.auto_enabled || !cfg.config_id) return;
    let alive = true;
    const check = () => {
      if (phaseRef.current.k === "applying" || phaseRef.current.k === "scanning") return;
      void api.syncPlan(cfg.config_id).then(plan => {
        if (alive && (plan.has_deletes || plan.has_unresolved_conflicts) && (phaseRef.current.k !== "plan" || phaseRef.current.plan.plan_id !== plan.plan_id)) setPhase({k:"plan",plan});
      },()=>{});
    };
    check(); const timer=setInterval(check,10000);
    return()=>{alive=false;clearInterval(timer);};
  },[desktop,cfg.auto_enabled,cfg.config_id]);

  useEffect(() => {
    if (!desktop || phase.k !== "applying") return;
    let alive = true, pending = false;
    const poll = async () => {
      if (pending) return; pending = true;
      try {
        const run = await api.syncRunStatus(phase.job_id);
        if (!alive) return;
        if (["completed","failed","cancelled"].includes(run.state)) {
          setPhase({ k:"applied",plan:phase.plan,ok:run.state==="completed",error:run.error });
          loadHistory(); onSaved(cfg);
        } else setPhase(p => p.k === "applying" && p.job_id === run.job_id ? { ...p, state:run.state } : p);
      } catch(e) { if(alive) setPhase({k:"applied",plan:phase.plan,ok:false,error:errorOf(e)}); }
      finally { pending = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 2500);
    return () => { alive = false; clearInterval(timer); };
  }, [desktop, phase.k === "applying" ? phase.job_id : null]);

  const save = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await api.syncConfigSave(form);
      toast(tr("同步配置已保存"), "success");
      onSaved(saved);
    } catch (e) {
      setSaveError(errorOf(e));
    } finally {
      setSaving(false);
    }
  };

  const scan = async () => {
    try {
      const ref = await api.syncScan(cfg.config_id);
      setPhase({ k: "scanning", scan_id: ref.scan_id, files: 0, warnings: [] });
      if (desktop) {
        const plan = await api.syncPlan(cfg.config_id);
        setPhase({k:"plan",plan}); loadHistory();
      }
    } catch (e) {
      setPhase({ k: "scan_failed", error: errorOf(e) });
    }
  };

  const apply = async (plan: SyncPlan) => {
    const deletes = plan.actions.filter((a) => a.op === "delete");
    const n = plan.actions.filter((a) => a.op !== "skip").length;
    const ok = await confirmDialog({
      title: tr("执行同步计划？"),
      body: (
        <div>
          <p>{tr("将执行")}{n} {tr("项操作：")}{MODE_LABEL[plan.mode]}{tr("，从")}<span dir="auto">{plan.source}</span>{tr("到")} <span dir="auto">{plan.destination}</span>。</p>
          {deletes.length ? (
            <>
              <p class="danger-text"><Icon name="alert" size={14} /> {tr("其中")}{deletes.length} {tr("项是删除：")}</p>
              <ul class="plan-deletes">{deletes.map((d) => <li dir="auto">{d.path}</li>)}</ul>
            </>
          ) : <p class="muted">{tr("此计划不包含删除。")}</p>}
          <p class="muted">{tr("执行前会重新校验计划版本")}{plan.base_revision}{tr("；若期间有变化，将要求重新扫描。")}</p>
        </div>
      ),
      confirmLabel: deletes.length ? tx("执行并删除 {0} 项", deletes.length) : tr("执行计划"),
      danger: deletes.length > 0,
      acknowledge: deletes.length ? tx("我已检查以上 {0} 个将被删除的文件", deletes.length) : undefined,
    });
    if (!ok) return;
    try {
      const ref = await api.syncApply(plan.plan_id, deletes.length > 0);
      setPhase({ k: "applying", plan, job_id: ref.task_id, state: "queued" });
    } catch (e) {
      const err = errorOf(e);
      toast(err.message, "error", { error: err });
      if (err.code === "CONFLICT") setPhase({ k: "idle" });
    }
  };

  return (
    <div class="sync-detail-inner">
      <form class="sync-form" onSubmit={(e) => { e.preventDefault(); if (!pathError && form.local_path) void save(); }}>
        <div class="form-grid">
          <Field label={tr("名称")} htmlFor="sync-name">
            <input id="sync-name" class="input" value={form.name} onInput={(e) => setForm({ ...form, name: (e.target as HTMLInputElement).value })} />
          </Field>
          <Field label={tr("方向")} hint={form.mode === "upload_only" ? tr("只把本地变化上传到云盘") : tr("只把云盘变化下载到本地")}>
            <Segmented<SyncConfig["mode"]> label={tr("同步方向")} value={form.mode} onChange={(mode) => setForm({ ...form, mode })}
              options={[
                { value: "upload_only", label: tr("本地 → 云盘"), icon: "upload" },
                { value: "download_only", label: tr("云盘 → 本地"), icon: "download" },
                { value: "bidirectional", label: tr("双向"), icon: "sync", disabled: !bidirectional, reason: tr("双向同步在 M3 开放：需要三方比较与冲突副本经过验证") },
              ]} />
          </Field>
          <Field label={tr("本地文件夹")} htmlFor="sync-local" error={pathError}
            hint={tr("输入绝对路径。")}>
            <div class="directory-picker"><input id="sync-local" class="input mono" value={form.local_path} placeholder={tr("D:\\Study\\课程资料")} spellcheck={false}
              onInput={(e) => setForm({ ...form, local_path: (e.target as HTMLInputElement).value })} />
              {desktop ? <Btn label={tr("选择文件夹")} icon="folderOpen" onClick={() => void api.downloadDirectoryPick().then(path => { if (path) setForm(f => ({...f,local_path:path})); },e => setSaveError(errorOf(e)))}/> : null}</div>
          </Field>
          <Field label={tr("云盘文件夹")}>
            <div class="row-gap">
              <span class="path-pill" dir="auto" title={form.remote_path}><Icon name="folder" />{form.remote_path}</span>
              <Btn size="sm" label={tr("选择…")} disabled={!signed} reason={tr("需要先登录")} onClick={async () => {
                const f = await pickFolder(tr("选择云盘文件夹"), tr("使用此文件夹"), [], form.remote_id);
                if (f) setForm({ ...form, remote_id: f.id, remote_path: f.path });
              }} />
            </div>
          </Field>
          <Field label={tr("排除规则")} htmlFor="sync-ex" hint={tr("每行一个 glob，例如 node_modules/**。符号链接默认不跟随。")}>
            <textarea id="sync-ex" class="input mono" rows={3} value={form.exclude_globs.join("\n")}
              onInput={(e) => setForm({ ...form, exclude_globs: (e.target as HTMLTextAreaElement).value.split(/\r?\n/).map((s) => s.trim()).filter(Boolean) })} />
          </Field>
          {desktop ? <Field label={tr("自动同步")} hint={tr("自动同步遇到删除或冲突时保留计划，等待你确认。") }>
            <Switch label={tr("启用自动同步")} checked={form.auto_enabled??false} onChange={auto_enabled=>setForm({...form,auto_enabled})}/>
            {form.auto_enabled ? <label>{tr("检查间隔（秒）")}<input class="input" type="number" min={60} max={86400} value={form.interval_seconds??300} onInput={e=>setForm({...form,interval_seconds:Number((e.target as HTMLInputElement).value)})}/></label> : null}
          </Field> : null}
          <div class="field">
            <div class="switch-row">
              <span>
                <span class="field-label">{tr("同步删除")}</span>
                <span class="field-hint">{form.mode === "upload_only" ? tr("本地删除后，同时删除云盘中的对应文件") : tr("云盘删除后，同时删除本地的对应文件")}</span>
              </span>
              <Switch label={tr("同步删除")} checked={form.allow_deletes}
                disabled={cfg.first_run} reason={tr("首次同步不允许删除：需要先建立同步基线")}
                onChange={async (v) => {
                  if (v) {
                    const ok = await confirmDialog({ title: tr("开启同步删除？"), body: <p>{tr("开启后，计划中可能出现删除操作。每次执行包含删除的计划前仍会要求单独确认。")}</p>, confirmLabel: tr("开启"), danger: true });
                    if (!ok) return;
                  }
                  setForm({ ...form, allow_deletes: v });
                }} />
            </div>
          </div>
        </div>
        {saveError ? <ErrorPanel error={saveError} compact /> : null}
        <div class="form-actions">
          {onDiscard ? <Btn label={tr("放弃")} onClick={onDiscard} /> : null}
          <Btn type="submit" tone={isNew ? "primary" : "default"} label={isNew ? tr("保存配置") : tr("保存更改")} busy={saving}
            disabled={!dirty && !isNew || !!pathError || !form.local_path} reason={!form.local_path ? tr("请填写本地文件夹") : pathError ?? tr("没有未保存的更改")} />
        </div>
      </form>

      <section class="plan" aria-labelledby="plan-title">
        <div class="plan-head">
          <h2 id="plan-title">{tr("更改预览")}</h2>
          <Btn tone="primary" icon="search" label={phase.k === "plan" || phase.k === "applied" ? tr("重新扫描") : tr("扫描并生成计划")}
            busy={phase.k === "scanning"}
            disabled={isNew || dirty || !signed || phase.k === "applying"}
            reason={isNew ? tr("请先保存配置") : dirty ? tr("请先保存更改") : !signed ? tr("需要先登录") : tr("正在执行")}
            onClick={() => void scan()} />
        </div>
        <p class="muted plan-sub">{tr("扫描是只读的：只生成计划，不会上传、下载或删除任何文件。")}</p>
        <PlanView phase={phase} onApply={(p) => void apply(p)} onRescan={() => void scan()} />
      </section>

      {history.length ? (
        <section class="history" aria-labelledby="hist-title">
          <h2 id="hist-title">{tr("历史记录")}</h2>
          <ol class="history-list">
            {history.map((h) => (
              <li class={`hist-${h.kind}`}>
                <Icon name={h.kind === "error" ? "alertCircle" : h.kind === "apply" ? "checkCircle" : h.kind === "plan" ? "doc" : "search"} size={14} />
                <span class="hist-time" title={h.at}>{formatDateTime(h.at)}</span>
                <span class="hist-sum">{h.summary}{h.error ? `：${h.error.message}` : ""}</span>
              </li>
            ))}
          </ol>
        </section>
      ) : null}
    </div>
  );
}

function PlanView({ phase, onApply, onRescan }: { phase: Phase; onApply: (p: SyncPlan) => void; onRescan: () => void }) {
  const [filter, setFilter] = useState<SyncAction["op"] | "all">("all");
  const plan = phase.k === "plan" || phase.k === "applying" || phase.k === "applied" ? phase.plan : null;
  const counts = useMemo(() => {
    const c: Partial<Record<SyncAction["op"], number>> = {};
    for (const a of plan?.actions ?? []) c[a.op] = (c[a.op] ?? 0) + 1;
    return c;
  }, [plan]);

  if (phase.k === "idle") return <div class="plan-empty muted">{tr("还没有计划。")}</div>;
  if (phase.k === "scanning") {
    return (
      <div class="plan-scanning" role="status">
        <Spinner label={tr("正在扫描")} />
        <span>{tr("正在扫描，已检查")}{groupDigits(String(phase.files))} {tr("个文件…")}</span>
        {phase.warnings.map((w) => <span class="warn-text"><Icon name="alert" size={14} />{w}</span>)}
      </div>
    );
  }
  if (phase.k === "scan_failed") {
    return (
      <div>
        <ErrorPanel error={phase.error} onRetry={onRescan} context="sync_scan" />
        <p class="muted pad-top"><Icon name="shield" size={14} /> {tr("扫描未完成，因此没有生成任何操作，也不会把未扫描到的文件视为已删除。")}</p>
      </div>
    );
  }
  if (!plan) return null;
  const shown = filter === "all" ? plan.actions : plan.actions.filter((a) => a.op === filter);
  const blocking = plan.has_unresolved_conflicts;

  return (
    <div class="plan-view">
      <div class="plan-route">
        <span dir="auto" class="mono">{plan.source}</span>
        <Icon name="chevronRight" />
        <span dir="auto" class="mono">{plan.destination}</span>
        <span class="muted" title={`plan_id ${plan.plan_id}`}>{tr("基线版本")}{plan.base_revision}</span>
      </div>
      <div class="plan-counts" role="group" aria-label={tr("按操作筛选")}>
        <button type="button" class={`chip${filter === "all" ? " on" : ""}`} aria-pressed={filter === "all" ? "true" : "false"} onClick={() => setFilter("all")}>{tr("全部")} {plan.actions.length}</button>
        {(Object.keys(OP_META) as SyncAction["op"][]).filter((op) => counts[op]).map((op) => (
          <button type="button" class={`chip chip-${OP_META[op].tone}${filter === op ? " on" : ""}`} aria-pressed={filter === op ? "true" : "false"} onClick={() => setFilter(op)}>
            <Icon name={OP_META[op].icon} size={14} />{OP_META[op].label} {counts[op]}
          </button>
        ))}
      </div>
      {plan.warnings.length ? (
        <ul class="warn-list">{plan.warnings.map((w) => <li><Icon name="alert" size={14} />{w}</li>)}</ul>
      ) : null}
      <ul class="plan-actions" aria-label={tr("计划中的操作")}>
        {shown.map((a) => (
          <li class={`plan-action${a.destructive ? " destructive" : ""}`}>
            <Badge tone={OP_META[a.op].tone} icon={OP_META[a.op].icon}>{OP_META[a.op].label}</Badge>
            <span class="plan-path" dir="auto" title={a.path}>{a.path}</span>
            <span class="plan-reason muted">{a.reason}</span>
          </li>
        ))}
      </ul>
      <div class="plan-foot">
        {phase.k === "applying" ? (
          <span class="row-gap" role="status"><Spinner label={tr("正在执行")} />{tr("正在执行（")}{phase.state}）…</span>
        ) : phase.k === "applied" ? (
          phase.ok ? <span class="ok-text"><Icon name="checkCircle" size={14} /> {tr("已执行。如需再次同步，请重新扫描。")}</span>
            : phase.error ? <ErrorPanel error={phase.error} compact onRetry={onRescan} /> : null
        ) : (
          <>
            <span class="muted">{blocking ? tr("存在未解决的冲突，处理后重新扫描。") : plan.has_deletes ? tr("此计划包含删除，执行前需要单独确认。") : tr("确认无误后执行。")}</span>
            <Btn tone={plan.has_deletes ? "danger" : "primary"} icon="check" label={plan.has_deletes ? tr("检查并执行…") : tr("执行计划…")}
              disabled={blocking} reason={tr("计划含未解决的冲突，不能执行")} onClick={() => onApply(plan)} />
          </>
        )}
      </div>
    </div>
  );
}
