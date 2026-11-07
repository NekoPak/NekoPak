import { useEffect, useState } from "preact/hooks";
import type { BackendId, CapabilityReport } from "../lib/contract";
import { mockControls } from "../lib/api";
import { useStore } from "../lib/store";
import { app, backendLabel, beginAuth, cancelAuth, logout, refreshCaps, resync } from "../state/app";
import { toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Badge, Btn, ErrorPanel, Spinner } from "../components/ui";

export const STATUS_LABEL: Record<CapabilityReport["status"], string> = {
  ready: "可用",
  needs_login: "需要授权",
  not_installed: "未配置",
  unavailable: "不可用",
};

export const STATUS_TONE: Record<CapabilityReport["status"], "ok" | "accent" | "neutral" | "warn"> = {
  ready: "ok",
  needs_login: "accent",
  not_installed: "neutral",
  unavailable: "neutral",
};

export const BACKEND_BLURB: Record<BackendId, string> = {
  mock: "离线演示后端，使用固定样本数据，用于在没有真实账号时开发和测试界面。",
  official_cli: "通过 PikPak 官方命令行工具连接账号，授权由官方页面完成。当前接入支持只读预览。",
  rust_compat: "社区兼容层，使用非官方接口。审计完成前不会启用。",
  webdav: "PikPak WebDAV（Premium 功能），需要在官方应用中生成 WebDAV 凭据。",
  rclone: "使用你自行安装的 Rclone 作为外部程序。",
};

function selectable(c: CapabilityReport): boolean {
  return c.status === "ready" || c.status === "needs_login";
}

export function BackendChoice({ caps, value, onChange }: { caps: CapabilityReport[]; value: BackendId | null; onChange: (b: BackendId) => void }) {
  return (
    <div class="backend-choice" role="radiogroup" aria-label="连接方式">
      {caps.map((c) => {
        const ok = selectable(c) && c.backend !== "webdav";
        const reason = c.backend === "webdav" ? "WebDAV 凭据录入需要安全存储命令（提案 UI-001），M0 暂不开放" : !selectable(c) ? c.limitations[0] : undefined;
        return (
          <label class={`backend-opt${value === c.backend ? " on" : ""}${ok ? "" : " off"}`} title={ok ? undefined : reason}>
            <input type="radio" name="backend" value={c.backend} checked={value === c.backend} disabled={!ok} onChange={() => onChange(c.backend)} />
            <span class="bo-main">
              <span class="bo-title">
                <strong>{backendLabel(c.backend)}</strong>
                <Badge tone={STATUS_TONE[c.status]}>{STATUS_LABEL[c.status]}</Badge>
                {c.version ? <span class="muted">{c.version}</span> : null}
              </span>
              <span class="bo-blurb">{BACKEND_BLURB[c.backend]}</span>
              {c.limitations.length ? (
                <ul class="bo-limits">{c.limitations.map((l) => <li>{l}</li>)}</ul>
              ) : null}
            </span>
          </label>
        );
      })}
    </div>
  );
}

/** Sign-in panel. The UI never asks for a PikPak password. */
export function AuthPanel({ compact }: { compact?: boolean }) {
  const s = useStore(app, (x) => x);
  const firstReady = s.caps.find((c) => selectable(c) && c.backend !== "webdav")?.backend ?? null;
  const [choice, setChoice] = useState<BackendId | null>(firstReady);
  const [mock, setMock] = useState(false);

  useEffect(() => {
    if (!choice && firstReady) setChoice(firstReady);
  }, [firstReady]);
  useEffect(() => {
    void mockControls().then((m) => setMock(!!m));
  }, []);

  if (s.runtime === "tauri") {
    const demo = s.app?.mode === "mock";
    return <div class="auth-panel">
      <div class="inline-note"><Icon name={demo ? "flask" : "lock"} />{demo ? "尚未连接 PikPak · 当前为离线演示" : "官方 CLI · 只读预览"}</div>
      <p>{demo ? "当前界面使用测试文件和模拟任务，没有登录真实账号，也不会下载文件。" : s.auth?.state === "signed_in" ? "外部 CLI 已授权。当前仅开放文件浏览与元数据读取。" : "请在已配置的官方 CLI 中完成授权，然后刷新状态。NekoPak 不收集密码、令牌或验证码。"}</p>
      {!demo ? <p class="muted">凭据由外部 CLI 管理，本应用不声称该工具的文件已采用系统加密存储。后端选择由启动配置决定。</p> : null}
      <Btn icon="refresh" label="刷新连接状态" onClick={() => void resync()} />
      <Btn label="在浏览器中授权" disabled reason="应用内授权流程尚未实现，请使用外部官方 CLI" />
      <Btn label="退出登录" disabled reason="当前版本不操作外部 CLI 的授权文件" />
      {s.authError ? <ErrorPanel error={s.authError} compact /> : null}
    </div>;
  }

  if (s.auth?.state === "signed_in") {
    return (
      <div class="auth-done">
        <div class="kv-row"><span class="muted">账号</span><strong>{s.auth.account?.display_name ?? "已登录"}</strong></div>
        <div class="kv-row"><span class="muted">账号类型</span><span>{s.auth.account?.tier === "premium" ? "Premium" : s.auth.account?.tier === "free" ? "Free" : "未知"}</span></div>
        <div class="kv-row"><span class="muted">后端</span><span>{backendLabel(s.auth.backend)}</span></div>
        {!compact ? (
          <div class="row-gap pad-top">
            <Btn icon="logout" label="退出登录" onClick={() => void logout().then(() => toast("已退出登录，本地授权引用已清除", "success"))} />
          </div>
        ) : null}
      </div>
    );
  }

  const flow = s.authFlow;
  return (
    <div class="auth-panel">
      {s.auth?.state === "expired" ? (
        <div class="inline-note warn"><Icon name="alert" size={14} />登录已过期。重新授权后，文件列表与任务历史保持不变。</div>
      ) : null}
      {s.capsError ? <ErrorPanel error={s.capsError} onRetry={() => void refreshCaps()} compact /> : null}
      {s.capsLoading && !s.caps.length ? <Spinner label="正在检测可用后端" /> : null}
      {!flow ? (
        <>
          <BackendChoice caps={s.caps} value={choice} onChange={setChoice} />
          {!firstReady && s.caps.length ? (
            <p class="muted">当前没有可用于登录的后端。安装官方 CLI 后点击“重新检测”。</p>
          ) : null}
          <div class="row-gap pad-top">
            <Btn tone="primary" icon="external" label="在浏览器中授权" busy={s.authBusy} disabled={!choice} reason="请选择一个可用的连接方式" onClick={() => choice && void beginAuth(choice)} />
            <Btn tone="ghost" icon="refresh" label="重新检测" busy={s.capsLoading} onClick={() => void refreshCaps()} />
          </div>
          <p class="muted small-print"><Icon name="lock" size={14} />NekoPak 不会要求你输入 PikPak 密码。授权在官方页面或官方 CLI 中完成，凭据只保存在系统安全存储中。</p>
        </>
      ) : (
        <div class="auth-wait" role="status">
          <Spinner label="等待授权" />
          <div>
            <p><strong>等待你在浏览器中完成授权</strong></p>
            <p class="muted">{flow.instructions}</p>
          </div>
          <div class="row-gap">
            <Btn label="取消授权" onClick={() => void cancelAuth()} />
          </div>
          {mock ? (
            <div class="mock-auth">
              <span class="muted"><Icon name="flask" size={14} />Mock：</span>
              <Btn size="sm" tone="primary" label="模拟授权成功" onClick={() => void mockControls().then((m) => m?.completeAuth("success"))} />
              <Btn size="sm" label="模拟用户取消" onClick={() => void mockControls().then((m) => m?.completeAuth("cancel"))} />
              <Btn size="sm" label="模拟网络失败" onClick={() => void mockControls().then((m) => m?.completeAuth("network"))} />
            </div>
          ) : null}
        </div>
      )}
      {s.authError && !flow ? (
        s.authError.code === "CANCELLED"
          ? <div class="inline-note"><Icon name="info" size={14} />授权已取消。可以重新开始。</div>
          : <ErrorPanel error={s.authError} compact onRetry={() => choice && void beginAuth(choice)} />
      ) : null}
    </div>
  );
}
