import { tr, tx } from "../lib/i18n";
/**
 * “批量重命名…”: rules on the left, live preview on the right. Each item
 * is renamed with files_mutate("rename") and its own idempotency key; the
 * whole batch is one undo entry.
 */
import { useMemo, useRef, useState } from "preact/hooks";
import type { RemoteEntry } from "../lib/contract";
import { api, errorOf, newIdempotencyKey } from "../lib/api";
import { DEFAULT_RULES, planRename, type RenameRules } from "../lib/rename";
import { fileGate } from "../state/app";
import { openDialog, toast } from "../state/ui";
import { pushUndo, undo } from "../state/undo";
import { Icon } from "../components/Icon";
import { Btn, Segmented, Switch } from "../components/ui";
import { requestRefresh } from "./common";
import { sessionGeneration } from "../state/session";

interface Ctx {
  siblings: () => string[];
  reload: () => void;
}

const MAX_ROWS = 500;

function RenameBody({ entries, ctx, close }: { entries: RemoteEntry[]; ctx: Ctx; close: (v?: unknown) => void }) {
  const [rules, setRules] = useState<RenameRules>(DEFAULT_RULES);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(0);
  const [failures, setFailures] = useState<{ name: string; message: string }[]>([]);
  const keys = useRef(new Map<string, string>());
  const items = useMemo(() => entries.map((e) => ({ id: e.id, name: e.name, kind: e.kind })), [entries]);
  const plan = useMemo(() => planRename(items, ctx.siblings(), rules), [items, rules]);
  const set = <K extends keyof RenameRules>(k: K, v: RenameRules[K]) => setRules((r) => ({ ...r, [k]: v }));
  const text = (k: "find" | "replace" | "prefix" | "suffix" | "separator", label: string, placeholder = "") => (
    <label class="br-field">
      <span>{label}</span>
      <input class="input" value={rules[k]} placeholder={placeholder} spellcheck={false} onInput={(e) => set(k, (e.target as HTMLInputElement).value)} />
    </label>
  );

  const apply = async () => {
    const generation=sessionGeneration();
    const rows = plan.rows.filter((r) => r.status === "ok");
    if (!rows.length) return;
    setBusy(true);
    setFailures([]);
    const ok: { id: string; from: string }[] = [];
    const fails: { name: string; message: string }[] = [];
    for (const r of rows) {
      if (generation!==sessionGeneration()) return;
      let key = keys.current.get(`${r.id}→${r.to}`);
      if (!key) keys.current.set(`${r.id}→${r.to}`, (key = newIdempotencyKey()));
      try {
        const res = await api.filesMutate("rename", [r.id], { name: r.to }, key);
        if (generation!==sessionGeneration()) return;
        if (res.failed.length) fails.push({ name: r.from, message: res.failed[0].error.message });
        else ok.push({ id: r.id, from: r.from });
      } catch (e) {
        if (generation!==sessionGeneration()) return;
        fails.push({ name: r.from, message: errorOf(e).message });
        if (errorOf(e).code === "AUTH_EXPIRED") break;
      }
      setDone(ok.length + fails.length);
    }
    setBusy(false);
    if (generation!==sessionGeneration()) return;
    ctx.reload();
    if (ok.length) {
      const entry = pushUndo(tx("批量重命名 {0} 项", ok.length), async () => {
        let bad = 0;
        for (const o of ok) {
          if (generation!==sessionGeneration()) return false;
          try {
            const res = await api.filesMutate("rename", [o.id], { name: o.from }, newIdempotencyKey());
            bad += res.failed.length;
          } catch {
            bad++;
          }
        }
        requestRefresh();
        if (generation!==sessionGeneration()) return false;
        toast(bad ? tx("撤销时 {0} 项失败", bad) : tx("已撤销批量重命名 {0} 项", ok.length), bad ? "error" : "success");
        return bad === 0;
      });
      toast(tx("已重命名 {0} 项{1}", ok.length, fails.length ? `，${fails.length} 项失败` : ""), fails.length ? "warning" : "success", { action: { label: tr("撤销"), run: () => void undo(entry.id) } });
    }
    if (fails.length) setFailures(fails);
    else close(true);
  };

  return (
    <form class="br" onSubmit={(e) => { e.preventDefault(); if (!busy && plan.changed && !plan.errors && !plan.ruleError) void apply(); }}>
      <div class="br-rules">
        <fieldset class="br-group">
          <legend><Icon name="findReplace" size={18} />{tr("查找与替换")}</legend>
          {text("find", tr("查找"), tr("要替换的文字"))}
          {text("replace", tr("替换为"), tr("留空表示删除"))}
          <div class="br-switches">
            <label class="br-switch"><Switch label={tr("正则表达式")} checked={rules.regex} onChange={(v) => set("regex", v)} /><span>{tr("正则表达式")}</span></label>
            <label class="br-switch"><Switch label={tr("区分大小写")} checked={rules.caseSensitive} onChange={(v) => set("caseSensitive", v)} /><span>{tr("区分大小写")}</span></label>
          </div>
          {plan.ruleError ? <p class="field-error" role="alert">{plan.ruleError}</p> : null}
        </fieldset>
        <fieldset class="br-group">
          <legend><Icon name="textFields" size={18} />{tr("添加文字")}</legend>
          <div class="br-two">
            {text("prefix", tr("前缀"))}
            {text("suffix", tr("后缀"), tr("加在扩展名之前"))}
          </div>
        </fieldset>
        <fieldset class="br-group">
          <legend><Icon name="numbered" size={18} />{tr("编号")}</legend>
          <Segmented<RenameRules["numbering"]> label={tr("编号位置")} value={rules.numbering} onChange={(v) => set("numbering", v)}
            options={[{ value: "off", label: tr("不编号") }, { value: "prefix", label: tr("加在前面") }, { value: "suffix", label: tr("加在后面") }]} />
          {rules.numbering !== "off" ? (
            <div class="br-three">
              <label class="br-field"><span>{tr("起始")}</span><input class="input" type="number" min={0} value={rules.start} onInput={(e) => set("start", Math.max(0, Number((e.target as HTMLInputElement).value) || 0))} /></label>
              <label class="br-field"><span>{tr("位数")}</span><input class="input" type="number" min={1} max={6} value={rules.digits} onInput={(e) => set("digits", Math.min(6, Math.max(1, Number((e.target as HTMLInputElement).value) || 1)))} /></label>
              {text("separator", tr("分隔"))}
            </div>
          ) : null}
        </fieldset>
        <fieldset class="br-group">
          <legend><Icon name="label" size={18} />{tr("大小写与其他")}</legend>
          <Segmented<RenameRules["case"]> label={tr("大小写")} value={rules.case} onChange={(v) => set("case", v)}
            options={[{ value: "keep", label: tr("不变") }, { value: "lower", label: tr("小写") }, { value: "upper", label: tr("大写") }, { value: "title", label: tr("首字母") }]} />
          <div class="br-switches">
            <label class="br-switch"><Switch label={tr("保留扩展名")} checked={rules.keepExtension} onChange={(v) => set("keepExtension", v)} /><span>{tr("只改扩展名之前的部分")}</span></label>
            <label class="br-switch"><Switch label={tr("去除首尾空格")} checked={rules.trim} onChange={(v) => set("trim", v)} /><span>{tr("去除首尾空格")}</span></label>
          </div>
        </fieldset>
      </div>

      <div class="br-preview">
        <div class="br-head">
          <strong>{tr("预览")}</strong>
          <span class="dim">{plan.changed} {tr("项将改名")}{plan.errors ? tx("，{0} 项有问题", plan.errors) : ""}，{plan.rows.length - plan.changed - plan.errors} {tr("项不变")}</span>
        </div>
        <ol class="br-list" aria-label={tr("重命名预览")}>
          {plan.rows.slice(0, MAX_ROWS).map((r) => (
            <li class={`br-row st-${r.status}`}>
              <span class="br-from ellipsis" dir="auto" title={r.from}>{r.from}</span>
              <Icon name="arrowForward" size={16} class="br-arrow" />
              <span class="br-to" dir="auto" title={r.to}>
                <span class="ellipsis">{r.to}</span>
                {r.error ? <span class="br-err"><Icon name="alertCircle" size={14} filled />{r.error}</span> : r.warnings[0] ? <span class="br-warn"><Icon name="alert" size={14} />{r.warnings[0]}</span> : null}
              </span>
            </li>
          ))}
          {plan.rows.length > MAX_ROWS ? <li class="dim br-more">{tr("还有")}{plan.rows.length - MAX_ROWS} {tr("项未显示，规则同样会作用于它们。")}</li> : null}
        </ol>
        {failures.length ? (
          <div class="inline-note warn" role="alert">
            <Icon name="alert" size={16} />
            <span>{failures.length} {tr("项失败：")}{failures[0].name}：{failures[0].message}{tr("。修正后可再次执行，已成功的项目不会重复处理。")}</span>
          </div>
        ) : null}
      </div>

      <div class="dialog-actions spread br-actions">
        <span class="muted">{busy ? tx("正在重命名 {0}/{1}…", done, plan.changed) : ""}</span>
        <div class="row-gap">
          <Btn tone="ghost" label={failures.length ? tr("关闭") : tr("取消")} onClick={() => close()} />
          <Btn type="submit" tone="primary" icon="pencil" label={plan.changed ? tx("重命名 {0} 项", plan.changed) : tr("重命名")} busy={busy}
            disabled={!plan.changed || plan.errors > 0 || !!plan.ruleError}
            reason={plan.ruleError ? plan.ruleError : plan.errors ? tr("请先解决标红的冲突") : tr("规则没有改变任何名称")} />
        </div>
      </div>
    </form>
  );
}

export function openBatchRename(entries: RemoteEntry[], ctx: Ctx): void {
  const g = fileGate("rename", entries.map(e => e.id));
  if (!g.ok) return void toast(g.reason ?? tr("不能重命名"), "info");
  if (entries.length < 2) return void toast(tr("至少选择两个项目才能批量重命名"), "info");
  void openDialog({ title: tx("批量重命名 {0} 项", entries.length), width: 880, render: (close) => <RenameBody entries={entries} ctx={ctx} close={close} /> });
}
