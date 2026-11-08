import { tr, tx } from "./i18n";
/**
 * Batch rename: pure rule application and conflict detection.
 * The result is a preview; nothing here talks to the backend.
 */
import { splitName, validateName } from "./names";

export interface RenameRules {
  find: string;
  replace: string;
  regex: boolean;
  caseSensitive: boolean;
  prefix: string;
  suffix: string;
  numbering: "off" | "prefix" | "suffix";
  start: number;
  digits: number;
  separator: string;
  case: "keep" | "lower" | "upper" | "title";
  /** Apply find/replace and case only to the part before the extension. */
  keepExtension: boolean;
  trim: boolean;
}

export const DEFAULT_RULES: RenameRules = {
  find: "",
  replace: "",
  regex: false,
  caseSensitive: false,
  prefix: "",
  suffix: "",
  numbering: "off",
  start: 1,
  digits: 2,
  separator: " ",
  case: "keep",
  keepExtension: true,
  trim: true,
};

export interface RenameInput {
  id: string;
  name: string;
  kind: "file" | "folder";
}

export interface RenamePreview {
  id: string;
  from: string;
  to: string;
  status: "unchanged" | "ok" | "error";
  error?: string;
  warnings: string[];
}

export interface RenamePlan {
  rows: RenamePreview[];
  /** Invalid regular expression, if any. */
  ruleError?: string;
  changed: number;
  errors: number;
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function applyCase(s: string, mode: RenameRules["case"]): string {
  switch (mode) {
    case "lower": return s.toLowerCase();
    case "upper": return s.toUpperCase();
    case "title": return s.replace(/(^|[\s_\-.([])(\p{L})/gu, (_, sep: string, ch: string) => sep + ch.toUpperCase());
    default: return s;
  }
}

export function buildMatcher(rules: RenameRules): { re: RegExp | null; error?: string } {
  if (!rules.find) return { re: null };
  try {
    const flags = `g${rules.caseSensitive ? "" : "i"}u`;
    return { re: new RegExp(rules.regex ? rules.find : escapeRegExp(rules.find), flags) };
  } catch (e) {
    return { re: null, error: tx("正则表达式无效：{0}", (e as Error).message) };
  }
}

/** New name for one item at position `index` of the batch. */
export function renameOne(item: RenameInput, index: number, rules: RenameRules, re: RegExp | null): string {
  const split = item.kind === "file" && rules.keepExtension ? splitName(item.name) : { base: item.name, ext: "" };
  let base = split.base;
  if (re) base = base.replace(re, rules.replace);
  base = applyCase(base, rules.case);
  if (rules.trim) base = base.trim();
  const num = String(rules.start + index).padStart(Math.max(1, Math.min(6, rules.digits)), "0");
  if (rules.numbering === "prefix") base = `${num}${rules.separator}${base}`;
  else if (rules.numbering === "suffix") base = `${base}${rules.separator}${num}`;
  base = `${rules.prefix}${base}${rules.suffix}`;
  return `${base}${split.ext}`;
}

const nfcLower = (s: string) => s.normalize("NFC").toLowerCase();

/**
 * Compute the preview. `siblings` are the names of every item in the
 * folder (including the ones being renamed).
 */
export function planRename(items: RenameInput[], siblings: string[], rules: RenameRules): RenamePlan {
  const { re, error } = buildMatcher(rules);
  if (error) {
    return { rows: items.map((i) => ({ id: i.id, from: i.name, to: i.name, status: "unchanged", warnings: [] })), ruleError: error, changed: 0, errors: 0 };
  }
  const batchIds = new Set(items.map((i) => i.id));
  const fromNames = new Map(items.map((i) => [i.id, i.name]));
  // Names in the folder that stay put (not part of this batch).
  const fixed = new Set(siblings.filter((n) => !items.some((i) => i.name === n)).map(nfcLower));
  const proposed = items.map((item, idx) => ({ item, to: renameOne(item, idx, rules, re) }));

  const targetCount = new Map<string, number>();
  for (const p of proposed) targetCount.set(nfcLower(p.to), (targetCount.get(nfcLower(p.to)) ?? 0) + 1);
  const currentOfChanging = new Map<string, string>();
  for (const p of proposed) if (p.to !== p.item.name) currentOfChanging.set(nfcLower(p.item.name), p.item.id);

  const rows: RenamePreview[] = proposed.map(({ item, to }) => {
    if (to === item.name) return { id: item.id, from: item.name, to, status: "unchanged", warnings: [] };
    const v = validateName(to, [], undefined);
    if (v.error) return { id: item.id, from: item.name, to, status: "error", error: v.error, warnings: v.warnings };
    const key = nfcLower(to);
    if ((targetCount.get(key) ?? 0) > 1) return { id: item.id, from: item.name, to, status: "error", error: tr("与本批中另一项的新名称相同"), warnings: v.warnings };
    if (fixed.has(key)) return { id: item.id, from: item.name, to, status: "error", error: tr("文件夹中已有同名项目"), warnings: v.warnings };
    const holder = currentOfChanging.get(key);
    if (holder && holder !== item.id && batchIds.has(holder)) {
      return { id: item.id, from: item.name, to, status: "error", error: tx("与本批另一项的当前名称“{0}”相同，请分两次完成", fromNames.get(holder)), warnings: v.warnings };
    }
    // an unchanged batch member that already has this name
    const sameAsUnchanged = proposed.some((p) => p.item.id !== item.id && p.to === p.item.name && nfcLower(p.item.name) === key);
    if (sameAsUnchanged) return { id: item.id, from: item.name, to, status: "error", error: tr("文件夹中已有同名项目"), warnings: v.warnings };
    return { id: item.id, from: item.name, to, status: "ok", warnings: v.warnings };
  });
  return {
    rows,
    changed: rows.filter((r) => r.status === "ok").length,
    errors: rows.filter((r) => r.status === "error").length,
  };
}
