import { useState } from "preact/hooks";
import { mockControls, type MockControlsApi } from "../lib/api";
import { SCENARIOS, type FaultFlags, type ScenarioId } from "../lib/mock/fixtures";
import { resync, resetDisclaimerForDemo } from "../state/app";
import { loadSnapshot } from "../state/transfers";
import { openDialog, toast } from "../state/ui";
import { Btn, Switch } from "../components/ui";

const FAULTS: { key: keyof Omit<FaultFlags, "latency_ms">; label: string; hint: string }[] = [
  { key: "drop_at_37", label: "下载到 37% 时断网", hint: "任务进入“重试等待”，随后继续" },
  { key: "out_of_order", label: "进度事件乱序", hint: "定期重发旧 revision 的事件，界面应丢弃" },
  { key: "integrity_fail", label: "完整性校验失败", hint: "下载到 100% 后校验失败，保留 .part" },
  { key: "auth_expired", label: "令牌过期", hint: "下一次远端请求返回 AUTH_EXPIRED" },
];

function Panel({ ctl, close }: { ctl: MockControlsApi; close: () => void }) {
  const [scenario, setScenario] = useState<ScenarioId>(ctl.scenario());
  const [faults, setFaults] = useState<FaultFlags>(ctl.faults());

  const apply = async (s: ScenarioId) => {
    ctl.setScenario(s);
    setScenario(s);
    await resync();
    toast(`已切换到 Mock 场景“${SCENARIOS.find((x) => x.id === s)?.label}”`, "success");
  };

  const toggle = (k: keyof FaultFlags, v: boolean | number) => {
    const next = { ...faults, [k]: v } as FaultFlags;
    setFaults(next);
    ctl.setFaults({ [k]: v });
  };

  return (
    <div class="devpanel">
      <p class="muted">这些控制只存在于 Mock 模式，用来复现 03_CONTRACTS §6 的七组样本和故障。接入 Tauri 后不会显示。</p>
      <fieldset class="scenario-list">
        <legend>场景</legend>
        {SCENARIOS.map((s) => (
          <label class={`scenario${scenario === s.id ? " on" : ""}`}>
            <input type="radio" name="scenario" value={s.id} checked={scenario === s.id} onChange={() => void apply(s.id)} />
            <span>
              <strong>{s.label}</strong>
              <span class="muted">{s.description}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <fieldset class="fault-list">
        <legend>故障注入</legend>
        {FAULTS.map((f) => (
          <div class="fault-row">
            <span>
              <strong>{f.label}</strong>
              <span class="muted">{f.hint}</span>
            </span>
            <Switch checked={faults[f.key]} label={f.label} onChange={(v) => toggle(f.key, v)} />
          </div>
        ))}
        <div class="fault-row">
          <span>
            <strong>响应延迟</strong>
            <span class="muted">每个命令额外等待的毫秒数</span>
          </span>
          <select
            aria-label="响应延迟"
            value={String(faults.latency_ms)}
            onChange={(e) => toggle("latency_ms", Number((e.target as HTMLSelectElement).value))}
          >
            {[0, 120, 800, 2500].map((ms) => <option value={ms}>{ms} ms</option>)}
          </select>
        </div>
      </fieldset>
      <div class="dialog-actions spread">
        <div class="row-gap">
          <Btn
            label="模拟应用重启"
            icon="retry"
            title="运行中的任务被持久化为排队状态，界面从快照恢复"
            onClick={async () => {
              ctl.simulateRestart();
              await loadSnapshot();
              toast("已模拟重启：界面已从任务快照恢复", "success");
            }}
          />
          <Btn label="重新显示首次启动" tone="ghost" onClick={() => { resetDisclaimerForDemo(); close(); }} />
        </div>
        <Btn tone="primary" label="完成" onClick={close} />
      </div>
    </div>
  );
}

export async function openMockPanel(): Promise<void> {
  const ctl = await mockControls();
  if (!ctl) {
    toast("当前连接的是真实 IPC，Mock 控制不可用", "info");
    return;
  }
  await openDialog({ title: "Mock 场景与故障注入", width: 560, render: (close) => <Panel ctl={ctl} close={() => close()} /> });
}
