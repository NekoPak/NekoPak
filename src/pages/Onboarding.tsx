import { useEffect, useState } from "preact/hooks";
import { useStore } from "../lib/store";
import { formatBytes } from "../lib/format";
import { acceptDisclaimer, app, backendLabel, signedIn } from "../state/app";
import { navigate } from "../state/route";
import { LogoMark } from "../components/Brand";
import { Icon } from "../components/Icon";
import { Btn } from "../components/ui";
import { WindowControls } from "../components/Frame";
import { AuthPanel } from "./Auth";

type Step = "notice" | "connect" | "done";

const DIM: Record<string, string> = {
  downstream_daily: "每日下行", downstream_monthly: "每月下行", upload_monthly: "每月上传", cloud_download_monthly: "每月云下载",
};

/**
 * First launch: brand on the frame colour, the form island floating over it.
 * Notice → backend detection & external auth → account tier & quota → files.
 */
export function Onboarding({ onClose }: { onClose: () => void }) {
  const s = useStore(app, (x) => x);
  const [step, setStep] = useState<Step>(s.disclaimerAccepted ? (signedIn(s) ? "done" : "connect") : "notice");

  useEffect(() => {
    if (step === "connect" && signedIn(s)) setStep("done");
  }, [s.auth?.state, step]);

  const steps: { id: Step; label: string }[] = [
    { id: "notice", label: "声明" },
    { id: "connect", label: "连接账号" },
    { id: "done", label: "完成" },
  ];
  const idx = steps.findIndex((x) => x.id === step);

  return (
    <div class="onboarding" role="dialog" aria-modal="true" aria-labelledby="ob-title">
      <div class="ob-caption" data-tauri-drag-region><span class="pf-drag" data-tauri-drag-region /><WindowControls /></div>
      <div class="ob-brand">
        <LogoMark size={64} />
        <p class="ob-name">NekoPak</p>
        <p class="ob-tagline">轻量的 PikPak 桌面客户端，Rust 核心，社区独立开发。</p>
        <ul class="ob-promises">
          <li><Icon name="lock" filled /><span>不要求输入 PikPak 密码，授权在官方页面或官方 CLI 中完成。</span></li>
          <li><Icon name="shield" filled /><span>不修改系统 Hosts、DNS 或证书，不绕过配额与风控。</span></li>
          <li><Icon name="sync" filled /><span>自动同步先生成计划给你确认，默认不删除任何文件。</span></li>
          <li><Icon name="info" filled /><span>不收集遥测数据。</span></li>
        </ul>
      </div>
      <div class="ob-island">
        <ol class="ob-steps" aria-label="步骤">
          {steps.map((x, i) => (
            <li class={i < idx ? "done" : i === idx ? "on" : ""} aria-current={i === idx ? "step" : undefined}>
              <span class="ob-n">{i < idx ? <Icon name="check" size={16} /> : i + 1}</span>{x.label}
            </li>
          ))}
        </ol>

        {step === "notice" ? (
          <div class="ob-body">
            <h1 id="ob-title">欢迎使用 NekoPak</h1>
            <div class="about-statement" role="note">
              <p><strong>NekoPak 不是 PikPak 官方产品</strong>，与 PikPak 服务提供者没有隶属关系。使用时请遵守 PikPak 的服务条款。</p>
            </div>
            <p class="ob-lede">{s.runtime === "tauri" ? "当前为开发预览。离线演示与真实账号连接会分别标明；尚未接入的功能不会执行。" : "接下来预览连接账号的演示流程。所有账号数据均为测试样本。"}</p>
            <div class="ob-actions">
              <Btn tone="primary" label="我已了解，继续" data-autofocus="" onClick={() => { acceptDisclaimer(); setStep(signedIn(s) ? "done" : "connect"); }} />
            </div>
          </div>
        ) : step === "connect" ? (
          <div class="ob-body">
            <h1 id="ob-title">连接你的账号</h1>
            <p class="ob-lede">选择连接方式。只有检测为可用的方式才能选择。</p>
            <AuthPanel compact />
            <div class="ob-actions spread">
              <Btn tone="ghost" label="稍后再说" onClick={onClose} />
            </div>
          </div>
        ) : (
          <div class="ob-body">
            <h1 id="ob-title">{s.auth?.state === "demo" ? "离线演示已就绪" : "已连接"}</h1>
            <p class="ob-lede">
              {s.auth?.state === "demo" ? "当前可预览界面和模拟任务，尚未连接 PikPak。" : `${s.auth?.account?.display_name ?? "账号"}，${s.auth?.account?.tier === "premium" ? "Premium" : s.auth?.account?.tier === "free" ? "Free" : "类型未知"}，通过${backendLabel(s.auth?.backend)}。`}
            </p>
            {s.quota && s.auth?.state !== "demo" ? (
              <ul class="ob-quota">
                {s.quota.items.map((q) => (
                  <li>
                    <span>{DIM[q.dimension]}</span>
                    <span class="dim">
                      {q.exactness === "unavailable" ? "不可获取"
                        : q.remaining ? `剩余 ${q.unit === "bytes" ? formatBytes(q.remaining) : `${q.remaining} 次`}${q.exactness === "estimated" ? "（估计）" : ""}`
                          : q.used ? `已用 ${q.unit === "bytes" ? formatBytes(q.used) : q.used}` : "未提供"}
                    </span>
                  </li>
                ))}
              </ul>
            ) : <p class="dim">真实账号配额将在完成连接后查询。</p>}
            <p class="dim">Free 与 Premium 能使用的功能取决于后端实际提供的能力，可在“设置 › 后端”查看。</p>
            <div class="ob-actions">
              <Btn tone="primary" icon="cloud" label="进入云盘" data-autofocus="" onClick={() => { onClose(); navigate({ page: "files", folder: "root" }); }} />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
