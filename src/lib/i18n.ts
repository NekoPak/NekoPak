import { Store } from "./store";

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

/** `revision` changes when a catalog finishes loading, so views re-render with it. */
export const language = new Store<{ preference: Language; revision: number }>({ preference: initial(), revision: 0 });
let nativeSystemLanguage: string | undefined;

type Catalog = Readonly<Record<string, string>>;
/**
 * Catalogs are loaded on demand: Simplified Chinese (the source language)
 * needs none, and every other language downloads only its own. Earlier
 * entries win, matching the previous EN_EXTRA-over-EN lookup.
 */
const catalogs: Partial<Record<Locale, readonly Catalog[]>> = {};
const loading: Partial<Record<Locale, Promise<void>>> = {};

export function registerLocale(locale: Locale, parts: readonly Catalog[]): void {
  catalogs[locale] = parts;
}

/** Resolves once `locale` can be translated synchronously. Never rejects. */
export function ensureLocale(locale: Locale = resolvedLanguage()): Promise<void> {
  if (locale === "zh-CN" || catalogs[locale]) return Promise.resolve();
  return loading[locale] ??= (async () => {
    try {
      if (locale === "zh-TW") registerLocale(locale, [(await import("./locales/zh-tw")).ZH_TW]);
      else if (locale === "ja-JP") registerLocale(locale, [(await import("./locales/ja")).JA]);
      else if (locale === "ko-KR") registerLocale(locale, [(await import("./locales/ko")).KO]);
      else registerLocale(locale, [(await import("./locales/en-extra")).EN_EXTRA, (await import("./locales/en")).EN]);
    } catch {
      // Offline or a failed chunk: keep the source language and allow a retry.
      delete loading[locale];
    }
  })();
}

/** Loads the active catalog if needed, then re-renders everything that shows copy. */
function refreshWhenLoaded(): void {
  const locale = resolvedLanguage();
  if (locale === "zh-CN" || catalogs[locale]) return;
  void ensureLocale(locale).then(() => {
    if (resolvedLanguage() === locale) language.set(state => ({ revision: state.revision + 1 }));
  });
}

/** Android WebView may retain an English navigator locale while the OS is Chinese. */
export function setSystemLanguageOverride(locale?: string): void {
  nativeSystemLanguage = typeof locale === "string" && /^[A-Za-z]{2,3}(?:[-_][A-Za-z0-9]{2,8})*$/.test(locale)
    ? locale.replaceAll("_", "-") : undefined;
  if (typeof document !== "undefined") document.documentElement.lang = resolvedLanguage();
  refreshWhenLoaded();
}

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
  return nativeSystemLanguage ? resolveSystemLanguage(nativeSystemLanguage)
    : typeof document === "undefined" ? "zh-CN" : resolveSystemLanguage(globalThis.navigator?.language ?? "en");
}

export function setLanguage(preference: Language): void {
  language.set({ preference });
  try { localStorage.setItem(KEY, preference); } catch { /* Optional browser cache. */ }
  if (typeof document !== "undefined") document.documentElement.lang = resolvedLanguage();
  refreshWhenLoaded();
}

/** Only developer-authored UI copy is passed here. Remote names, paths and identifiers are untouched. */
export function tr(text: string): string {
  const locale = resolvedLanguage();
  if (locale === "zh-CN") return text;
  for (const part of catalogs[locale] ?? []) {
    const hit = part[text];
    if (hit !== undefined) return hit;
  }
  return text;
}
/** Interpolated values are inserted after translating the developer-authored template. */
export function tx(template: string, ...values: (string | number | undefined)[]): string {
  return tr(template).replace(/\{(\d+)\}/g, (_, index: string) => String(values[Number(index)] ?? ""));
}
