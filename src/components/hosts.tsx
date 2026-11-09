import { tr } from "../lib/i18n";
import { Fragment } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useStore } from "../lib/store";
import { closeDialog, closeMenu, dialogs, dismissToast, menu, toasts, type MenuItem } from "../state/ui";
import { Icon } from "./Icon";
import { ErrorPanel } from "./ui";

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function DialogHost() {
  const stack = useStore(dialogs, (s) => s.stack);
  const top = stack[stack.length - 1];
  const ref = useRef<HTMLDivElement>(null);
  const restore = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!top) return;
    restore.current ??= document.activeElement as HTMLElement | null;
    const el = ref.current;
    const first = el?.querySelector<HTMLElement>("[data-autofocus]") ?? el?.querySelector<HTMLElement>(FOCUSABLE);
    requestAnimationFrame(() => first?.focus());
    return () => {
      if (dialogs.get().stack.length === 0) {
        const r = restore.current;
        restore.current = null;
        requestAnimationFrame(() => r?.focus?.());
      }
    };
  }, [top?.id]);

  if (!top) return null;
  const dismissable = top.dismissable !== false;
  const titleId = `dlg-title-${top.id}`;

  return (
    <div
      class="modal-backdrop"
      onMouseDown={(e) => {
        if (dismissable && e.target === e.currentTarget) closeDialog(top.id);
      }}
    >
      {stack.map(entry => <div
        key={entry.id}
        ref={entry.id === top.id ? ref : undefined}
        class="modal"
        role={entry.id === top.id ? "dialog" : undefined}
        aria-modal={entry.id === top.id ? "true" : undefined}
        aria-hidden={entry.id === top.id ? undefined : "true"}
        inert={entry.id === top.id ? undefined : true}
        aria-labelledby={entry.id === top.id ? titleId : `dlg-title-${entry.id}`}
        style={{ width: `min(${entry.width ?? 440}px, calc(100vw - 32px))`, display: entry.id === top.id ? undefined : "none" }}
        onKeyDown={(e) => {
          if (e.key === "Escape" && dismissable) {
            e.stopPropagation();
            closeDialog(top.id);
            return;
          }
          if (e.key === "Tab") {
            const items = [...(ref.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter((x) => x.offsetParent !== null);
            if (items.length === 0) return;
            const first = items[0];
            const last = items[items.length - 1];
            if (e.shiftKey && document.activeElement === first) {
              last.focus();
              e.preventDefault();
            } else if (!e.shiftKey && document.activeElement === last) {
              first.focus();
              e.preventDefault();
            }
          }
          e.stopPropagation();
        }}
      >
        <header class="modal-head">
          <h2 id={`dlg-title-${entry.id}`}>{entry.title}</h2>
          {dismissable ? (
            <button type="button" class="btn btn-quiet btn-icon btn-sm" aria-label={tr("关闭")} title={tr("关闭 (Esc)")} onClick={() => closeDialog(top.id)}>
              <Icon name="x" />
            </button>
          ) : null}
        </header>
        <div class="modal-body">{entry.render((v) => closeDialog(entry.id, v))}</div>
      </div>)}
    </div>
  );
}

export function MenuHost() {
  const m = useStore(menu, (s) => s);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [active, setActive] = useState(0);

  // Quick actions (icon row) are listed first; the rest form the grouped list.
  const items = [...m.items.filter((i) => i.quick), ...m.items.filter((i) => !i.quick)];
  const quickCount = items.filter((i) => i.quick).length;

  useLayoutEffect(() => {
    if (!m.open || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    // Opens to the bottom-right of the pointer; flips per axis when it would overflow.
    const x = Math.max(8, Math.min(m.x, window.innerWidth - r.width - 8));
    const y = m.y + r.height > window.innerHeight - 8 ? Math.max(8, m.y - r.height) : m.y;
    setPos({ x, y });
    // Initial focus on the first list row, not the icon row (avoids an instant tooltip).
    const firstList = items.findIndex((i, idx) => idx >= quickCount && !i.disabled);
    setActive(firstList < 0 ? Math.max(0, items.findIndex((i) => !i.disabled)) : firstList);
    requestAnimationFrame(() => ref.current?.focus());
  }, [m.open, m.items, m.x, m.y]);

  useEffect(() => {
    if (!m.open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) closeMenu();
    };
    const onBlur = () => closeMenu();
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [m.open]);

  if (!m.open) return null;

  const run = (i: number) => {
    const item = items[i];
    if (!item || item.disabled) return;
    closeMenu();
    item.run?.();
  };

  const row = (item: MenuItem, i: number) => (
    <Fragment key={item.id}>
      {item.separatorBefore && i > quickCount ? <div class="menu-sep" role="separator" /> : null}
      <div
        id={`menu-item-${i}`}
        role="menuitem"
        class={`menu-item${i === active ? " active" : ""}${item.danger ? " danger" : ""}`}
        aria-disabled={item.disabled ? "true" : undefined}
        title={item.disabled ? item.reason : undefined}
        onMouseEnter={() => !item.disabled && setActive(i)}
        onClick={() => run(i)}
      >
        {item.icon ? <Icon name={item.icon} /> : <span class="icon" />}
        <span class="menu-label">
          {item.label}
          {item.disabled && item.reason ? <span class="menu-reason">{item.reason}</span> : null}
        </span>
        {item.shortcut ? <span class="menu-kbd">{item.shortcut}</span> : null}
      </div>
    </Fragment>
  );

  return (
    <div
      ref={ref}
      class="menu"
      role="menu"
      aria-label={m.label ?? tr("操作")}
      tabIndex={-1}
      aria-activedescendant={`menu-item-${active}`}
      style={{ left: `${pos.x}px`, top: `${pos.y}px` }}
      onKeyDown={(e) => {
        const n = items.length;
        const step = (dir: 1 | -1) => {
          let i = active;
          for (let k = 0; k < n; k++) {
            i = (i + dir + n) % n;
            if (!items[i].disabled) break;
          }
          setActive(i);
        };
        if (e.key === "ArrowDown" || (e.key === "ArrowRight" && active < quickCount)) step(1);
        else if (e.key === "ArrowUp" || (e.key === "ArrowLeft" && active < quickCount)) step(-1);
        else if (e.key === "Home") setActive(items.findIndex((i) => !i.disabled));
        else if (e.key === "End") {
          for (let i = n - 1; i >= 0; i--) if (!items[i].disabled) { setActive(i); break; }
        } else if (e.key === "Enter" || e.key === " ") run(active);
        else if (e.key === "Escape" || e.key === "Tab") closeMenu();
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {quickCount ? (
        <div class="menu-quick" role="group" aria-label={tr("常用操作")}>
          {items.slice(0, quickCount).map((item, i) => (
            <button
              type="button"
              id={`menu-item-${i}`}
              role="menuitem"
              tabIndex={-1}
              class={`menu-qbtn${i === active ? " active" : ""}`}
              aria-label={item.label}
              aria-disabled={item.disabled ? "true" : undefined}
              title={item.disabled ? `${item.label}：${item.reason ?? tr("不可用")}` : `${item.label}${item.shortcut ? `（${item.shortcut}）` : ""}`}
              onMouseEnter={() => !item.disabled && setActive(i)}
              onClick={() => run(i)}
            >
              <Icon name={item.icon ?? "more"} />
            </button>
          ))}
        </div>
      ) : null}
      <div class="menu-list">{items.slice(quickCount).map((item, k) => row(item, k + quickCount))}</div>
    </div>
  );
}

export function ToastHost() {
  const items = useStore(toasts, (s) => s.items);
  return (
    <div class="toasts" aria-live="polite" aria-relevant="additions">
      {items.map((t) => (
        <div class={`toast toast-${t.tone}`} role={t.tone === "error" ? "alert" : "status"}>
          {t.error ? (
            <ErrorPanel error={t.error} compact />
          ) : (
            <>
              <Icon name={t.tone === "success" ? "checkCircle" : t.tone === "error" ? "alertCircle" : t.tone === "warning" ? "alert" : "info"} />
              <span class="toast-text">{t.text}</span>
            </>
          )}
          {t.action ? (
            <button type="button" class="btn btn-ghost btn-sm" onClick={() => { t.action!.run(); dismissToast(t.id); }}>
              {t.action.label}
            </button>
          ) : null}
          <button type="button" class="btn btn-quiet btn-icon btn-sm" aria-label={tr("关闭通知")} onClick={() => dismissToast(t.id)}>
            <Icon name="x" size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
