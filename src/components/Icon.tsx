/**
 * Material Symbols Rounded glyphs (Apache-2.0, see icons.generated.ts).
 * Outlined by default; `filled` for selected / active states, following
 * the M3 convention.
 */
import type { JSX } from "preact";
import { ICONS } from "./icons.generated";

export type IconName = keyof typeof ICONS | string;

export function Icon({ name, size = 20, filled = false, class: cls, label }: { name: IconName; size?: number; filled?: boolean; class?: string; label?: string }): JSX.Element {
  const paths = ICONS[name] ?? ICONS.file;
  const d = filled && paths.length > 1 ? paths[1] : paths[0];
  return (
    <svg
      class={`icon${cls ? ` ${cls}` : ""}`}
      width={size}
      height={size}
      viewBox="0 -960 960 960"
      fill="currentColor"
      aria-hidden={label ? undefined : "true"}
      role={label ? "img" : undefined}
      aria-label={label}
      focusable="false"
    >
      <path d={d} />
    </svg>
  );
}
