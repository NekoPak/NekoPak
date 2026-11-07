import type { ComponentChildren, JSX } from "preact";
import { useState } from "preact/hooks";
import type { ClientError } from "../lib/contract";
import { diagnosticsText, ERROR_COPY } from "../lib/errors";
import { copyText, openExternal } from "../lib/api";
import { BoxCat } from "./Brand";
import { Icon } from "./Icon";
import { toast } from "../state/ui";
import { navigate } from "../state/route";

let uid = 0;
export const useId = (prefix = "id") => useState(() => `${prefix}-${++uid}`)[0];

export type Tone = "primary" | "tonal" | "default" | "outlined" | "ghost" | "quiet" | "danger";

export interface BtnProps {
  label?: string;
  icon?: string;
  /** Trailing icon, e.g. a dropdown caret. */
  trail?: string;
  tone?: Tone;
  size?: "sm" | "md";
  disabled?: boolean;
  /** Why it is disabled. Tooltip + screen-reader description. */
  reason?: string;
  title?: string;
  kbd?: string;
  busy?: boolean;
  type?: "button" | "submit";
  iconOnly?: boolean;
  pressed?: boolean;
  class?: string;
  onClick?: (e: MouseEvent) => void;
  children?: ComponentChildren;
  [key: `data-${string}`]: string | undefined;
}

/**
 * M3 buttons. Disabled buttons stay focusable (aria-disabled) so keyboard
 * and screen-reader users can still discover why an action is unavailable.
 */
export function Btn(p: BtnProps): JSX.Element {
  const descId = useId("why");
  const off = p.disabled || p.busy;
  const tone = p.tone === "default" || !p.tone ? (p.iconOnly ? "quiet" : "tonal") : p.tone;
  const tip = p.disabled && p.reason ? p.reason : p.title ?? (p.iconOnly ? p.label : undefined);
  const dataAttrs = Object.fromEntries(Object.entries(p).filter(([k]) => k.startsWith("data-")));
  return (
    <>
      <button
        type={p.type ?? "button"}
        class={`btn btn-${tone} btn-${p.size ?? "md"}${p.iconOnly ? " btn-icon" : ""}${p.class ? ` ${p.class}` : ""}`}
        aria-disabled={off ? "true" : undefined}
        aria-busy={p.busy ? "true" : undefined}
        aria-label={p.iconOnly ? p.label : undefined}
        aria-pressed={p.pressed === undefined ? undefined : p.pressed ? "true" : "false"}
        aria-describedby={p.disabled && p.reason ? descId : undefined}
        aria-keyshortcuts={p.kbd}
        title={tip}
        onClick={(e) => {
          if (off) {
            e.preventDefault();
            if (p.disabled && p.reason) toast(p.reason, "info");
            return;
          }
          p.onClick?.(e as unknown as MouseEvent);
        }}
        {...dataAttrs}
      >
        {p.busy ? <span class="spinner" aria-hidden="true" /> : p.icon ? <Icon name={p.icon} size={p.size === "sm" ? 18 : 20} filled={!!p.pressed} /> : null}
        {!p.iconOnly && (p.label || p.children) ? <span class="btn-label">{p.children ?? p.label}</span> : null}
        {p.trail ? <Icon name={p.trail} size={18} class="btn-trail" /> : null}
      </button>
      {p.disabled && p.reason ? <span id={descId} class="sr-only">{`不可用：${p.reason}`}</span> : null}
    </>
  );
}

export function Spinner({ label = "正在加载" }: { label?: string }) {
  return (
    <span class="spinner-wrap" role="status">
      <span class="spinner" aria-hidden="true" />
      <span class="sr-only">{label}</span>
    </span>
  );
}

export function Empty({ title, children, action, awake }: { title: string; children?: ComponentChildren; action?: ComponentChildren; awake?: boolean }) {
  return (
    <div class="empty">
      <BoxCat awake={awake} />
      <h2 class="empty-title">{title}</h2>
      {children ? <p class="empty-body">{children}</p> : null}
      {action ? <div class="empty-actions">{action}</div> : null}
    </div>
  );
}

/**
 * M3 linear progress: fill, a small gap, the remaining track and a stop dot.
 * Unknown totals render the track only — never a fake percentage.
 */
export function Progress({ value, tone, label }: { value: number | null; tone?: "ok" | "warn" | "error" | "muted"; label: string }) {
  const pct = value === null ? null : Math.max(0, Math.min(100, value));
  return (
    <div
      class={`lp${pct === null ? " lp-unknown" : ""}${tone ? ` lp-${tone}` : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
      aria-valuetext={pct === null ? "进度未提供" : `${pct.toFixed(1)}%`}
    >
      {pct === null ? null : <span class="lp-fill" style={{ width: `${pct}%` }} />}
      {pct === 100 ? null : <span class="lp-track" />}
    </div>
  );
}

export function Badge({ tone = "neutral", children, icon }: { tone?: "neutral" | "accent" | "ok" | "warn" | "error"; children: ComponentChildren; icon?: string }) {
  return (
    <span class={`badge badge-${tone}`}>
      {icon ? <Icon name={icon} size={14} filled /> : null}
      {children}
    </span>
  );
}

export async function copyWithToast(text: string, what: string): Promise<void> {
  const ok = await copyText(text);
  toast(ok ? `已复制${what}` : `无法访问剪贴板，未能复制${what}`, ok ? "success" : "error");
}

export async function openOfficial(url = "https://mypikpak.com/"): Promise<void> {
  const r = await openExternal(url);
  if (r === "copied") toast("无法直接打开浏览器，已复制链接", "info");
  if (r === "failed") toast("无法打开链接", "error");
}

const CODE_ICON: Partial<Record<ClientError["code"], string>> = {
  QUOTA_EXCEEDED: "quota",
  PERMISSION_DENIED: "lock",
  ACCOUNT_RESTRICTED: "lock",
  AUTH_EXPIRED: "user",
  TRANSIENT_NETWORK: "cloud",
  UNSUPPORTED: "info",
  NOT_FOUND: "search",
};

/** Uniform ClientError presentation with a copyable request_id; never shows tokens or signed URLs. */
export function ErrorPanel({ error, onRetry, context, compact, onReauth }: { error: ClientError; onRetry?: () => void; context?: string; compact?: boolean; onReauth?: () => void }) {
  const copy = ERROR_COPY[error.code];
  const actions: JSX.Element[] = [];
  if (onRetry && (error.retryable || error.code === "NOT_FOUND" || error.code === "INTERNAL")) {
    actions.push(<Btn tone={error.code === "AUTH_EXPIRED" ? "tonal" : "primary"} icon="retry" label="重试" onClick={onRetry} />);
  }
  if (error.code === "AUTH_EXPIRED") {
    actions.push(<Btn tone="primary" icon="user" label="重新授权" onClick={onReauth ?? (() => navigate({ page: "settings", section: "account" }))} />);
  }
  if (error.code === "QUOTA_EXCEEDED") {
    actions.push(<Btn tone="tonal" icon="quota" label="查看配额" onClick={() => navigate({ page: "settings", section: "quota" })} />);
    actions.push(<Btn tone="ghost" icon="external" label="打开 PikPak 官网" onClick={() => void openOfficial()} />);
  }
  if (error.code === "UNSUPPORTED") {
    actions.push(<Btn tone="ghost" icon="backend" label="查看后端能力" onClick={() => navigate({ page: "settings", section: "backends" })} />);
  }
  return (
    <div class={`error-panel${compact ? " compact" : ""} code-${error.code.toLowerCase()}`} role="alert" data-error-code={error.code}>
      <span class="error-icon"><Icon name={CODE_ICON[error.code] ?? "alertCircle"} size={compact ? 20 : 24} /></span>
      <div class="error-text">
        <p class="error-title">{copy.title}<span class="error-code">{error.code}</span></p>
        <p class="error-msg">{error.message}</p>
        {!compact ? <p class="error-hint">{copy.hint}</p> : null}
        <div class="error-actions">
          {actions}
          <Btn
            tone="quiet"
            size="sm"
            icon="copy"
            label={error.request_id ? `复制诊断信息（${error.request_id}）` : "复制诊断信息"}
            title="复制错误码与 request_id，不包含令牌或下载链接"
            onClick={() => void copyWithToast(diagnosticsText(error, context), "诊断信息")}
          />
        </div>
      </div>
    </div>
  );
}

export function Field({ label, hint, children, error, htmlFor }: { label: string; hint?: ComponentChildren; children: ComponentChildren; error?: string; htmlFor?: string }) {
  return (
    <div class={`field${error ? " has-error" : ""}`}>
      <label class="field-label" for={htmlFor}>{label}</label>
      {children}
      {/* The message line always reserves its space; only its text changes. */}
      <p class={error ? "field-error" : "field-hint"} role={error ? "alert" : undefined}>{error ?? hint ?? " "}</p>
    </div>
  );
}

export function Switch({ checked, onChange, label, disabled, reason, id }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean; reason?: string; id?: string }) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      class="switch"
      aria-checked={checked ? "true" : "false"}
      aria-disabled={disabled ? "true" : undefined}
      aria-label={label}
      title={disabled ? reason : undefined}
      onClick={() => {
        if (disabled) {
          if (reason) toast(reason, "info");
          return;
        }
        onChange(!checked);
      }}
    >
      <span class="switch-thumb">{checked ? <Icon name="check" size={14} /> : null}</span>
    </button>
  );
}

/** M3 segmented button (single choice, radiogroup semantics). */
export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string; icon?: string; disabled?: boolean; reason?: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div class="segmented" role="radiogroup" aria-label={label}
      onKeyDown={(e) => {
        if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
        const enabled = options.filter((o) => !o.disabled);
        const i = enabled.findIndex((o) => o.value === value);
        const n = enabled[(i + (e.key === "ArrowRight" ? 1 : enabled.length - 1)) % enabled.length];
        if (n) {
          onChange(n.value);
          e.preventDefault();
          const host = e.currentTarget as HTMLElement | null;
          requestAnimationFrame(() => host?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
        }
      }}
    >
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            type="button"
            role="radio"
            aria-checked={on ? "true" : "false"}
            aria-disabled={o.disabled ? "true" : undefined}
            tabIndex={on ? 0 : -1}
            title={o.disabled ? o.reason : undefined}
            onClick={() => {
              if (o.disabled) {
                if (o.reason) toast(o.reason, "info");
                return;
              }
              onChange(o.value);
            }}
          >
            {on ? <Icon name="check" size={18} /> : o.icon ? <Icon name={o.icon} size={18} /> : null}
            <span>{o.label}</span>
          </button>
        );
      })}
    </div>
  );
}

export function Kbd({ children }: { children: ComponentChildren }) {
  return <kbd class="kbd">{children}</kbd>;
}

/** Small uppercase-free section label used inside islands and sidebars. */
export function SectionLabel({ children, count }: { children: ComponentChildren; count?: number }) {
  return (
    <p class="section-label">
      {children}
      {count !== undefined ? <span class="section-count">{count}</span> : null}
    </p>
  );
}
