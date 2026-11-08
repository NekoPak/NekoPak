import { describe, expect, it } from "vitest";
import { adaptResult, authForUi, RUST_COMMANDS, statusForUi, quotaForUi } from "../../src/lib/transport/rust-adapter";
import { applyProgress, applyChanged } from "../../src/state/reconcile";
import type { TransferTaskExt } from "../../src/lib/contract-proposals";
import free from "../fixtures/free.json";
import loggedOut from "../fixtures/native_unconfigured.json";
import type { AppStatus, QuotaReport, AuthState } from "../../src/lib/api-types";

describe("supplied UI maps the actual Rust wire shapes", () => {
  it("keeps Rust Mock a demo rather than inventing a signed-in account", () => {
    const auth = authForUi({ backend: "mock", authenticated: false, state: "signed_out", account_tier: "unknown", credential_owner: "none", message: "mock" });
    expect(auth.state).toBe("demo"); expect(auth.account).toBeUndefined();
    expect(authForUi(loggedOut.auth as AuthState).state).toBe("signed_out");
  });
  it("maps storage/features without interpreting readiness as login", () => {
    const status = statusForUi(free.app_status as AppStatus);
    expect(status.mode).toBe("mock"); expect(status.storage.status).toBe("ok");
    expect(status.feature_flags.sync).toBe(false);
    const damaged = statusForUi({ ...free.app_status, storage: { status: "recovery_required", schema_version: 1, error: { code: "INTEGRITY_FAILED", message: "preserved", retryable: false } } } as AppStatus);
    expect(damaged.storage.status).toBe("corrupt_readonly"); expect(damaged.storage.message).toBe("preserved");
  });
  it("shows mock quotas as synthetic and never fills missing values", () => {
    const quota = quotaForUi(free.quota_get.ok as QuotaReport);
    expect(quota.items[0].remaining).toBe("42000000000");
    expect(quota.items[0].note).toContain("模拟");
    const unknown = quotaForUi({ ...(free.quota_get.ok as QuotaReport), dimensions: [{ dimension: "download", period: "unknown" }] });
    expect(unknown.items[0].exactness).toBe("unavailable"); expect(unknown.items[0].remaining).toBeUndefined();
  });
  it("does not invent success for unimplemented commands", () => {
    for (const cmd of ["auth_begin", "auth_resend", "media_preview_frame"]) expect(RUST_COMMANDS.has(cmd)).toBe(false);
    for (const cmd of ["sync_apply", "sync_run_status", "media_session_open", "share_open", "share_restore", "share_preview_entry", "upload_directory_pick_and_start", "mount_start", "mount_configs_list", "mount_config_save", "mount_status", "accounts_list", "account_switch", "ui_prefs_get", "ui_prefs_update"]) expect(RUST_COMMANDS.has(cmd)).toBe(true);
    for (const cmd of ["files_mutate", "settings_update", "files_starred", "verification_status", "verification_open"]) expect(RUST_COMMANDS.has(cmd)).toBe(true);
    const page = { items: [] }; expect(adaptResult("files_list", page)).toBe(page);
  });
  it("keeps actual used quota beyond the limit and daily use without an invented limit", () => {
    const quota=quotaForUi({backend:"rust_compat",source:"account",account_tier:"unknown",checked_at:"2026-11-08T00:00:00Z",limitations:[],dimensions:[
      {dimension:"download",period:"month",limit_bytes:"100",used_bytes:"120",remaining_bytes:"0"},
      {dimension:"download_daily",period:"day",used_bytes:"25"}
    ]});
    expect(quota.items[0].used).toBe("120"); expect(quota.items[0].exactness).toBe("exact");
    expect(quota.items[1].used).toBe("25"); expect(quota.items[1].limit).toBeUndefined(); expect(quota.items[1].state).toBe("unknown");
  });
  it("keeps full error payload when progress and changed have the same large revision", () => {
    const task: TransferTaskExt = { id: "t", remote_id: "r", kind: "download", state: "running", transferred_bytes: "10", backend: "mock", created_at: "2026-11-07T00:00:00Z", updated_at: "2026-11-07T00:00:00Z", revision: "9007199254740993" };
    const event = { task_id: "t", state: "failed" as const, transferred_bytes: "11", revision: "9007199254740994", error: { code: "DISK_FULL" as const, message: "disk full", retryable: false } };
    const updated = applyProgress(task, event)!;
    expect(updated.error?.code).toBe("DISK_FULL"); expect(updated.revision).toBe(event.revision);
    expect(applyChanged(updated, event)).toBeNull();
    expect(applyProgress(updated, { ...event, state: "running", revision: "9007199254740993" })).toBeNull();
  });
});
