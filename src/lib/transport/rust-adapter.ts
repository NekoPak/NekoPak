/** Wire contract v0.1 -> supplied UI's display models. No fake success or backend state. */
import type * as Wire from "../api-types";
import type { AppStatus, AuthState, QuotaReport, QuotaItem } from "../contract-proposals";

export function statusForUi(status: Wire.AppStatus): AppStatus {
  return {
    version: status.version, contract_version: status.contract_version, runtime: "tauri", mode: status.mode,
    storage: { status: status.storage.status === "ready" ? "ok" : "corrupt_readonly", message: status.storage.error?.message },
    feature_flags: { ...status.features, settings: false, auth_flow: false, auth_logout: false, sync: false, mount: false },
  };
}

export function authForUi(auth: Wire.AuthState): AuthState {
  return {
    state: auth.backend === "mock" ? "demo" : auth.authenticated ? "signed_in" : "signed_out",
    backend: auth.backend, credential_owner: auth.credential_owner, message: auth.message,
    ...(auth.authenticated ? { account: { display_name: "已授权的外部 CLI 账号", tier: auth.account_tier } } : {}),
    checked_at: new Date().toISOString(),
  };
}

export function quotaForUi(quota: Wire.QuotaReport): QuotaReport {
  const dimensions: Record<string, QuotaItem["dimension"]> = { download: "downstream_monthly", upload: "upload_monthly", cloud_download: "cloud_download_monthly" };
  const items = quota.dimensions.filter(d => d.dimension in dimensions).map((dimension): QuotaItem => ({
    dimension: dimensions[dimension.dimension], scope: "connected_apps", unit: "bytes",
    limit: dimension.limit_bytes, remaining: dimension.remaining_bytes,
    exactness: dimension.remaining_bytes === undefined ? "unavailable" : "estimated",
    state: dimension.remaining_bytes === undefined ? "unknown" : dimension.remaining_bytes === "0" ? "exhausted" : "ok",
    resets_at: dimension.resets_at,
    note: `模拟额度（${dimension.period}），不是实际账号余额。${quota.limitations.join(" ")}`,
  }));
  return { backend: quota.backend, account_tier: quota.account_tier, items, checked_at: quota.checked_at };
}

export function unsupported(command: string): Wire.ClientError {
  return { code: "UNSUPPORTED", message: `当前 Rust 后端尚未实现 ${command}；浏览器 Mock 的演示行为不会替代实际能力。`, retryable: false };
}

export const RUST_COMMANDS = new Set([
  "app_status", "backend_discover", "auth_status", "files_list", "files_stat", "files_path",
  "transfers_list", "transfer_start", "transfer_control", "cloud_task_create", "quota_get",
]);

export function adaptResult(command: string, value: unknown): unknown {
  switch (command) {
    case "app_status": return statusForUi(value as Wire.AppStatus);
    case "auth_status": return authForUi(value as Wire.AuthState);
    case "quota_get": return quotaForUi(value as Wire.QuotaReport);
    default: return value;
  }
}
