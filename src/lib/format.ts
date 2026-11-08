import { tr, tx, resolvedLanguage } from "./i18n";
/**
 * Formatting helpers. All u64 quantities arrive as decimal strings
 * (03_CONTRACTS §1) and are handled with BigInt so values above
 * Number.MAX_SAFE_INTEGER never lose precision.
 */

const U64_RE = /^\d{1,20}$/;
const U64_MAX = 18446744073709551615n;

export function parseU64(value: string | undefined | null): bigint | null {
  if (value == null || !U64_RE.test(value)) return null;
  const n = BigInt(value);
  return n > U64_MAX ? null : n;
}

export function compareU64(a: string | undefined, b: string | undefined): number {
  const x = parseU64(a);
  const y = parseU64(b);
  if (x === null && y === null) return 0;
  if (x === null) return -1;
  if (y === null) return 1;
  return x < y ? -1 : x > y ? 1 : 0;
}

export function addU64(values: Iterable<string | undefined>): bigint {
  let sum = 0n;
  for (const v of values) {
    const n = parseU64(v);
    if (n !== null) sum += n;
  }
  return sum;
}

// Windows Explorer convention: base 1024, labelled KB/MB/GB…
const UNITS = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

export function formatBytes(value: string | bigint | undefined | null): string {
  const n = typeof value === "bigint" ? value : parseU64(value ?? null);
  if (n === null) return "—";
  if (n < 1024n) return `${n} B`;
  let idx = 0;
  let div = 1n;
  while (idx < UNITS.length - 1 && n >= div * 1024n) {
    div *= 1024n;
    idx++;
  }
  // Two decimals via integer math, then trim to 3 significant figures.
  const hundredths = Number((n * 100n) / div) / 100;
  const digits = hundredths >= 100 ? 0 : hundredths >= 10 ? 1 : 2;
  return `${hundredths.toFixed(digits)} ${UNITS[idx]}`;
}

/** "9,007,199,254,740,993 字节" — exact, no float conversion. */
export function formatBytesExact(value: string | bigint | undefined | null): string {
  const n = typeof value === "bigint" ? value : parseU64(value ?? null);
  if (n === null) return tr("未提供");
  return tx("{0} 字节", groupDigits(n.toString()));
}

export function groupDigits(digits: string): string {
  return digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** Percentage with one decimal, or null if total is unknown/zero. */
export function percent(transferred: string | undefined, total: string | undefined): number | null {
  const t = parseU64(transferred ?? null);
  const all = parseU64(total ?? null);
  if (t === null || all === null || all === 0n) return null;
  const clamped = t > all ? all : t;
  return Number((clamped * 1000n) / all) / 10;
}

export function formatSpeed(bytesPerSec: number | undefined): string {
  if (bytesPerSec === undefined || !Number.isFinite(bytesPerSec) || bytesPerSec < 0) return tr("未提供");
  return `${formatBytes(BigInt(Math.round(bytesPerSec)))}/s`;
}

export function formatEta(transferred: string, total: string | undefined, bytesPerSec: number | undefined): string | null {
  const t = parseU64(transferred);
  const all = parseU64(total ?? null);
  if (t === null || all === null || !bytesPerSec || bytesPerSec <= 0) return null;
  if (t >= all) return null;
  const remaining = Number(all - t) / bytesPerSec;
  return formatDuration(remaining);
}

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.round(seconds);
  const locale = resolvedLanguage();
  if(locale === "en") {if(s<60)return `${s} s`;const m=Math.floor(s/60);if(m<60)return `${m} min ${s%60} s`;const h=Math.floor(m/60);if(h<48)return `${h} h ${m%60} min`;return `${Math.floor(h/24)} d ${h%24} h`;}
  if (locale === "ja-JP" || locale === "ko-KR" || locale === "zh-TW") {
    const units = locale === "ja-JP" ? ["秒", "分", "時間", "日"] : locale === "ko-KR" ? ["초", "분", "시간", "일"] : ["秒", "分", "小時", "天"];
    if (s < 60) return `${s} ${units[0]}`; const m = Math.floor(s / 60);
    if (m < 60) return `${m} ${units[1]} ${s % 60} ${units[0]}`; const h = Math.floor(m / 60);
    if (h < 48) return `${h} ${units[2]} ${m % 60} ${units[1]}`;
    return `${Math.floor(h / 24)} ${units[3]} ${h % 24} ${units[2]}`;
  }
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分 ${s % 60} 秒`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} 小时 ${m % 60} 分`;
  return `${Math.floor(h / 24)} 天 ${h % 24} 小时`;
}

const dateFormats = new Map<string, Intl.DateTimeFormat>();
function dateFormat(): Intl.DateTimeFormat {
  const locale = resolvedLanguage();
  let format = dateFormats.get(locale);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
    });
    dateFormats.set(locale, format);
  }
  return format;
}

export function formatDateTime(iso: string | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return dateFormat().format(d);
}

export function formatRelative(iso: string | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "—";
  const diff = Math.round((now - t) / 1000);
  if (diff < 0) return formatDateTime(iso);
  if (diff < 45) return tr("刚刚");
  if(resolvedLanguage()!=="zh-CN"){
    const format = new Intl.RelativeTimeFormat(resolvedLanguage(), { numeric: "auto" });
    if(diff<3600)return format.format(-Math.max(1,Math.round(diff/60)),"minute");
    if(diff<86400)return format.format(-Math.round(diff/3600),"hour");return formatDateTime(iso);
  }
  if (diff < 3600) return `${Math.max(1, Math.round(diff / 60))} 分钟前`;
  if (diff < 86400) return `${Math.round(diff / 3600)} 小时前`;
  return formatDateTime(iso);
}

export function plural(n: number, unit: string): string {
  return `${groupDigits(String(n))} ${unit}`;
}
