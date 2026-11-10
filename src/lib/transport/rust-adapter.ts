/** Wire contract v0.2 -> supplied UI's display models. Rust owns all real state. */
import type * as Wire from "../api-types";
import type { AppStatus, AuthState, QuotaReport, QuotaItem } from "../contract-proposals";
import { tr } from "../i18n";

export function statusForUi(status: Wire.AppStatus): AppStatus {
  return {
    system_locale: status.system_locale,
    version: status.version, contract_version: status.contract_version, runtime: "tauri", mode: status.mode,
    storage: { status: status.storage.status === "ready" ? "ok" : "corrupt_readonly", message: status.storage.error?.message },
    feature_flags: { ...status.features, auth_flow: false },
  };
}

export function authForUi(auth: Wire.AuthState): AuthState {
  return {
    state: auth.backend === "mock" ? "demo" : auth.state,
    account_id: auth.account_id,
    backend: auth.backend, credential_owner: auth.credential_owner, message: auth.message,
    ...(auth.authenticated ? { account: { display_name: auth.display_name ?? tr("已登录"), tier: auth.account_tier } } : {}),
    checked_at: new Date().toISOString(),
  };
}

export function quotaForUi(quota: Wire.QuotaReport): QuotaReport {
  const dimensions: Record<string, QuotaItem["dimension"]> = { storage: "storage", download: "downstream_monthly", download_daily: "downstream_daily", upload: "upload_monthly", cloud_download: "cloud_download_monthly" };
  const items = quota.dimensions.filter(d => d.dimension in dimensions).map((dimension): QuotaItem => ({
    dimension: dimensions[dimension.dimension], scope: quota.source === "account" ? "account" : "connected_apps", unit: "bytes",
    limit: dimension.limit_bytes, remaining: dimension.remaining_bytes,
    used: dimension.used_bytes,
    exactness: dimension.remaining_bytes === undefined && dimension.used_bytes === undefined ? "unavailable" : quota.source === "account" ? "exact" : "estimated",
    state: dimension.remaining_bytes === undefined ? "unknown" : dimension.remaining_bytes === "0" ? "exhausted" : "ok",
    resets_at: dimension.resets_at,
    note: quota.source === "account" ? quota.limitations.join(" ") : `模拟额度（${dimension.period}），不是实际账号余额。${quota.limitations.join(" ")}`,
  }));
  return { backend: quota.backend, account_tier: quota.account_tier, items, checked_at: quota.checked_at };
}

export function unsupported(command: string): Wire.ClientError {
  return { code: "UNSUPPORTED", message: tr("当前版本尚未支持这项操作。"), retryable: false, details: { command } };
}

export const RUST_COMMANDS = new Set([
  "cloud_torrent_pick", "cloud_torrent_create", "cloud_torrent_release",
  "transfer_forget", "transfer_clear_finished", "auth_preferences_get", "auth_preferences_update", "network_reconnect", "network_domains_probe",
  "app_status", "backend_discover", "auth_status", "files_list", "files_stat", "files_path",
  "auth_sign_in", "auth_verify", "auth_logout",
  "accounts_list", "account_switch", "account_remove",
  "ui_prefs_get", "ui_prefs_update",
  "mount_capabilities", "mount_configs_list", "mount_config_save", "mount_start", "mount_stop", "mount_status",
  "verification_status", "verification_open", "verification_complete", "verification_cancel",
  "files_recent", "files_starred", "files_search", "trash_list", "files_mutate", "shares_list",
  "settings_get", "settings_update",
  "autostart_get", "autostart_set",
  "reference_create", "reference_list", "reference_status", "reference_store_manifest", "reference_archive", "reference_restore",
  "download_directory_pick", "transfer_open_directory", "transfer_export",
  "desktop_status",
  "transfer_start_batch", "duplicates_scan", "duplicates_report", "duplicates_cancel", "files_space_summary",
  "local_sources_list", "local_sources_add", "local_sources_remove", "media_session_open", "media_session_close",
  "share_create", "shares_cancel", "cloud_task_preview", "archive_list", "archive_unlock", "archive_extract", "jobs_list", "upload_pick_and_start",
  "upload_directory_pick_and_start",
  "sync_configs_list", "sync_config_save", "sync_scan", "sync_plan", "sync_apply", "sync_run_status", "sync_history",
  "media_probe", "media_align", "media_open_external", "player_config_pick", "share_open", "share_folder", "share_restore", "share_preview_entry",
  "transfers_list", "transfer_start", "transfer_control", "cloud_task_create", "quota_get",
]);

export function adaptResult(command: string, value: unknown): unknown {
  switch (command) {
    case "app_status": return statusForUi(value as Wire.AppStatus);
    case "auth_status": case "account_switch": return authForUi(value as Wire.AuthState);
    case "quota_get": return quotaForUi(value as Wire.QuotaReport);
    default: return value;
  }
}
