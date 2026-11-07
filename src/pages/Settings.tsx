import { useEffect, useState } from "preact/hooks";
import type { Operation } from "../lib/contract";
import { CONTRACT_VERSION } from "../lib/contract";
import type { QuotaItem, Settings } from "../lib/contract-proposals";
import { PROPOSED_COMMANDS } from "../lib/contract-proposals";
import { errorOf } from "../lib/api";
import { formatBytes, formatDateTime, parseU64, percent } from "../lib/format";
import { useStore } from "../lib/store";
import { app, applyTheme, backendLabel, refreshCaps, refreshQuota, updateSettings } from "../state/app";
import { navigate, type SettingsSection } from "../state/route";
import { toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Badge, Btn, ErrorPanel, Field, Progress, Segmented, Spinner, Switch, openOfficial } from "../components/ui";
import { confirmDialog, SHORTCUTS } from "../components/dialogs";
import { AuthPanel, BACKEND_BLURB } from "./Auth";
import { openMockPanel } from "./DevPanel";
import { useDensity } from "./Files";
import { AboutContent } from "./About";
import { PageFrame, PageTitle } from "../components/Frame";
import { prefs, setPref } from "../state/prefs";
import { ACCENTS } from "../styles/accents.generated";

const SECTIONS: { id: SettingsSection; label: string; icon: string }[] = [
  { id: "account", label: "账号", icon: "user" },
  { id: "general", label: "外观", icon: "palette" },
  { id: "transfers", label: "下载与传输", icon: "download" },
  { id: "network", label: "网络", icon: "network" },
  { id: "backends", label: "连接", icon: "backend" },
  { id: "quota", label: "配额", icon: "quota" },
  { id: "shortcuts", label: "快捷键", icon: "keyboard" },
  { id: "developer", label: "开发者", icon: "flask" },
  { id: "about", label: "关于", icon: "info" },
];

export function SettingsPage({ section }: { section: SettingsSection }) {
  return (
    <PageFrame label="设置" class="settings-page" caption={<PageTitle>设置</PageTitle>}>
      <div class="settings-layout">
        <nav class="settings-nav" aria-label="设置分类">
          {SECTIONS.map((x) => (
            <a href={`#/settings/${x.id}`} class={`settings-link${section === x.id ? " on" : ""}`} aria-current={section === x.id ? "page" : undefined}
              onClick={(e) => { e.preventDefault(); navigate({ page: "settings", section: x.id }); }}>
              <Icon name={x.icon} filled={section === x.id} />
              <span>{x.label}</span>
            </a>
          ))}
        </nav>
        <div class="settings-body">
          {section === "general" ? <General />
            : section === "account" ? <Section title="账号与授权"><div class="set-card"><AuthPanel /></div></Section>
              : section === "backends" ? <Backends />
                : section === "quota" ? <Quota />
                  : section === "transfers" ? <TransfersSettings />
                    : section === "network" ? <Network />
                      : section === "shortcuts" ? <Shortcuts />
                        : section === "about" ? <AboutContent />
                          : <Developer />}
        </div>
      </div>
    </PageFrame>
  );
}

function Section({ title, children, desc }: { title: string; desc?: preact.ComponentChildren; children: preact.ComponentChildren }) {
  return (
    <div class="settings-section">
      <h2>{title}</h2>
      {desc ? <p class="section-desc">{desc}</p> : null}
      <div class="set-group">{children}</div>
    </div>
  );
}

function Row({ label, hint, children, id, icon, below }: { label: string; hint?: preact.ComponentChildren; children: preact.ComponentChildren; id?: string; icon?: string; below?: boolean }) {
  return (
    <div class={`set-row${below ? " below" : ""}`}>
      {icon ? <Icon name={icon === "dark" ? "moon" : icon} class="set-icon" /> : null}
      <div class="set-text">
        <label class="set-label" for={id}>{label}</label>
        {hint ? <p class="set-hint">{hint}</p> : null}
      </div>
      <div class="set-control">{children}</div>
    </div>
  );
}

function General() {
  const settings = useStore(app, (s) => s.settings);
  const accent = useStore(prefs, (p) => p.accent);
  const reduce = useStore(prefs, (p) => p.reduceMotion);
  const [, density, setDensity] = useDensity();
  const theme = settings?.theme ?? "system";
  return (
    <Section title="外观">
      <Row icon="dark" label="主题" hint="跟随系统时随 Windows 的浅色/深色模式切换。">
        <Segmented<Settings["theme"]> label="主题" value={theme}
          onChange={(t) => {
            applyTheme(t);
            updateSettings({ theme: t }).catch((e) => toast(errorOf(e).message, "error"));
          }}
          options={[
            { value: "system", label: "跟随系统", icon: "monitor" },
            { value: "light", label: "浅色", icon: "sun" },
            { value: "dark", label: "深色", icon: "moon" },
          ]} />
      </Row>
      <Row icon="palette" label="强调色" hint="整套配色由所选颜色按 Material 3 规则生成。" below>
        <div class="swatches" role="radiogroup" aria-label="强调色">
          {ACCENTS.map((a) => (
            <button type="button" role="radio" aria-checked={accent === a.id ? "true" : "false"} class={`swatch${accent === a.id ? " on" : ""}`}
              style={{ "--sw": a.seed } as preact.JSX.CSSProperties} title={a.label} onClick={() => setPref("accent", a.id)}>
              <span class="swatch-dot">{accent === a.id ? <Icon name="check" size={18} /> : null}</span>
              <span class="swatch-label">{a.label}</span>
            </button>
          ))}
        </div>
      </Row>
      <Row icon="rows" label="列表行距" hint="紧凑行距一屏能显示更多文件。">
        <Segmented label="列表行距" value={density} onChange={setDensity}
          options={[{ value: "comfortable", label: "舒适" }, { value: "compact", label: "紧凑" }]} />
      </Row>
      <Row icon="motion" label="减少动画" hint="与系统的“动画效果”设置同时生效，开启后切换与展开直接跳到结果。">
        <Switch label="减少动画" checked={reduce} onChange={(v) => setPref("reduceMotion", v)} />
      </Row>
      <Row icon="label" label="界面语言" hint="其他语言在后续版本提供。">
        <select class="select" aria-label="界面语言" value="zh-CN" disabled title="其他语言在后续版本提供">
          <option value="zh-CN">简体中文</option>
        </select>
      </Row>
      <Row icon="shield" label="使用数据" hint="NekoPak 不收集遥测数据。错误报告只会在你主动复制或导出时生成，并已脱敏。">
        <Badge tone="ok" icon="checkCircle">遥测已关闭</Badge>
      </Row>
    </Section>
  );
}

const OPS: { op: Operation; label: string }[] = [
  { op: "list", label: "列目录" }, { op: "search", label: "搜索" }, { op: "mkdir", label: "新建" },
  { op: "rename", label: "重命名" }, { op: "move", label: "移动" }, { op: "copy", label: "复制" },
  { op: "trash", label: "回收站" }, { op: "download", label: "下载" }, { op: "upload", label: "上传" },
  { op: "cloud_task", label: "云下载" }, { op: "quota", label: "配额" }, { op: "stream_url", label: "播放链接" },
  { op: "read_range", label: "随机读取" }, { op: "segment_download", label: "指定视频片段" },
];

function Backends() {
  const s = useStore(app, (x) => x);
  const real = s.caps.filter(c => c.backend !== "mock");
  const stateLabel = (c: typeof real[number]) => c.status === "not_installed" ? "未配置" : c.status === "needs_login" ? "待授权" : c.status === "ready" ? "只读已连接" : "尚未启用";
  return (
    <Section title="PikPak 连接" desc="官方 CLI 是 NekoPak 要支持的连接方式。完整云盘功能仍按上游清单实现，也可以由经验证的社区接口提供。">
      <div class="row-gap pad-bottom">
        <Btn icon="refresh" label="刷新连接状态" busy={s.capsLoading} onClick={() => void refreshCaps()} />
      </div>
      {s.capsError ? <ErrorPanel error={s.capsError} onRetry={() => void refreshCaps()} /> : null}
      {s.app?.mode === "mock" || s.runtime === "mock" ? <p class="inline-note">当前尚未连接 PikPak，文件和任务是离线演示数据。测试能力详情仅在“开发者”页显示。</p> : null}
      <div class="backend-list">
        {real.map((c) => (
          <article class="backend-card" aria-labelledby={`be-${c.backend}`}>
            <header>
              <h3 id={`be-${c.backend}`}>{backendLabel(c.backend)}</h3>
              <Badge tone={c.status === "ready" ? "ok" : "neutral"}>{stateLabel(c)}</Badge>
              {c.version ? <span class="muted">版本 {c.version}</span> : null}
            </header>
            <p class="muted">{BACKEND_BLURB[c.backend]}</p>
            {c.status === "not_installed" ? <p>应用尚未配置可执行文件与授权资料目录，暂未检测安装状态。接入后会显示实际授权与可用功能。</p>
              : c.status === "needs_login" ? <p>已识别连接程序，等待你完成官方授权。当前没有登录账号。</p>
                : c.status === "unavailable" ? <p>这条连接方式尚未启用。不能从此状态判断服务支持哪些功能。</p>
                  : <p>当前接入仅完成文件浏览和元数据读取。真实下载、上传、视频片段等功能仍待完成。</p>}
            {c.status === "ready" ? (
              <ul class="op-chips" aria-label="能力">
                {OPS.filter(o => c.operations[o.op]).map((o) => {
                  const v = c.operations[o.op];
                  return <li class="yes"><Icon name="check" size={12} />{o.label}<span class="sr-only">{v ? "当前可用" : "未启用"}</span></li>;
                })}
              </ul>
            ) : null}
          </article>
        ))}
      </div>
      <p class="muted">所有连接方式遵守同一账号的实际权限和配额。</p>
    </Section>
  );
}

const DIM_LABEL: Record<QuotaItem["dimension"], string> = {
  downstream_daily: "每日下行",
  downstream_monthly: "每月下行",
  upload_monthly: "每月上传",
  cloud_download_monthly: "每月云下载",
};

function QuotaRow({ q }: { q: QuotaItem }) {
  const pct = q.unit === "bytes" ? percent(q.used, q.limit) : (() => {
    const u = parseU64(q.used ?? null);
    const l = parseU64(q.limit ?? null);
    return u !== null && l !== null && l > 0n ? Number((u * 1000n) / l) / 10 : null;
  })();
  const fmt = (v?: string) => (v === undefined ? "?" : q.unit === "bytes" ? formatBytes(v) : `${v} 次`);
  return (
    <li class={`quota-row q-${q.state}`}>
      <div class="quota-top">
        <strong>{DIM_LABEL[q.dimension]}</strong>
        <span class="muted">{q.scope === "connected_apps" ? "Connected Apps" : "账号"}</span>
        {q.state === "exhausted" ? <Badge tone="error" icon="alert">已用尽</Badge>
          : q.state === "near_limit" ? <Badge tone="warn" icon="alert">接近上限</Badge>
            : q.exactness === "estimated" ? <Badge tone="warn">估计值</Badge>
              : q.exactness === "unavailable" ? <Badge>不可获取</Badge> : <Badge tone="ok">正常</Badge>}
      </div>
      {q.exactness === "unavailable" ? (
        <p class="muted">{q.note ?? "后端未提供"}</p>
      ) : (
        <>
          {q.limit ? <Progress value={pct} tone={q.state === "exhausted" ? "error" : q.state === "near_limit" ? "warn" : undefined} label={`${DIM_LABEL[q.dimension]} 已用`} /> : null}
          <p class="tabular">
            已用 {fmt(q.used)}{q.limit ? ` / ${fmt(q.limit)}` : "（上限未提供）"}{q.remaining ? `，剩余 ${fmt(q.remaining)}` : ""}
          </p>
          {q.resets_at ? <p class="muted">重置时间 {formatDateTime(q.resets_at)}</p> : null}
          {q.note ? <p class="muted">{q.note}</p> : null}
        </>
      )}
    </li>
  );
}

function Quota() {
  const s = useStore(app, (x) => x);
  useEffect(() => void refreshQuota(), []);
  return (
    <Section title="配额" desc={<>
      数值来自后端。后端没有提供可靠余额时显示“不可获取”，不会根据本机下载量推算后冒充真实余额。
      {s.runtime === "mock" ? <strong> 当前为 Mock 数据。</strong> : null}
    </>}>
      <div class="row-gap pad-bottom">
        <Btn icon="refresh" label="刷新" busy={s.quotaLoading} onClick={() => void refreshQuota()} />
        {s.quota ? <span class="muted">查询于 {formatDateTime(s.quota.checked_at)}，{backendLabel(s.quota.backend)}</span> : null}
      </div>
      {s.quotaError ? <ErrorPanel error={s.quotaError} onRetry={() => void refreshQuota()} />
        : s.auth?.state !== "signed_in" && s.auth?.state !== "demo" ? <p class="muted">登录后可查询配额。</p>
          : !s.quota ? <Spinner />
            : <ul class="quota-list">{s.quota.items.map((q) => <QuotaRow q={q} />)}</ul>}
      <div class="explain">
        <h3>Connected Apps 规则摘要</h3>
        <ul>
          <li>官方 CLI、WebDAV、Rclone 等都属于 Connected Apps。云下载、下行、上传三项分别计算，Connected Apps 合计最多使用该项月度额度的 25%。</li>
          <li>Free 账号的 Connected Apps 每日下行最多 5 GB，新加坡时间 00:00 重置；每日与每月限制中先达到的那个先生效。</li>
          <li>配额用尽后，文件列表与任务历史仍可查看。NekoPak 不提供“切换后端恢复额度”之类的操作。</li>
        </ul>
        <Btn tone="ghost" icon="external" label="查看官方说明" onClick={() => void openOfficial("https://mypikpak.com/en-US/connect-apps-faq")} />
      </div>
    </Section>
  );
}

const ABS = /^([a-zA-Z]:\\|%[A-Z]+%\\|\/)/;
const GiB = 1024n ** 3n;

function TransfersSettings() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <Section title="下载与传输" desc="后端设置服务尚未实现，当前页面不会保存无效配置。"><div class="inline-note">Rust Mock 仅模拟任务、不写入下载文件；CLI 预览不开放真实下载。并发、缓存、限速和目录设置将在后端实现后启用。</div><Btn label="保存" disabled reason="后端设置服务尚未实现" /></Section>;
  return <BrowserTransferSettings />;
}

function BrowserTransferSettings() {
  const settings = useStore(app, (s) => s.settings);
  const [form, setForm] = useState(() => settings && {
    download_dir: settings.download_dir,
    max: String(settings.max_concurrent_transfers),
    bw: String(settings.bandwidth_limit_kib_s),
    cache_dir: settings.cache.dir,
    cache_gib: String(Number((parseU64(settings.cache.max_bytes) ?? 0n) / GiB)),
  });
  const [busy, setBusy] = useState(false);
  if (!settings || !form) return <Section title="传输与缓存"><Spinner /></Section>;

  const max = Number(form.max);
  const bw = Number(form.bw);
  const cg = Number(form.cache_gib);
  const errs = {
    download_dir: ABS.test(form.download_dir) ? undefined : "需要绝对路径",
    max: Number.isInteger(max) && max >= 1 && max <= 8 ? undefined : "请输入 1–8 之间的整数",
    bw: Number.isInteger(bw) && bw >= 0 ? undefined : "请输入不小于 0 的整数",
    cache_dir: ABS.test(form.cache_dir) ? undefined : "需要绝对路径",
    cache_gib: Number.isInteger(cg) && cg >= 1 && cg <= 4096 ? undefined : "请输入 1–4096 之间的整数",
  };
  const invalid = Object.values(errs).some(Boolean);

  const save = async () => {
    setBusy(true);
    try {
      await updateSettings({
        download_dir: form.download_dir,
        max_concurrent_transfers: max,
        bandwidth_limit_kib_s: bw,
        cache: { dir: form.cache_dir, max_bytes: (BigInt(cg) * GiB).toString() },
      });
      toast("传输与缓存设置已保存", "success");
    } catch (e) {
      toast(errorOf(e).message, "error", { error: errorOf(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="传输与缓存">
      <form class="form-grid" onSubmit={(e) => { e.preventDefault(); if (!invalid) void save(); }}>
        <Field label="下载目录" htmlFor="dl-dir" error={errs.download_dir} hint="文件名中的 Windows 非法字符由后端安全转义。">
          <input id="dl-dir" class="input mono" value={form.download_dir} onInput={(e) => setForm({ ...form, download_dir: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label="同时传输数" htmlFor="dl-max" error={errs.max} hint="默认保守。遇到限流时后端不会自动加大并发。">
          <input id="dl-max" class="input narrow" type="number" min={1} max={8} value={form.max} onInput={(e) => setForm({ ...form, max: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label="带宽上限（KiB/s）" htmlFor="dl-bw" error={errs.bw} hint="0 表示不限制。作用于 NekoPak 自身的传输。">
          <input id="dl-bw" class="input narrow" type="number" min={0} value={form.bw} onInput={(e) => setForm({ ...form, bw: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label="缓存目录" htmlFor="cache-dir" error={errs.cache_dir}>
          <input id="cache-dir" class="input mono" value={form.cache_dir} onInput={(e) => setForm({ ...form, cache_dir: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label="缓存上限（GiB）" htmlFor="cache-max" error={errs.cache_gib} hint="按需文件与预览缓存共用此上限，超出后按最近最少使用淘汰。">
          <input id="cache-max" class="input narrow" type="number" min={1} value={form.cache_gib} onInput={(e) => setForm({ ...form, cache_gib: (e.target as HTMLInputElement).value })} />
        </Field>
        <div class="form-actions">
          <Btn label="清理缓存" icon="trash" disabled reason="契约中尚无清理缓存命令（提案 UI-001 §P9）" />
          <Btn type="submit" tone="primary" label="保存" busy={busy} disabled={invalid} reason="请先修正标红的字段" />
        </div>
      </form>
    </Section>
  );
}

function Network() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <Section title="高级网络诊断" desc="实验网络设置默认关闭。"><div class="inline-note">当前 Rust 后端没有网络设置服务，不能通过此页修改代理、DNS或日志。外部 CLI 的网络配置由其自身管理。</div><Switch label="启用高级网络设置" checked={false} disabled reason="后端尚未实现" onChange={() => undefined} /></Section>;
  return <BrowserNetwork />;
}

function BrowserNetwork() {
  const settings = useStore(app, (s) => s.settings);
  if (!settings) return <Section title="高级网络诊断"><Spinner /></Section>;
  const n = settings.experimental_network;
  const set = (patch: Partial<Settings["experimental_network"]>) =>
    updateSettings({ experimental_network: patch }).catch((e) => toast(errorOf(e).message, "error", { error: errorOf(e) }));

  return (
    <Section title="高级网络诊断" desc="默认关闭。这里的设置只影响 NekoPak 自己发出的请求，不能影响独立运行的官方 CLI，也不会改动系统设置。">
      <Row icon="network" label="启用高级网络设置" hint="开启后才能修改下面的选项。关闭时 NekoPak 使用系统网络设置。">
        <Switch label="启用高级网络设置" checked={n.enabled} onChange={async (v) => {
          if (v) {
            const ok = await confirmDialog({
              title: "启用高级网络设置？",
              body: <>
                <p>这些选项只作用于 NekoPak 进程内的请求，可以随时关闭恢复默认。</p>
                <ul class="plain-list">
                  <li>不会修改系统 Hosts、DNS、注册表、防火墙或证书。</li>
                  <li>不会绕过验证码、地区限制或账号风控。</li>
                  <li>对独立运行的官方 CLI 无效。</li>
                </ul>
              </>,
              confirmLabel: "启用",
            });
            if (!ok) return;
          }
          void set({ enabled: v });
        }} />
      </Row>
      <fieldset class="set-subgroup" aria-disabled={n.enabled ? undefined : "true"}>
        <Row icon="network" label="代理" hint={n.proxy_mode === "user_provided" ? "代理地址保存在系统安全存储中，不写入配置文件。" : "“系统”使用 Windows 代理设置。"}>
          <Segmented<Settings["experimental_network"]["proxy_mode"]> label="代理模式" value={n.proxy_mode} onChange={(v) => n.enabled && void set({ proxy_mode: v })}
            options={[
              { value: "system", label: "系统", disabled: !n.enabled, reason: "请先启用高级网络设置" },
              { value: "none", label: "不使用", disabled: !n.enabled, reason: "请先启用高级网络设置" },
              { value: "user_provided", label: "自定义", disabled: true, reason: "代理地址需要安全存储命令（提案 UI-001 §P9），M0 暂不开放" },
            ]} />
        </Row>
        <Row icon="backend" label="自定义 DNS" hint="尚未经过审计，暂不能开启。不会修改系统 DNS。">
          <Switch label="自定义 DNS" checked={false} disabled reason="自定义 DNS 尚未审计" onChange={() => undefined} />
        </Row>
        <Row icon="doc" label="连接诊断日志" hint="记录 NekoPak 请求的连通性与耗时（已脱敏），便于排查。">
          <Switch label="连接诊断日志" checked={n.connection_diagnostics} disabled={!n.enabled} reason="请先启用高级网络设置" onChange={(v) => void set({ connection_diagnostics: v })} />
        </Row>
        <Row icon="shield" label="连通性检查" hint="检查 DNS、TLS 与后端可达性。">
          <Btn icon="shield" label="运行检查" disabled reason="契约中尚无诊断命令（提案 UI-001 §P9）" />
        </Row>
      </fieldset>
    </Section>
  );
}

function Shortcuts() {
  return (
    <Section title="快捷键">
      <table class="kbd-table">
        <tbody>
          {SHORTCUTS.map(([k, v]) => <tr><th scope="row"><kbd class="kbd">{k}</kbd></th><td>{v}</td></tr>)}
        </tbody>
      </table>
    </Section>
  );
}

function Developer() {
  const s = useStore(app, (x) => x);
  return (
    <Section title="开发者" desc="运行时与接口信息，用于和 Rust 核心对齐契约。">
      <dl class="kv">
        <dt>运行时</dt><dd>{s.runtime === "mock" ? "Mock（前端内置样本，未调用任何真实服务）" : "Tauri IPC"}</dd>
        <dt>应用版本</dt><dd>{s.app?.version ?? "—"}</dd>
        <dt>契约版本</dt><dd>前端 {CONTRACT_VERSION}，后端 {s.app?.contract_version ?? "—"}{s.app && s.app.contract_version !== CONTRACT_VERSION ? <Badge tone="warn">不一致</Badge> : null}</dd>
        <dt>存储</dt><dd>{s.app?.storage.status === "ok" ? "正常" : s.app?.storage.status ?? "—"}{s.app?.storage.message ? `：${s.app.storage.message}` : ""}</dd>
        <dt>提案中的命令</dt><dd><code class="wrap">{[...PROPOSED_COMMANDS].join(", ")}</code></dd>
      </dl>
      <div class="row-gap pad-top">
        {s.runtime === "mock" ? <Btn icon="flask" label="Mock 场景与故障注入" onClick={() => void openMockPanel()} /> : null}
        <Btn icon="download" label="导出脱敏诊断" disabled reason="需要后端提供日志导出命令（提案 UI-001 §P9）" />
      </div>
      <details class="pad-top"><summary>开发测试与连接诊断</summary>
        {s.caps.map(c => <article class="backend-card"><h3>{c.backend === "mock" ? "Mock · 仅离线模拟，不是实际账号或服务能力" : backendLabel(c.backend)}</h3>
          <p class="muted">{c.backend === "mock" ? "以下项目仅描述测试样本支持的模拟动作，不表示真实功能已实现。" : `诊断状态：${c.status}`}</p>
          <ul class="op-chips">{OPS.map(o => <li class="no">{o.label}：{c.operations[o.op] ? c.backend === "mock" ? "可模拟" : "当前可用" : "未启用"}</li>)}</ul>
          <ul class="bo-limits">{c.limitations.map(l => <li>{l}</li>)}</ul></article>)}
      </details>
    </Section>
  );
}
