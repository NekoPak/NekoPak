import { tr, tx } from "./i18n";
import type { BackendId, ClientError, ClientErrorCode } from "./contract";

const CODES: ReadonlySet<ClientErrorCode> = new Set<ClientErrorCode>([
  "AUTH_EXPIRED", "ACCOUNT_RESTRICTED", "PERMISSION_DENIED",
  "VERIFICATION_REQUIRED",
  "QUOTA_EXCEEDED", "NOT_FOUND", "UNSUPPORTED",
  "TRANSIENT_NETWORK", "DISK_FULL", "INTEGRITY_FAILED",
  "CONFLICT", "CANCELLED", "INTERNAL",
]);

/**
 * Defensive redaction. Rust is responsible for never sending secrets, but
 * the UI scrubs again before rendering or copying anything.
 */
export function redact(text: string): string {
  return text
    // query strings & fragments on any URL (signed download links etc.)
    .replace(/\b((?:https?|wss?|ftp):\/\/[^\s?#"'<>]+)[?#][^\s"'<>]*/gi, "$1?<已隐藏>")
    .replace(/\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}/gi, "$1 <已隐藏>")
    .replace(
      /\b(access_token|refresh_token|id_token|token|secret|password|passwd|pwd|cookie|session|sig|signature|captcha_token|authorization)\b\s*[=:]\s*("[^"]*"|'[^']*'|[^\s&;,]+)/gi,
      "$1=<已隐藏>",
    )
    .replace(/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g, "<JWT 已隐藏>");
}

function redactDetails(details: unknown, depth = 0): Record<string, unknown> | undefined {
  if (!details || typeof details !== "object" || depth > 3) return undefined;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(details as Record<string, unknown>)) {
    if (/token|secret|password|cookie|auth|sig/i.test(k)) {
      out[k] = tr("<已隐藏>");
    } else if (typeof v === "string") {
      out[k] = redact(v);
    } else if (typeof v === "number" || typeof v === "boolean" || v === null) {
      out[k] = v;
    } else if (typeof v === "object") {
      out[k] = redactDetails(v, depth + 1);
    }
  }
  return out;
}

export function isClientError(e: unknown): e is ClientError {
  return (
    !!e && typeof e === "object" &&
    typeof (e as ClientError).code === "string" &&
    CODES.has((e as ClientError).code) &&
    typeof (e as ClientError).message === "string"
  );
}

/** Normalise anything thrown across the IPC boundary into a ClientError. */
export function toClientError(e: unknown, backend?: BackendId): ClientError {
  if (isClientError(e)) {
    return {
      code: e.code,
      message: redact(e.message),
      retryable: !!e.retryable,
      backend: e.backend ?? backend,
      request_id: e.request_id,
      details: redactDetails(e.details),
    };
  }
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : tr("未知错误");
  // Tauri rejects unknown commands with a string such as "command foo not found".
  const missing = /command\s+([\w:]+)\s+not\s+found/i.exec(raw);
  if (missing) {
    return {
      code: "UNSUPPORTED",
      message: tx("暂不支持（缺少命令 {0}）", missing[1]),
      retryable: false,
      backend,
      details: { command: missing[1] },
    };
  }
  return { code: "INTERNAL", message: redact(raw).slice(0, 500), retryable: false, backend };
}

export interface ErrorCopy {
  title: string;
  hint: string;
}

/** User-facing explanation per code. Quota/permission texts are explicit on purpose. */
export const ERROR_COPY: Record<ClientErrorCode, ErrorCopy> = {
  VERIFICATION_REQUIRED: {
    get title() { return tr("需要安全验证"); },
    get hint() { return tr("完成 PikPak 的安全验证后可继续读取，当前登录会话会保留。"); },
  },
  AUTH_EXPIRED: {
    get title() { return tr("登录已过期"); },
    get hint() { return tr("重新登录后即可继续。"); },
  },
  ACCOUNT_RESTRICTED: {
    get title() { return tr("账号受限"); },
    get hint() { return tr("PikPak 限制了此账号的操作，请在 PikPak 官网查看账号状态。"); },
  },
  PERMISSION_DENIED: {
    get title() { return tr("没有权限"); },
    get hint() { return tr("例如只读的共享文件夹，或需要 Premium 的功能。"); },
  },
  QUOTA_EXCEEDED: {
    get title() { return tr("额度已用完"); },
    get hint() { return tr("等额度重置后再试，可在“设置 › 配额”查看重置时间。"); },
  },
  NOT_FOUND: {
    get title() { return tr("找不到项目"); },
    get hint() { return tr("它可能已在其他设备上被移动或删除。刷新后再试。"); },
  },
  UNSUPPORTED: {
    get title() { return tr("暂不支持"); },
    get hint() { return tr("这项功能暂时不能使用。"); },
  },
  TRANSIENT_NETWORK: {
    get title() { return tr("网络连接失败"); },
    get hint() { return tr("检查网络后重试。"); },
  },
  DISK_FULL: {
    get title() { return tr("磁盘空间不足"); },
    get hint() { return tr("释放目标磁盘空间，或在设置中更换下载目录后重试。"); },
  },
  INTEGRITY_FAILED: {
    get title() { return tr("完整性校验失败"); },
    get hint() { return tr("下载内容与远端信息不一致。已保留 .part 现场供排查，可重新下载。"); },
  },
  CONFLICT: {
    get title() { return tr("存在冲突"); },
    get hint() { return tr("目标位置已有同名项，或数据在此期间已被修改。刷新后重新确认。"); },
  },
  CANCELLED: {
    get title() { return tr("已取消"); },
    get hint() { return tr("操作已被取消。"); },
  },
  INTERNAL: {
    get title() { return tr("内部错误"); },
    get hint() { return tr("复制诊断信息并反馈问题，诊断信息不含令牌和下载链接。"); },
  },
};

/** Text copied by “复制诊断信息”. Never includes tokens or signed URLs. */
export function diagnosticsText(err: ClientError, context?: string): string {
  const lines = [
    `NekoPak diagnostic`,
    `code: ${err.code}`,
    `message: ${redact(err.message)}`,
    `retryable: ${err.retryable}`,
    `backend: ${err.backend ?? "unknown"}`,
    `request_id: ${err.request_id ?? "none"}`,
  ];
  if (context) lines.push(`context: ${redact(context)}`);
  if (err.details) lines.push(`details: ${redact(JSON.stringify(err.details))}`);
  lines.push(`ui_time: ${new Date().toISOString()}`);
  return lines.join("\n");
}
