import { LANGUAGES, tr, tx } from "../lib/i18n";
import type { Language } from "../lib/i18n";
import { useEffect, useState } from "preact/hooks";
import { CONTRACT_VERSION } from "../lib/contract";
import type { NetworkDomainReport, ServerDomain, QualityCap, QuotaItem, Settings } from "../lib/contract-proposals";
import { PROPOSED_COMMANDS } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { formatBytes, formatDateTime, parseU64, percent } from "../lib/format";
import { useStore } from "../lib/store";
import { app, applyTheme, hostSupports, isMobileHost, refreshQuota, resync, updateSettings } from "../state/app";
import { navigate, type SettingsSection } from "../state/route";
import { toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Badge, Btn, ErrorPanel, Field, Progress, Segmented, Spinner, Switch } from "../components/ui";
import { SHORTCUTS } from "../components/dialogs";
import { AuthPanel, SavedAccounts } from "./Auth";
import { CAP_LABEL } from "./download";
import { openMockPanel } from "./lazy";
import { useDensity } from "./Files";
import { AboutContent } from "./About";
import { PageFrame, PageTitle } from "../components/Frame";
import { prefs, setPref } from "../state/prefs";
import { transfers } from "../state/transfers";
import { ACCENTS } from "../styles/accents.generated";
import { useMobileLayout } from "../lib/mobile";

const SECTIONS: { id: SettingsSection; label: string; icon: string }[] = [
  { id: "account", get label() { return tr("账号"); }, icon: "user" },
  { id: "general", get label() { return tr("外观"); }, icon: "palette" },
  { id: "transfers", get label() { return tr("下载与传输"); }, icon: "download" },
  { id: "network", get label() { return tr("网络"); }, icon: "network" },
  { id: "quota", get label() { return tr("配额"); }, icon: "quota" },
  { id: "shortcuts", get label() { return tr("快捷键"); }, icon: "keyboard" },
  { id: "developer", get label() { return tr("开发者"); }, icon: "flask" },
  { id: "about", get label() { return tr("关于"); }, icon: "info" },
];

/** The developer section is for wiring the Rust core; hidden in release builds with a real core. */
function showDev(): boolean {
  return import.meta.env.DEV || app.get().runtime === "mock";
}

export function SettingsPage({ section }: { section: SettingsSection }) {
  const phone = useMobileLayout();
  if(phone)return <PageFrame label={section==="language"?tr("界面语言"):SECTIONS.find(s=>s.id===section)?.label??tr("设置")} caption={null} class="phone-settings">
    <div class="phone-settings-body">
      {section==="general"?<PhoneAppearance/>:section==="language"?<LanguageSettings/>:section==="account"||section==="backends"?<><AuthPanel/><SavedAccounts/></>:section==="quota"?<Quota/>:section==="transfers"?<TransfersSettings/>:section==="network"?<Network/>:section==="about"?<AboutContent/>:<PhoneAppearance/>}
    </div>
  </PageFrame>;
  return (
    <PageFrame label={tr("设置")} class="settings-page" caption={<PageTitle>{tr("设置")}</PageTitle>}>
      <div class="settings-layout">
        <nav class="settings-nav" aria-label={tr("设置分类")}>
          {SECTIONS.filter((x) => x.id !== "developer" || showDev()).map((x) => (
            <a href={`#/settings/${x.id}`} class={`settings-link${section === x.id ? " on" : ""}`} aria-current={section === x.id ? "page" : undefined}
              onClick={(e) => { e.preventDefault(); navigate({ page: "settings", section: x.id }); }}>
              <Icon name={x.icon} filled={section === x.id} />
              <span>{x.label}</span>
            </a>
          ))}
        </nav>
        <div class="settings-body">
          {section === "general" ? <General /> : section === "language" ? <LanguageSettings/>
            : section === "account" || section === "backends" ? <Section title={tr("账号")}><div class="set-card"><AuthPanel /></div><SavedAccounts/></Section>
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

function LanguageSettings() {
  const settings=useStore(app,s=>s.settings);
  return <div class="mobile-settings-group">{LANGUAGES.map(item=><button type="button" class="mobile-setting-link" aria-pressed={settings?.language===item.value} onClick={()=>void updateSettings({language:item.value}).catch(e=>toast(errorOf(e).message,"error"))}><span>{item.value==="system"?tr(item.label):item.label}</span>{settings?.language===item.value?<Icon name="check"/>:null}</button>)}</div>;
}

function PhoneAppearance() {
  const settings=useStore(app,s=>s.settings), accent=useStore(prefs,p=>p.accent), reduce=useStore(prefs,p=>p.reduceMotion);
  return <div class="phone-appearance"><h2>{tr("主题")}</h2><div class="phone-themes">{(["system","light","dark"] as const).map(value=><button type="button" aria-pressed={settings?.theme===value} onClick={()=>void updateSettings({theme:value}).catch(e=>toast(errorOf(e).message,"error"))}><Icon name={value==="dark"?"moon":value==="light"?"sun":"monitor"}/><span>{tr(value==="system"?"跟随系统":value==="light"?"浅色":"深色")}</span></button>)}</div><h2>{tr("强调色")}</h2><div class="swatches">{ACCENTS.map(a=><button type="button" class={`swatch${accent===a.id?" on":""}`} style={{"--sw":a.seed} as preact.JSX.CSSProperties} aria-label={tr(a.label)} aria-pressed={accent===a.id} onClick={()=>setPref("accent",a.id)}><span class="swatch-dot">{accent===a.id?<Icon name="check"/>:null}</span><span>{tr(a.label)}</span></button>)}</div><Row label={tr("减少动画")}><Switch label={tr("减少动画")} checked={reduce} onChange={v=>setPref("reduceMotion",v)}/></Row></div>;
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
  const tray = useStore(app, s => hostSupports("tray", s));
  const accent = useStore(prefs, (p) => p.accent);
  const reduce = useStore(prefs, (p) => p.reduceMotion);
  const foldersFirst = useStore(prefs, (p) => p.foldersFirst);
  const gridSize = useStore(prefs, (p) => p.gridSize);
  const clipWatch = useStore(prefs, (p) => p.clipboardWatch);
  const [, density, setDensity] = useDensity();
  const theme = settings?.theme ?? "system";
  return (
    <Section title={tr("外观")}>
      <Row icon="dark" label={tr("主题")}>
        <Segmented<Settings["theme"]> label={tr("主题")} value={theme}
          onChange={(t) => {
            applyTheme(t);
            updateSettings({ theme: t }).catch((e) => toast(errorOf(e).message, "error"));
          }}
          options={[
            { value: "system", label: tr("跟随系统"), icon: "monitor" },
            { value: "light", label: tr("浅色"), icon: "sun" },
            { value: "dark", label: tr("深色"), icon: "moon" },
          ]} />
      </Row>
      <Row icon="palette" label={tr("强调色")} below>
        <div class="swatches" role="radiogroup" aria-label={tr("强调色")}>
          {ACCENTS.map((a) => (
            <button type="button" role="radio" aria-checked={accent === a.id ? "true" : "false"} class={`swatch${accent === a.id ? " on" : ""}`}
              style={{ "--sw": a.seed } as preact.JSX.CSSProperties} title={tr(a.label)} onClick={() => setPref("accent", a.id)}>
              <span class="swatch-dot">{accent === a.id ? <Icon name="check" size={18} /> : null}</span>
              <span class="swatch-label">{tr(a.label)}</span>
            </button>
          ))}
        </div>
      </Row>
      <Row icon="rows" label={tr("列表行距")}>
        <Segmented label={tr("列表行距")} value={density} onChange={setDensity}
          options={[{ value: "comfortable", label: tr("舒适") }, { value: "compact", label: tr("紧凑") }]} />
      </Row>
      <Row icon="folder" label={tr("文件夹排在前面")}>
        <Switch label={tr("文件夹始终排在前面")} checked={foldersFirst} onChange={(v) => setPref("foldersFirst", v)} />
      </Row>
      <Row icon="grid" label={tr("网格卡片大小")} hint={tr("在文件列表里按住 Ctrl 滚动滚轮也能切换。")}>
        <Segmented label={tr("网格卡片大小")} value={gridSize} onChange={(v) => setPref("gridSize", v)}
          options={[{ value: "s", label: tr("小") }, { value: "m", label: tr("中") }, { value: "l", label: tr("大") }]} />
      </Row>
      <Row icon="link" label={tr("检测剪贴板中的链接")} hint={tr("切回 NekoPak 时，如果剪贴板里有磁力或 http(s) 链接，询问是否添加。")}>
        <Switch label={tr("检测剪贴板中的链接")} checked={clipWatch} onChange={(v) => setPref("clipboardWatch", v)} />
      </Row>
      <Row icon="motion" label={tr("减少动画")}>
        <Switch label={tr("减少动画")} checked={reduce} onChange={(v) => setPref("reduceMotion", v)} />
      </Row>
      <Row icon="label" label={tr("界面语言")}>
        <select class="select" aria-label={tr("界面语言")} value={settings?.language??"system"} onChange={e=>void updateSettings({language:(e.target as HTMLSelectElement).value as Language}).catch(e=>toast(errorOf(e).message,"error"))}>
          {LANGUAGES.map(item => <option value={item.value}>{item.value === "system" ? tr(item.label) : item.label}</option>)}
        </select>
      </Row>
      <Row icon="monitor" label={tr("开机自动启动")}>
        <AutostartSetting/>
      </Row>
      <Row icon="minimize" label={tr("关闭窗口时最小化到托盘")}>
        <Switch label={tr("关闭窗口时最小化到托盘")} checked={settings?.close_to_tray??false} disabled={!tray} reason={tr("Android 不提供系统托盘")} onChange={v=>void updateSettings({close_to_tray:v}).catch(e=>toast(errorOf(e).message,"error"))} />
      </Row>
    </Section>
  );
}

function AutostartSetting() {
  const enabled = useStore(app, s => s.runtime === "tauri" && !!s.app?.feature_flags.autostart);
  const [status, setStatus] = useState<{ enabled: boolean; available: boolean }>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ReturnType<typeof errorOf>>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let alive = true; setError(undefined);
    void api.autostartGet().then(value => { if (alive) setStatus(value); }, e => { if (alive) setError(errorOf(e)); });
    return () => { alive = false; };
  }, [enabled, revision]);
  const set = async (value: boolean) => {
    if (busy || !status?.available) return;
    setBusy(true); setError(undefined);
    try { setStatus(await api.autostartSet(value)); }
    catch (e) { setError(errorOf(e)); }
    finally { setBusy(false); }
  };
  return <div class="autostart-setting"><Switch label={tr("开机自动启动")} checked={status?.enabled ?? false} disabled={!enabled || !status?.available || busy} reason={busy ? tr("正在保存") : !enabled ? tr("当前平台尚未支持自动启动") : !status ? tr("正在读取自动启动设置") : tr("当前用户自动启动不可用")} onChange={value => void set(value)}/>
    <p class="set-hint">{tr("仅为当前 Windows 用户设置登录后启动 NekoPak。")}</p>
    {error ? <ErrorPanel error={error} compact onRetry={() => setRevision(value => value + 1)}/> : null}
  </div>;
}

const DIM_LABEL: Record<QuotaItem["dimension"], string> = {
  get storage() { return tr("存储容量"); },
  get downstream_daily() { return tr("每日下行"); },
  get downstream_monthly() { return tr("每月下行"); },
  get upload_monthly() { return tr("每月上传"); },
  get cloud_download_monthly() { return tr("每月云下载"); },
};

function QuotaRow({ q }: { q: QuotaItem }) {
  const pct = q.unit === "bytes" ? percent(q.used, q.limit) : (() => {
    const u = parseU64(q.used ?? null);
    const l = parseU64(q.limit ?? null);
    return u !== null && l !== null && l > 0n ? Number((u * 1000n) / l) / 10 : null;
  })();
  const fmt = (v?: string) => (v === undefined ? "?" : q.unit === "bytes" ? formatBytes(v) : tx("{0} 次", v));
  return (
    <li class={`quota-row q-${q.state}`}>
      <div class="quota-top">
        <strong>{DIM_LABEL[q.dimension]}</strong>
        {q.state === "exhausted" ? <Badge tone="error" icon="alert">{tr("已用尽")}</Badge>
          : q.state === "near_limit" ? <Badge tone="warn" icon="alert">{tr("接近上限")}</Badge>
            : q.exactness === "estimated" ? <Badge tone="warn">{tr("估计值")}</Badge>
              : q.exactness === "unavailable" ? <Badge>{tr("不可获取")}</Badge> : <Badge tone="ok">{tr("正常")}</Badge>}
      </div>
      {q.exactness === "unavailable" ? (
        <p class="muted">{q.note ?? tr("暂时无法获取")}</p>
      ) : (
        <>
          {q.limit ? <Progress value={pct} tone={q.state === "exhausted" ? "error" : q.state === "near_limit" ? "warn" : undefined} label={tx("{0} 已用", DIM_LABEL[q.dimension])} /> : null}
          <p class="tabular">{tr("已用")} {fmt(q.used)}{q.limit ? ` / ${fmt(q.limit)}` : tr("（上限未提供）")}{q.remaining ? tx("，剩余 {0}", fmt(q.remaining)) : ""}
          </p>
          {q.resets_at ? <p class="muted">{tr("重置时间")} {formatDateTime(q.resets_at)}</p> : null}
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
    <Section title={tr("配额")} desc={s.runtime === "mock" ? tr("当前为演示数据。") : undefined}>
      <div class="row-gap pad-bottom">
        <Btn icon="refresh" label={tr("刷新")} busy={s.quotaLoading} onClick={() => void refreshQuota()} />
        {s.quota ? <span class="muted">{tr("更新于")} {formatDateTime(s.quota.checked_at)}</span> : null}
      </div>
      {s.quotaError ? <ErrorPanel error={s.quotaError} onRetry={() => void refreshQuota()} />
        : s.auth?.state !== "signed_in" && s.auth?.state !== "demo" ? <p class="muted">{tr("登录后可查询配额。")}</p>
          : !s.quota ? <Spinner />
            : <ul class="quota-list">{s.quota.items.map((q) => <QuotaRow q={q} />)}</ul>}
    </Section>
  );
}

const ABS = /^([a-zA-Z]:\\|%[A-Z]+%\\|\/)/;
const GiB = 1024n ** 3n;

function TransfersSettings() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <NativeTransfersSettings />;
  return <BrowserTransferSettings />;
}

function NativeTransfersSettings() {
  const phone=useMobileLayout();
  const settings = useStore(app, s => s.settings);
  const mobile = useStore(app, isMobileHost);
  const directoryPicker = useStore(app, s => hostSupports("directory_picker", s));
  const playerPicker = useStore(app, s => hostSupports("player_config", s));
  const keepAlive = useStore(app, s => isMobileHost(s) && hostSupports("background_transfers", s));
  const [background,setBackground]=useState(settings?.background_transfers??true);
  const [dir, setDir] = useState(settings?.download_dir ?? "");
  const [bandwidth, setBandwidth] = useState(String(settings?.bandwidth_limit_kib_s ?? 0));
  const [concurrent, setConcurrent] = useState(String(settings?.max_concurrent_transfers ?? 1));
  const [connections, setConnections] = useState(String(settings?.download?.connections_per_task ?? 1));
  const valid = Number.isInteger(Number(concurrent)) && Number(concurrent) >= 1 && Number(concurrent) <= 8
    && Number.isInteger(Number(connections)) && Number(connections) >= 1 && Number(connections) <= 16;
  const [busy, setBusy] = useState(false);
  const [picking,setPicking]=useState(false);
  const [ask,setAsk]=useState(settings?.ask_download_directory??false);
  const [quality,setQuality]=useState<QualityCap>(settings?.download?.default_quality??"original");
  const [player,setPlayer]=useState(settings?.media?.player_path??"");
  const pick=async()=>{setPicking(true);try{const chosen=await api.downloadDirectoryPick();if(chosen!==null)setDir(chosen);}catch(e){toast(errorOf(e).message,"error");}finally{setPicking(false);}};
  const save = async () => {
    setBusy(true);
    try { await updateSettings({ ...(directoryPicker ? {download_dir: dir, ask_download_directory:ask} : {}), bandwidth_limit_kib_s: Number(bandwidth),max_concurrent_transfers: Number(concurrent),download:{default_quality:quality,connections_per_task:Number(connections)},...(keepAlive ? {background_transfers:background} : {}),...(playerPicker ? {media:{player_path:player}} : {}) }); toast(tr("下载设置已保存"), "success"); }
    catch (e) { toast(errorOf(e).message, "error", { error: errorOf(e) }); }
    finally { setBusy(false); }
  };
  return <Section title={tr("下载与传输")}>
    {mobile ? <p class="mobile-storage-note">{tr("文件先保存到应用内。下载完成后，在任务菜单中选择“导出文件”。卸载前请导出需要保留的文件。")}</p> : null}
    <form class="form-grid" onSubmit={e => { e.preventDefault(); if (!busy && valid) void save(); }}>
      {!phone ? <>
      <Field label={tr("下载目录")} htmlFor="native-download-dir" hint={tr(directoryPicker ? "点击路径或“选择文件夹”浏览目录。同名文件不会被覆盖。" : "应用私有目录；完成后通过系统保存对话框导出。") }>
        <div class="directory-picker"><input id="native-download-dir" class="input mono" value={dir} readOnly onClick={directoryPicker ? ()=>void pick() : undefined} aria-label={tr("下载目录")} />
        <Btn icon="folderOpen" label={tr("选择文件夹")} disabled={!directoryPicker} reason={tr("Android 当前版本使用应用私有下载目录")} busy={picking} onClick={()=>void pick()} /></div>
      </Field>
      </> : null}
      <Field label={tr("带宽上限（KiB/s）")} htmlFor="native-bandwidth" hint={tr("0 表示不限速，修改后对后续下载块生效。")}>
        <input id="native-bandwidth" class="input" type="number" min={0} max={1048576} value={bandwidth} onInput={e => setBandwidth((e.target as HTMLInputElement).value)} />
      </Field>
      <Field label={tr("同时传输数")} htmlFor="native-concurrent" hint={tr("同时进行的任务数，1–8。") }>
        <input id="native-concurrent" class="input narrow" type="number" min={1} max={8} required value={concurrent} onInput={e => setConcurrent(e.currentTarget.value)} />
      </Field>
      <Field label={tr("每个任务的连接数")} htmlFor="native-connections" hint={tr("多个连接分段下载同一个文件，1–16。") }>
        <input id="native-connections" class="input narrow" type="number" min={1} max={16} required value={connections} onInput={e => setConnections(e.currentTarget.value)} />
      </Field>
      {!phone ? <Row label={tr("下载时询问保存位置")}><Switch label={tr("每次下载选择文件夹")} checked={ask} disabled={!directoryPicker} reason={tr("Android 当前版本使用应用私有下载目录")} onChange={setAsk} /></Row> : null}
      <Field label={tr("默认下载画质")} htmlFor="native-quality"><select id="native-quality" class="select" value={quality} onChange={e=>setQuality((e.target as HTMLSelectElement).value as QualityCap)}>{(["original","2160p","1080p","720p","480p"] as QualityCap[]).map(value=><option value={value}>{CAP_LABEL[value]}</option>)}</select></Field>
      {!phone ? <>
      <Field label={tr("外部播放器")} htmlFor="native-player" hint={tr(playerPicker ? "选择已安装的播放器。媒体地址由 Rust 交给播放器，不经过命令行脚本。" : "Android 当前版本尚未支持外部播放器") }>
        <div class="directory-picker"><input id="native-player" class="input mono" value={player} readOnly/><Btn label={tr("选择播放器")} icon="folderOpen" disabled={!playerPicker} reason={tr("Android 当前版本尚未支持外部播放器")} onClick={()=>void api.playerConfigPick().then(value=>{if(value!==null)setPlayer(value);},e=>toast(errorOf(e).message,"error"))}/></div>
        {player?<Btn label={tr("清除播放器配置")} size="sm" tone="quiet" onClick={()=>setPlayer("")}/>:null}
      </Field>
      </> : null}
      <p class="download-help">{tr("下载原始文件，支持暂停、继续及重启后恢复。")}</p>
      {keepAlive ? <Row label={tr("后台继续传输")}><Switch label={tr("后台继续传输")} checked={background} onChange={setBackground} /></Row> : null}
      {mobile ? <p class="download-help">{keepAlive && background ? tr("切到后台或锁屏后，本地传输通过系统通知继续运行，可在通知中全部暂停。系统收回后台时间或划掉应用时会暂停，返回后请手动继续。云端任务始终由服务端处理。") : tr("切到后台或锁屏会暂停本地传输，返回后请手动继续。云端任务继续由服务端处理。")}</p> : null}
      <div class="form-actions"><Btn type="submit" tone="primary" label={tr("保存")} busy={busy} disabled={!valid} /></div>
    </form>
  </Section>;
}

function BrowserTransferSettings() {
  const settings = useStore(app, (s) => s.settings);
  const [form, setForm] = useState(() => settings && {
    download_dir: settings.download_dir,
    max: String(settings.max_concurrent_transfers),
    bw: String(settings.bandwidth_limit_kib_s),
    cache_dir: settings.cache.dir,
    cache_gib: String(Number((parseU64(settings.cache.max_bytes) ?? 0n) / GiB)),
    quality: settings.download?.default_quality ?? "original",
    conns: String(settings.download?.connections_per_task ?? 4),
  });
  const [busy, setBusy] = useState(false);
  if (!settings || !form) return <Section title={tr("下载与传输")}><Spinner /></Section>;

  const max = Number(form.max);
  const bw = Number(form.bw);
  const cg = Number(form.cache_gib);
  const errs = {
    download_dir: ABS.test(form.download_dir) ? undefined : tr("需要绝对路径"),
    max: Number.isInteger(max) && max >= 1 && max <= 8 ? undefined : tr("请输入 1–8 之间的整数"),
    bw: Number.isInteger(bw) && bw >= 0 ? undefined : tr("请输入不小于 0 的整数"),
    cache_dir: ABS.test(form.cache_dir) ? undefined : tr("需要绝对路径"),
    cache_gib: Number.isInteger(cg) && cg >= 1 && cg <= 4096 ? undefined : tr("请输入 1–4096 之间的整数"),
    conns: Number.isInteger(Number(form.conns)) && Number(form.conns) >= 1 && Number(form.conns) <= 16 ? undefined : tr("请输入 1–16 之间的整数"),
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
        download: { default_quality: form.quality as QualityCap, connections_per_task: Number(form.conns) },
      });
      toast(tr("下载与传输设置已保存"), "success");
    } catch (e) {
      toast(errorOf(e).message, "error", { error: errorOf(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title={tr("下载与传输")}>
      <form class="form-grid" onSubmit={(e) => { e.preventDefault(); if (!invalid) void save(); }}>
        <Field label={tr("下载目录")} htmlFor="dl-dir" error={errs.download_dir}>
          <input id="dl-dir" class="input mono" value={form.download_dir} onInput={(e) => setForm({ ...form, download_dir: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label={tr("默认下载画质")} htmlFor="dl-q" hint={tr("下载视频时使用不高于此档的最高画质。右键“下载指定画质…”可临时改变。")}>
          <select id="dl-q" class="select" value={form.quality} onChange={(e) => setForm({ ...form, quality: (e.target as HTMLSelectElement).value as QualityCap })}>
            {(["original", "2160p", "1080p", "720p", "480p"] as QualityCap[]).map((c) => <option value={c}>{c === "original" ? tr("原画（原始文件）") : tx("不高于 {0}", CAP_LABEL[c])}</option>)}
          </select>
        </Field>
        <Field label={tr("同时传输数")} htmlFor="dl-max" error={errs.max} hint={tr("同时进行的任务数，1–8。")}>
          <input id="dl-max" class="input narrow" type="number" min={1} max={8} value={form.max} onInput={(e) => setForm({ ...form, max: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label={tr("每个任务的连接数")} htmlFor="dl-conn" error={errs.conns} hint={tr("多个连接分段下载同一个文件，1–16。")}>
          <input id="dl-conn" class="input narrow" type="number" min={1} max={16} value={form.conns} onInput={(e) => setForm({ ...form, conns: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label={tr("带宽上限（KiB/s）")} htmlFor="dl-bw" error={errs.bw} hint={tr("0 表示不限制。")}>
          <input id="dl-bw" class="input narrow" type="number" min={0} value={form.bw} onInput={(e) => setForm({ ...form, bw: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label={tr("缓存目录")} htmlFor="cache-dir" error={errs.cache_dir}>
          <input id="cache-dir" class="input mono" value={form.cache_dir} onInput={(e) => setForm({ ...form, cache_dir: (e.target as HTMLInputElement).value })} />
        </Field>
        <Field label={tr("缓存上限（GiB）")} htmlFor="cache-max" error={errs.cache_gib} hint={tr("超出后先清理最久没用的缓存。")}>
          <input id="cache-max" class="input narrow" type="number" min={1} value={form.cache_gib} onInput={(e) => setForm({ ...form, cache_gib: (e.target as HTMLInputElement).value })} />
        </Field>
        <div class="form-actions">
          <Btn label={tr("清理缓存")} icon="trash" disabled reason={tr("暂不支持")} />
          <Btn type="submit" tone="primary" label={tr("保存")} busy={busy} disabled={invalid} reason={tr("请先修正标红的字段")} />
        </div>
      </form>
    </Section>
  );
}

function Network() {
  const desktop=useStore(app,s=>s.runtime==="tauri");
  if(desktop)return <NativeNetwork />;
  return <BrowserNetwork />;
}

function NativeNetwork(){
  const settings=useStore(app,s=>s.settings);
  const n=settings?.experimental_network;
  const [enabled,setEnabled]=useState(n?.enabled??false);
  const [mode,setMode]=useState(n?.proxy_mode??"system");
  const [url,setUrl]=useState(n?.proxy_url??"");
  const [compatibility,setCompatibility]=useState(n?.compatibility_mode??false);
  const [blockAccessDomains,setBlockAccessDomains]=useState(n?.block_access_domains??false);
  const [serverDomain,setServerDomain]=useState<ServerDomain>(n?.server_domain??"auto");
  const [domainReport,setDomainReport]=useState<NetworkDomainReport>();
  const [probing,setProbing]=useState(false);
  const [reconnecting,setReconnecting]=useState(false);
  const [busy,setBusy]=useState(false);
  const save=async()=>{setBusy(true);try{await updateSettings({experimental_network:{enabled,proxy_mode:mode,proxy_url:url.trim(),compatibility_mode:compatibility,block_access_domains:blockAccessDomains,server_domain:serverDomain}});toast(tr("网络设置已保存"),"success");}catch(e){toast(errorOf(e).message,"error",{error:errorOf(e)});}finally{setBusy(false);}};
  const probe=async()=>{setProbing(true);try{setDomainReport(await api.networkDomainsProbe());}catch(e){toast(errorOf(e).message,"error");}finally{setProbing(false);}};
  const reconnect=async()=>{setReconnecting(true);try{await api.networkReconnect();await resync();toast(tr("已重新连接"),"success");}catch(e){toast(errorOf(e).message,"error");}finally{setReconnecting(false);}};
  return <Section title={tr("网络")}>
    <Field label={tr("服务器域名")} htmlFor="server-domain" hint={tr("保存后用于后续请求。测速使用已保存的网络设置。") }><select id="server-domain" class="select" value={serverDomain} onChange={e=>setServerDomain((e.target as HTMLSelectElement).value as ServerDomain)}>{(["auto","mypikpak.com","mypikpak.net","pikpak.me","pikpakdrive.com"] as ServerDomain[]).map(domain=><option value={domain}>{domain==="auto"?tr("自动选择"):domain}</option>)}</select></Field>
    <div class="phone-network-actions"><Btn label={tr("测速")} icon="network" busy={probing} onClick={()=>void probe()}/><Btn label={tr("重新连接")} icon="refresh" busy={reconnecting} onClick={()=>void reconnect()}/></div>
    {domainReport?<div class="phone-domain-results"><p>{tr("当前连接")}: <strong>{domainReport.active_domain}</strong></p>{domainReport.results.map(result=><div key={result.domain}>
      <code>{result.domain}</code><strong>{tr(result.usable?"可用":"不可用")}</strong>
      <small>{tr("文件")}: {result.latency_ms===undefined?"—":`${result.latency_ms} ms`} · {tr("复用连接")}: {result.warm_latency_ms===undefined?"—":`${result.warm_latency_ms} ms`} · {tr("账号")}: {result.auth_latency_ms===undefined?"—":`${result.auth_latency_ms} ms`}</small>
      {result.error?<small>{tr(result.error)}</small>:null}
    </div>)}</div>:null}

    <Row icon="network" label={tr("网络增强")} hint={tr("默认关闭。按你的网络环境选择应用内代理或兼容连接。")}><Switch label={tr("启用网络增强")} checked={enabled} onChange={setEnabled}/></Row>
    <Row label={tr("代理")}><Segmented label={tr("代理")} value={mode} onChange={setMode} options={[{value:"system",label:tr("跟随系统")},{value:"none",label:tr("不使用")},{value:"user_provided",label:tr("自定义")}]}/></Row>
    {mode==="user_provided"?<Field label={tr("代理地址")} htmlFor="proxy-url" hint={tr("支持 HTTP、HTTPS、SOCKS5；当前不保存代理账号密码。")}><input id="proxy-url" class="input mono" value={url} placeholder="http://127.0.0.1:7890" spellcheck={false} onInput={e=>setUrl((e.target as HTMLInputElement).value)}/></Field>:null}
    <Row label={tr("兼容连接")} hint={tr("限制认证接口使用 TLS 1.2，用于排查部分网络中的握手重置。保持证书校验；不保证解决所有地区的连接问题。")}><Switch label={tr("兼容连接")} checked={compatibility} onChange={setCompatibility}/></Row>
    <p class="download-help">{tr("设置只影响 NekoPak 后续连接。关闭网络增强后恢复系统代理，不修改 Hosts、DNS 或系统代理。")}</p>
    <AccessDomainBlocking checked={blockAccessDomains} onChange={setBlockAccessDomains}/>
    <div class="form-actions"><Btn label={tr("保存")} tone="primary" busy={busy} onClick={()=>void save()}/></div>
  </Section>;
}

function AccessDomainBlocking({ checked, onChange }: { checked: boolean; onChange: (value: boolean) => void }) {
  return <>
    <Row icon="shield" label={tr("屏蔽 access 域名")} hint={tr("默认关闭，独立于网络增强开关。只阻止 NekoPak 访问以下域名，不修改系统 Hosts。") }>
      <Switch label={tr("屏蔽 access 域名")} checked={checked} onChange={onChange}/>
    </Row>
    <ul class="access-domains" aria-label={tr("屏蔽域名列表")}>
      {["access.mypikpak.com", "access.mypikpak.net", "access.pikpak.me", "access.pikpakdrive.com"].map(domain => <li key={domain}><code>{domain}</code></li>)}
    </ul>
    <p class="download-help">{tr("当前直连路径不调用这些域名；此开关仅在出现相关请求时阻止连接。")}</p>
  </>;
}

function BrowserNetwork() {
  const settings = useStore(app, (s) => s.settings);
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (!settings) return <Section title={tr("网络")}><Spinner /></Section>;
  const n = settings.experimental_network;
  // Choosing anything other than the system proxy turns the override on; going back to "系统" turns it off.
  const set = (patch: Partial<Settings["experimental_network"]>) =>
    updateSettings({ experimental_network: patch }).catch((e) => toast(errorOf(e).message, "error", { error: errorOf(e) }));
  const mode = n.enabled ? n.proxy_mode : "system";
  return (
    <Section title={tr("网络")}>
      <Row icon="network" label={tr("代理")}>
        <Segmented<Settings["experimental_network"]["proxy_mode"]> label={tr("代理")} value={mode}
          onChange={(v) => void set(v === "system" ? { enabled: false, proxy_mode: "system" } : { enabled: true, proxy_mode: v })}
          options={[
            { value: "system", label: tr("跟随系统") },
            { value: "none", label: tr("不使用") },
            { value: "user_provided", label: tr("自定义"), disabled: true, reason: tr("暂不支持") },
          ]} />
      </Row>
      {desktop ? <p class="inline-note">{tr("只影响 NekoPak 后续请求，不修改系统代理、DNS 或 Hosts。已有请求按原设置结束；错误面板可复制诊断信息。")}</p> : <Row icon="doc" label={tr("连接诊断日志")} hint={tr("记录请求耗时与失败原因，便于排查网络问题。")}>
        <Switch label={tr("连接诊断日志")} checked={n.enabled && n.connection_diagnostics}
          onChange={(v) => void set(v ? { enabled: true, connection_diagnostics: true } : { connection_diagnostics: false })} />
      </Row>}
      <AccessDomainBlocking checked={n.block_access_domains ?? false} onChange={v => void set({ block_access_domains: v })}/>
    </Section>
  );
}

function Shortcuts() {
  return (
    <Section title={tr("快捷键")}>
      <table class="kbd-table">
        <tbody>
          {SHORTCUTS().map(([k, v]) => <tr><th scope="row"><kbd class="kbd">{k}</kbd></th><td>{v}</td></tr>)}
        </tbody>
      </table>
    </Section>
  );
}

function Developer() {
  const s = useStore(app, (x) => x);
  const dropped = useStore(transfers, (t) => t.dropped_events ?? 0);
  return (
    <Section title={tr("开发者")} desc={tr("运行时与接口信息，用于和 Rust 核心对齐契约。")}>
      <dl class="kv">
        <dt>{tr("运行时")}</dt><dd>{s.runtime === "mock" ? tr("Mock 传输层（演示模式：前端内置样本，未调用任何真实服务）") : "Tauri IPC"}</dd>
        <dt>{tr("应用版本")}</dt><dd>{s.app?.version ?? "—"}</dd>
        <dt>{tr("契约版本")}</dt><dd>{tr("前端")}{CONTRACT_VERSION}{tr("，Rust 核心")}{s.app?.contract_version ?? "—"}{s.app && s.app.contract_version !== CONTRACT_VERSION ? <Badge tone="warn">{tr("不一致")}</Badge> : null}</dd>
        <dt>{tr("存储")}</dt><dd>{s.app?.storage.status === "ok" ? tr("正常") : s.app?.storage.status ?? "—"}{s.app?.storage.message ? `：${s.app.storage.message}` : ""}</dd>
        <dt>{tr("已丢弃的过期事件")}</dt><dd class="tabular" data-dropped>{dropped}</dd>
        <dt>{tr("提案中的命令")}</dt><dd><code class="wrap">{[...PROPOSED_COMMANDS].join(", ")}</code></dd>
      </dl>
      <div class="row-gap pad-top">
        {s.runtime === "mock" ? <Btn icon="flask" label={tr("Mock 场景与故障注入")} onClick={() => void openMockPanel()} /> : null}
        <Btn icon="download" label={tr("导出诊断信息")} disabled reason={tr("暂不支持")} />
      </div>
      <details class="pad-top"><summary>{tr("开发测试与连接诊断")}</summary>
        {s.caps.filter(c => c.backend !== "official_cli").map(c => <article class="backend-card" key={c.backend}>
          <h3>{c.backend === "mock" ? tr("Mock · 仅离线模拟，不是实际账号或服务能力") : "PikPak"}</h3>
          <p class="muted">{tr("诊断状态：")}{c.status}</p>
          <ul class="op-chips">{Object.entries(c.operations).map(([op, enabled]) => <li key={op}>{op}：{enabled ? c.backend === "mock" ? tr("可模拟") : tr("当前可用") : tr("未启用")}</li>)}</ul>
          <ul class="bo-limits">{c.limitations.map(l => <li key={l}>{l}</li>)}</ul>
        </article>)}
      </details>
    </Section>
  );
}
