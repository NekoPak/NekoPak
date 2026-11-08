import { Store } from "./store";
import { EN } from "./locales/en";
import { EN_EXTRA } from "./locales/en-extra";
import { ZH_TW } from "./locales/zh-tw";
import { JA } from "./locales/ja";
import { KO } from "./locales/ko";

export type Locale = "zh-CN" | "zh-TW" | "en" | "ja-JP" | "ko-KR";
export type Language = "system" | Locale;
export const LANGUAGES: { value: Language; label: string }[] = [
  { value: "system", label: "跟随系统" }, { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" }, { value: "en", label: "English" },
  { value: "ja-JP", label: "日本語" }, { value: "ko-KR", label: "한국어" },
];
const KEY = "nekopak.language";

function initial(): Language {
  try {
    const value = localStorage.getItem(KEY);
    if (LANGUAGES.some(item => item.value === value)) return value as Language;
  } catch { /* Optional first-paint cache; native preferences live in Rust. */ }
  return "system";
}

export const language = new Store<{ preference: Language }>({ preference: initial() });

export function resolveSystemLanguage(value: string): Locale {
  if (/^zh(-Hant|-TW|-HK|-MO)/i.test(value)) return "zh-TW";
  if (/^zh/i.test(value)) return "zh-CN";
  if (/^ja/i.test(value)) return "ja-JP";
  if (/^ko/i.test(value)) return "ko-KR";
  return "en";
}
export function resolvedLanguage(): Locale {
  const pref = language.get().preference;
  if (pref !== "system") return pref;
  return typeof document === "undefined" ? "zh-CN" : resolveSystemLanguage(globalThis.navigator?.language ?? "en");
}

export function setLanguage(preference: Language): void {
  language.set({ preference });
  try { localStorage.setItem(KEY, preference); } catch { /* Optional browser cache. */ }
  if (typeof document !== "undefined") document.documentElement.lang = resolvedLanguage();
}

/** Only developer-authored UI copy is passed here. Remote names, paths and identifiers are untouched. */
export function tr(text: string): string {
  const locale = resolvedLanguage();
  if (locale === "zh-CN") return text;
  if (locale === "zh-TW") return ZH_TW[text] ?? text;
  if (locale === "ja-JP") return JA[text] ?? text;
  if (locale === "ko-KR") return KO[text] ?? text;
  return EN_EXTRA[text] ?? EN[text] ?? text;
}
/** Interpolated values are inserted after translating the developer-authored template. */
export function tx(template: string, ...values: (string | number | undefined)[]): string {
  return tr(template).replace(/\{(\d+)\}/g, (_, index: string) => String(values[Number(index)] ?? ""));
}
