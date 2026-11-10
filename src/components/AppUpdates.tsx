import { useEffect, useState } from "preact/hooks";
import type { UpdateStatus } from "../lib/contract-proposals";
import { api, errorOf } from "../lib/api";
import { tr, tx } from "../lib/i18n";
import { formatBytes, formatDateTime, percent } from "../lib/format";
import { useStore } from "../lib/store";
import { app, isMobileHost, updateSettings } from "../state/app";
import { Btn, ErrorPanel, Progress, Switch } from "./ui";
import { confirmDialog } from "./dialogs";
import { toast } from "../state/ui";
import { navigate } from "../state/route";

export async function checkUpdatesAtStartup(): Promise<void> {
  if(app.get().runtime!=="tauri" || !app.get().app?.feature_flags.app_updates || !app.get().settings?.updates?.auto_check)return;
  try {
    const status=await api.updateCheck();
    if(status.phase==="available"&&status.update)toast(tx("发现新版本 {0}",status.update.version),"info",{action:{label:tr("查看更新"),run:()=>navigate({page:"settings",section:"about"})}});
  } catch { /* Optional startup check must not block login; About shows retryable status. */ }
}

export function AppUpdates(){
  const enabled=useStore(app,s=>s.runtime==="tauri"&&s.app?.feature_flags.app_updates===true);
  const automatic=useStore(app,s=>s.settings?.updates?.auto_check??false);
  const mobile=useStore(app,isMobileHost);
  const [status,setStatus]=useState<UpdateStatus>();
  const [busy,setBusy]=useState(false);
  const [failure,setFailure]=useState<ReturnType<typeof errorOf>>();
  const [pollFailure,setPollFailure]=useState<ReturnType<typeof errorOf>>();
  const [permission,setPermission]=useState(false);
  useEffect(()=>{
    if(!enabled)return;
    let active=true,loading=false;
    const refresh=async()=>{if(loading)return;loading=true;try{const value=await api.updateStatus();if(active){setStatus(value);setPollFailure(undefined);}}catch(error){if(active)setPollFailure(errorOf(error));}finally{loading=false;}};
    void refresh();const timer=setInterval(()=>void refresh(),500);
    return()=>{active=false;clearInterval(timer);};
  },[enabled]);
  const check=async()=>{setBusy(true);setFailure(undefined);setPermission(false);try{setStatus(await api.updateCheck());}catch(error){setFailure(errorOf(error));}finally{setBusy(false);}};
  const download=async()=>{if(!status?.update)return;setBusy(true);setFailure(undefined);setPermission(false);try{await api.updateDownload(status.update.update_id);setStatus(await api.updateStatus());}catch(error){setFailure(errorOf(error));}finally{setBusy(false);}};
  const install=async()=>{
    if(!status?.update)return;
    if(!await confirmDialog({title:tr("安装更新"),body:<><p>{tr(mobile?"将打开系统安装界面，现有账号和文件会保留。":"将暂停本地传输并退出应用，然后打开更新安装器。")}</p>{!mobile?<p class="dim">{tr("Windows 更新使用安装器；便携版的原目录会保留。")}</p>:null}</>,confirmLabel:tr("安装更新")}))return;
    setBusy(true);setFailure(undefined);
    try{const result=await api.updateInstall(status.update.update_id);setPermission(result.state==="permission_required");if(result.state==="installer_opened"&&mobile)toast(tr("已打开系统安装界面"),"info");}
    catch(error){setFailure(errorOf(error));}finally{setBusy(false);}
  };
  const downloading=status?.phase==="downloading";
  const error=failure??pollFailure??status?.error;
  return <section class="app-updates" aria-label={tr("应用更新")}>
    <h3>{tr("应用更新")}</h3>
    {!enabled?<p class="dim">{tr("请在支持的桌面或手机应用中检查更新。")}</p>:<>
      <div class="spread"><span>{tr("启动时检查更新")}</span><Switch label={tr("启动时检查更新")} checked={automatic} onChange={value=>void updateSettings({updates:{auto_check:value}}).catch(error=>setFailure(errorOf(error)))}/></div>
      <p class="dim">{tr("预览通道 · 下载和安装需手动确认")}</p>
      <Btn label={tr("检查更新")} icon="refresh" busy={busy||status?.phase==="checking"} disabled={downloading||status?.supported===false} onClick={()=>void check()}/>
      {status?.checked_at?<p class="dim">{tr("上次检查")}: {formatDateTime(status.checked_at)}</p>:null}
      {status?.phase==="up_to_date"?<p role="status">{tr("当前通道没有更高版本。")}</p>:null}
      {error?<ErrorPanel error={error} compact/>:null}
      {status?.update?<div class="update-available">
        <strong>{tx("新版本 {0}",status.update.version)} · {formatBytes(status.update.size_bytes)}</strong>
        <pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere",fontFamily:"inherit",fontSize:"inherit"}}>{status.update.notes}</pre>
        {downloading?<><Progress value={percent(status.downloaded_bytes,status.update.size_bytes)} label={tr("更新包下载进度")}/><p class="tabular">{formatBytes(status.downloaded_bytes)} / {formatBytes(status.update.size_bytes)}</p><Btn label={tr("取消下载")} onClick={()=>void api.updateCancel().catch(error=>setFailure(errorOf(error)))}/></>
          :status.phase==="ready"?<><p>{tr("更新包已验证，可以安装。")}</p>{permission?<p class="inline-note">{tr("请允许本应用安装更新，返回后再次点击安装。")}</p>:null}<Btn label={tr("安装更新")} icon="download" tone="primary" busy={busy} onClick={()=>void install()}/></>
          :<Btn label={tr("下载更新")} icon="download" tone="primary" busy={busy} onClick={()=>void download()}/>}
      </div>:null}
    </>}
  </section>;
}
