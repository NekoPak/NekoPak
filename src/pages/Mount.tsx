import { useEffect, useState } from "preact/hooks";
import type { ClientError } from "../lib/contract";
import type { MountCapabilities, MountModeReport } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { formatRelative } from "../lib/format";
import { useStore } from "../lib/store";
import { app } from "../state/app";
import { Icon } from "../components/Icon";
import { Badge, Btn, ErrorPanel, Spinner } from "../components/ui";
import { useRefreshKey } from "./common";
import { PageFrame, PageTitle } from "../components/Frame";

const MODE_COPY: Record<MountModeReport["mode"], { title: string; what: string; not: string; icon: string }> = {
  cloud_files_sync_root: {
    title: "同步文件夹（按需文件）",
    icon: "folder",
    what: "在资源管理器中注册一个同步根目录，云端文件显示为占位文件，打开时才下载内容。基于 Windows Cloud Files API。",
    not: "这不是独立盘符：它是一个普通文件夹，位于你选择的位置。",
  },
  winfsp_drive: {
    title: "虚拟盘符 P:",
    icon: "drive",
    what: "通过 WinFsp 把云盘挂载成独立盘符，先只读。需要后端真实支持随机读取，否则播放器一打开就会下载整个文件。",
    not: "需要你自行安装 WinFsp 驱动；NekoPak 不会自动安装驱动或申请管理员权限。",
  },
};

function ModeSection({ m }: { m: MountModeReport }) {
  const copy = MODE_COPY[m.mode];
  const failed = m.checks.filter((c) => c.status !== "pass");
  return (
    <section class="mount-mode" aria-labelledby={`mm-${m.mode}`}>
      <header class="mount-head">
        <Icon name={copy.icon} size={20} />
        <div>
          <h2 id={`mm-${m.mode}`}>{copy.title}</h2>
          <p>{copy.what}</p>
          <p class="muted">{copy.not}</p>
        </div>
        <Badge tone={m.available ? "ok" : "neutral"} icon={m.available ? "checkCircle" : "lock"}>{m.available ? "可用" : "不可用"}</Badge>
      </header>
      <ul class="checks" aria-label={`${copy.title} 前置检查`}>
        {m.checks.map((c) => (
          <li class={`check-${c.status}`}>
            <Icon name={c.status === "pass" ? "checkCircle" : c.status === "fail" ? "alertCircle" : "help"} size={16} />
            <div>
              <span class="check-label">{c.label}</span>
              <span class="check-status">{c.status === "pass" ? "通过" : c.status === "fail" ? "未通过" : "未知"}</span>
              <span class="muted check-detail">{c.detail}</span>
              {c.requirement ? <span class="check-req"><Icon name="info" size={14} />{c.requirement}</span> : null}
            </div>
          </li>
        ))}
      </ul>
      <div class="mount-actions">
        <Btn tone="primary" icon={m.mode === "winfsp_drive" ? "drive" : "folder"} label={m.mode === "winfsp_drive" ? "只读挂载为 P:" : "创建同步文件夹"}
          disabled={!m.available} reason={`${failed.length} 项前置检查未通过：${failed.map((f) => f.label).join("、")}`} />
      </div>
    </section>
  );
}

export function MountPage() {
  const desktop = useStore(app, s => s.runtime === "tauri");
  if (desktop) return <PageFrame label="挂载" caption={<PageTitle>挂载</PageTitle>}><div class="canvas-pad"><p>Cloud Files 同步根与 WinFsp 独立盘符均未接入。当前未检测驱动，后端随机读取也尚未通过验收。</p><Btn label="创建同步文件夹" disabled reason="Cloud Files 尚未实现" /><Btn label="只读挂载为 P:" disabled reason="WinFsp 与随机读取尚未实现" /></div></PageFrame>;
  return <BrowserMountPage />;
}

function BrowserMountPage() {
  const epoch = useStore(app, (s) => s.epoch);
  const refresh = useRefreshKey();
  const [caps, setCaps] = useState<MountCapabilities | null>(null);
  const [error, setError] = useState<ClientError | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => {
    setBusy(true);
    setError(null);
    api.mountCapabilities().then(
      (c) => { setCaps(c); setBusy(false); },
      (e) => { setError(errorOf(e)); setBusy(false); },
    );
  };
  useEffect(load, [epoch, refresh]);

  return (
    <PageFrame label="挂载" caption={<PageTitle>挂载</PageTitle>}
      commands={<>
        <div class="cmd-group"><span class="cmd-meta">可选扩展，不影响浏览、下载和同步。检查全部通过才能启用，不会出现“已挂载”的假状态。</span></div>
        <div class="cmd-group cmd-end">
          {caps ? <span class="cmd-meta">检测于 {formatRelative(caps.checked_at)}{caps.platform === "windows" ? "，Windows" : ""}</span> : null}
          <Btn tone="tonal" icon="refresh" label="重新检测" busy={busy} onClick={load} />
        </div>
      </>}>
      <div class="canvas-scroll">
        {error ? <ErrorPanel error={error} onRetry={load} />
          : !caps ? <Spinner label="正在检测挂载能力" />
            : <div class="mount-grid">{caps.modes.map((m) => <ModeSection m={m} />)}</div>}
      </div>
    </PageFrame>
  );
}
