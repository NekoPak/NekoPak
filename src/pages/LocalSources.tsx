import { useEffect, useState } from "preact/hooks";
import { api, errorOf } from "../lib/api";
import type { ClientError } from "../lib/contract";
import type { LocalSource } from "../lib/contract-proposals";
import { tr } from "../lib/i18n";
import { useStore } from "../lib/store";
import { app } from "../state/app";
import { navigate } from "../state/route";
import { toast } from "../state/ui";
import { PageFrame, PageTitle } from "../components/Frame";
import { Btn, Empty, ErrorPanel, Spinner } from "../components/ui";
import { Icon } from "../components/Icon";
import { confirmDialog } from "../components/dialogs";

export function LocalSourcesPage() {
  const enabled = useStore(app, s => s.app?.feature_flags.local_sources === true);
  const [sources, setSources] = useState<LocalSource[] | null>(null);
  const [error, setError] = useState<ClientError>();
  const [busy, setBusy] = useState(false);
  const load = () => { if (enabled) void api.localSourcesList().then(setSources, e => setError(errorOf(e))); };
  useEffect(load, [enabled]);
  const add = async () => {
    setBusy(true); setError(undefined);
    try { const source = await api.localSourcesAdd(); if (source) { load(); toast(tr("已添加本地文件夹映射"), "success"); } }
    catch (e) { setError(errorOf(e)); } finally { setBusy(false); }
  };
  const remove = async (source: LocalSource) => {
    if (!await confirmDialog({ title: tr("取消本地映射？"), body: <p>{tr("这只会移除 NekoPak 中的入口，文件夹和其中的文件保持不变。")}</p>, confirmLabel: tr("取消映射") })) return;
    try { await api.localSourcesRemove(source.source_id); load(); } catch (e) { setError(errorOf(e)); }
  };
  return <PageFrame label={tr("本地文件夹")} caption={<PageTitle>{tr("本地文件夹")}</PageTitle>}
    commands={<Btn label={tr("添加本地文件夹")} icon="folderPlus" tone="primary" busy={busy} disabled={!enabled} reason={tr("当前宿主未提供本地文件夹映射")} onClick={() => void add()} />}>
    <div class="canvas-pad"><p class="muted">{tr("只读浏览你选择的本地文件夹。不会上传、同步或修改原文件。")}</p>
      {error ? <ErrorPanel error={error} onRetry={load}/> : !enabled ? <Empty title={tr("当前宿主未提供本地文件夹映射")}/> : sources === null ? <Spinner/> : sources.length === 0 ? <Empty title={tr("还没有本地文件夹")}/> :
        <ul class="local-sources">{sources.map(source => <li key={source.source_id}><Icon name="folder"/><div class="local-source-info"><strong dir="auto">{source.name}</strong><span class="mono dim" dir="auto">{source.path}</span></div>
          <Btn label={tr("打开")} icon="folderOpen" onClick={() => navigate({ page: "files", folder: source.root_id ?? source.source_id })}/>
          <Btn label={tr("取消映射")} tone="quiet" onClick={() => void remove(source)}/></li>)}</ul>}
    </div>
  </PageFrame>;
}
