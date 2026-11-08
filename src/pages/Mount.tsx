import { tr, tx } from "../lib/i18n";
import { useEffect, useState } from "preact/hooks";
import type { ClientError } from "../lib/contract";
import type { MountCapabilities, MountConfig, MountModeReport, MountState } from "../lib/contract-proposals";
import { api, errorOf, openExternal } from "../lib/api";
import { formatRelative } from "../lib/format";
import { useStore } from "../lib/store";
import { app, signedIn } from "../state/app";
import { Icon } from "../components/Icon";
import { Badge, Btn, ErrorPanel, Field, Spinner } from "../components/ui";
import { useRefreshKey } from "./common";
import { PageFrame, PageTitle } from "../components/Frame";
import { confirmDialog, pickFolder } from "../components/dialogs";
import { openDialog, toast } from "../state/ui";
import { sessionGeneration } from "../state/session";

const MODE_COPY: Record<MountModeReport["mode"], { title: string; what: string; not: string; icon: string }> = {
  cloud_files_sync_root: {
    get title() { return tr("同步文件夹（按需文件）"); },
    icon: "folder",
    get what() { return tr("在资源管理器中注册一个同步根目录，云端文件显示为占位文件，打开时才下载内容。基于 Windows Cloud Files API。"); },
    get not() { return tr("这不是独立盘符：它是一个普通文件夹，位于你选择的位置。"); },
  },
  winfsp_drive: {
    get title() { return tr("虚拟盘符 P:"); },
    icon: "drive",
    get what() { return tr("通过 WinFsp 把云盘挂载成独立盘符，先只读。需要支持随机读取，否则播放器一打开就会下载整个文件。"); },
    get not() { return tr("需要先安装 WinFsp。"); },
  },
};

function ModeSection({ m, configured, states, onChanged, enabled }: { m: MountModeReport; configured: MountConfig[]; states: Record<string, MountState>; onChanged: () => void; enabled: boolean }) {
  const copy = MODE_COPY[m.mode];
  const failed = m.checks.filter((c) => c.status !== "pass");
  const [working, setWorking] = useState<string>();
  const [error, setError] = useState<ClientError>();
  const canConfigure = enabled && signedIn();
  const act = async (config: MountConfig, stop: boolean) => {
    const generation = sessionGeneration();
    if (stop && !await confirmDialog({ title: tr("停止挂载？"), body: <p>{tr(config.mode === "cloud_files_sync_root" ? "停止后保留同步根和占位文件。重新连接前，尚未下载的文件暂时无法打开。" : "停止后虚拟盘符将移除。云盘文件保持不变。")}</p>, confirmLabel: tr("停止挂载") })) return;
    if (generation !== sessionGeneration()) return;
    setWorking(config.config_id); setError(undefined);
    try {
      if (stop) await api.mountStop(config.config_id);
      else {
        const result = await api.mountStart(config.config_id, true);
        if (result.status === "error") throw result.error ?? { code: "INTERNAL", message: tr("挂载失败"), retryable: false };
      }
      if (generation === sessionGeneration()) onChanged();
    } catch (e) { if (generation === sessionGeneration()) setError(errorOf(e)); }
    finally { setWorking(undefined); }
  };
  return (
    <section class="mount-mode" aria-labelledby={`mm-${m.mode}`}>
      <header class="mount-head">
        <Icon name={copy.icon} size={20} />
        <div>
          <h2 id={`mm-${m.mode}`}>{copy.title}</h2>
          <p>{copy.what}</p>
          <p class="muted">{copy.not}</p>
        </div>
        <Badge tone={m.available ? "ok" : "neutral"} icon={m.available ? "checkCircle" : "lock"}>{m.available ? tr("可用") : tr("不可用")}</Badge>
      </header>
      <ul class="checks" aria-label={tx("{0} 前置检查", copy.title)}>
        {m.checks.map((c) => (
          <li class={`check-${c.status}`}>
            <Icon name={c.status === "pass" ? "checkCircle" : c.status === "fail" ? "alertCircle" : "help"} size={16} />
            <div>
              <span class="check-label">{c.label}</span>
              <span class="check-status">{c.status === "pass" ? tr("通过") : c.status === "fail" ? tr("未通过") : tr("未知")}</span>
              <span class="muted check-detail">{c.detail}</span>
              {c.requirement ? <span class="check-req"><Icon name="info" size={14} />{c.requirement}</span> : null}
            </div>
          </li>
        ))}
      </ul>
      {m.mode === "winfsp_drive" ? <p class="mount-notice">WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos<br/><button type="button" class="link-btn" onClick={() => void openExternal("https://github.com/winfsp/winfsp")}>https://github.com/winfsp/winfsp</button></p> : null}
      {enabled ? <p class="dim">{tr("前置检查通过后，启动仍会验证真实随机读取。重启应用不会自动挂载。")}</p> : null}
      {error ? <ErrorPanel error={error}/> : null}
      {configured.length ? <ul class="mount-configs">{configured.map(config => {
        const state = states[config.config_id];
        const mounted = state?.status === "mounted";
        const pending = working === config.config_id || state?.status === "mounting" || state?.status === "unmounting";
        return <li key={config.config_id}><code class="wrap">{config.mode === "winfsp_drive" ? config.drive_letter : config.local_path}</code>
          <Badge tone={mounted ? "ok" : state?.status === "error" ? "error" : "neutral"}>{mounted ? tr("已挂载（只读）") : state?.status === "error" ? tr("挂载失败") : pending ? tr("处理中") : tr("已配置，未连接")}</Badge>
          {state?.error ? <ErrorPanel error={state.error} compact/> : null}
          <div class="row-gap"><Btn label={mounted ? tr("停止挂载") : tr("连接挂载")} tone={mounted ? "tonal" : "primary"} busy={pending} disabled={!!working || (!mounted && (!canConfigure || !m.available))} reason={!canConfigure ? tr("需要先登录") : tr("前置检查尚未通过")} onClick={() => void act(config, mounted)}/>
            <Btn label={tr("编辑配置")} disabled={mounted || pending} reason={tr("请先停止挂载再修改配置")} onClick={() => openMountConfig(m.mode, config, onChanged)}/></div>
        </li>;
      })}</ul> : null}
      <div class="mount-actions">
        <Btn tone="primary" icon={m.mode === "winfsp_drive" ? "drive" : "folder"} label={m.mode === "winfsp_drive" ? tr("只读挂载为 P:") : tr("创建同步文件夹")}
          disabled={!m.available || !canConfigure} reason={!signedIn() ? tr("需要先登录") : tx("{0} 项前置检查未通过：{1}", failed.length, failed.map((f) => f.label).join("、"))} onClick={() => openMountConfig(m.mode, undefined, onChanged)} />
      </div>
    </section>
  );
}

export function MountPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const [caps, setCaps] = useState<MountCapabilities | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [busy, setBusy] = useState(false);
  const enabled = useStore(app, s => s.runtime === "tauri" && !!s.app?.feature_flags.mount);
  const signed = useStore(app, signedIn);
  const [configs, setConfigs] = useState<MountConfig[]>([]);
  const [states, setStates] = useState<Record<string, MountState>>({});
  const [revision, setRevision] = useState(0);
  const [configError, setConfigError] = useState<ClientError>();
  const load = () => {
    setBusy(true);
    setError(null);
    api.mountCapabilities().then(
      (c) => { setCaps(c); setBusy(false); },
      (e) => { setError(errorOf(e)); setBusy(false); },
    );
  };
  useEffect(load, [epoch, refresh]);
  useEffect(() => {
    if (!enabled || !signed) { setConfigs([]); setStates({}); return; }
    let alive = true, pending = false;
    const read = async () => {
      if (pending) return; pending = true;
      try {
        const result = await api.mountConfigsList();
        const next = await Promise.all(result.map(async config => [config.config_id, await api.mountStatus(config.config_id)] as const));
        if (alive) { setConfigs(result); setStates(Object.fromEntries(next)); setConfigError(undefined); }
      } catch (e) { if (alive) setConfigError(errorOf(e)); }
      finally { pending = false; }
    };
    void read(); const timer = setInterval(() => void read(), 3000);
    return () => { alive = false; clearInterval(timer); };
  }, [enabled, signed, epoch, refresh, revision]);

  return (
    <PageFrame label={tr("挂载")} caption={<PageTitle>{tr("挂载")}</PageTitle>}
      commands={<>
        <div class="cmd-group"><span class="cmd-meta">{tr("可选功能，检查全部通过后才能启用。")}</span></div>
        <div class="cmd-group cmd-end">
          {caps ? <span class="cmd-meta">{tr("检测于")}{formatRelative(caps.checked_at)}{caps.platform === "windows" ? "，Windows" : ""}</span> : null}
          <Btn tone="tonal" icon="refresh" label={tr("重新检测")} busy={busy} onClick={load} />
        </div>
      </>}>
      <div class="canvas-scroll">
        {configError ? <ErrorPanel error={configError} onRetry={() => setRevision(n => n + 1)}/> : null}
        {error ? <ErrorPanel error={error} onRetry={load} />
          : !caps ? <Spinner label={tr("正在检测挂载能力")} />
            : <div class="mount-grid">{caps.modes.map((m) => <ModeSection key={m.mode} m={m} enabled={enabled} configured={configs.filter(config => config.mode === m.mode)} states={states} onChanged={() => setRevision(n => n + 1)} />)}</div>}
      </div>
    </PageFrame>
  );
}

function openMountConfig(mode: MountConfig["mode"], existing: MountConfig | undefined, done: () => void): void {
  void openDialog({ title: tr("挂载配置"), width: 640, render: close => <MountConfigForm mode={mode} existing={existing} onSaved={() => { done(); close(true); }}/> });
}

function MountConfigForm({ mode, existing, onSaved }: { mode: MountConfig["mode"]; existing?: MountConfig; onSaved: () => void }) {
  const [remote, setRemote] = useState({ id: existing?.remote_id ?? "root", path: existing?.remote_id ?? "/" });
  const [localPath, setLocalPath] = useState(existing?.local_path ?? "");
  const [drive, setDrive] = useState(existing?.drive_letter ?? "P:");
  const [busy, setBusy] = useState(false), [error, setError] = useState<ClientError>();
  const pickRemote = async () => { const chosen = await pickFolder(tr("选择云盘目录"), tr("使用此文件夹")); if (chosen) setRemote({ id: chosen.id, path: chosen.path }); };
  const pickLocal = async () => {
    try { const parent = await api.downloadDirectoryPick(); if (parent) setLocalPath(`${parent.replace(/[\\/]$/, "")}\\NekoPak-${Date.now().toString(36)}`); }
    catch (e) { setError(errorOf(e)); }
  };
  const invalid = mode === "winfsp_drive" ? !/^[A-Z]:$/i.test(drive) : !/^[A-Z]:[\\/].+/i.test(localPath);
  const save = async () => {
    setBusy(true); setError(undefined);
    try {
      await api.mountConfigSave({ config_id: existing?.config_id ?? "", mode, remote_id: remote.id, ...(mode === "winfsp_drive" ? { drive_letter: drive.toUpperCase() } : { local_path: localPath.trim() }) });
      toast(tr("挂载配置已保存，请点击连接挂载"), "success"); onSaved();
    } catch (e) { setError(errorOf(e)); } finally { setBusy(false); }
  };
  return <form class="form-grid" onSubmit={e => { e.preventDefault(); if (!busy && !invalid) void save(); }}>
    <Field label={tr("云盘目录")}><div class="directory-picker"><code class="wrap">{remote.path}</code><Btn label={tr("选择云盘目录")} icon="folderOpen" onClick={() => void pickRemote()}/></div></Field>
    {mode === "winfsp_drive" ? <Field label={tr("盘符")} htmlFor="mount-drive" hint={tr("只使用空闲盘符，挂载后保持只读。") }><select id="mount-drive" class="select" value={drive} onChange={e => setDrive((e.target as HTMLSelectElement).value)}>{Array.from({ length: 23 }, (_, i) => `${String.fromCharCode(68 + i)}:`).map(value => <option value={value}>{value}</option>)}</select></Field> : <Field label={tr("同步根目录")} htmlFor="mount-local" hint={tr("使用一个尚不存在的新目录。NekoPak 将创建专用同步根；停止后保留占位文件，之后可连接相同配置。") }>
      <div class="directory-picker"><input id="mount-local" class="input mono" value={localPath} onInput={e => setLocalPath((e.target as HTMLInputElement).value)}/><Btn label={tr("选择父文件夹")} icon="folderOpen" onClick={() => void pickLocal()}/></div>
    </Field>}
    {error ? <ErrorPanel error={error}/> : null}
    <p class="muted">{tr("前置检查通过后，启动仍会验证真实随机读取。重启应用不会自动挂载。")}</p>
    <div class="dialog-actions"><Btn label={tr("保存配置")} type="submit" tone="primary" busy={busy} disabled={invalid} reason={tr("请选择有效的本地目录或盘符")}/></div>
  </form>;
}
