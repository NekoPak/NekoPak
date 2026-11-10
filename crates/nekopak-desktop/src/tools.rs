use crate::{app::Ctx, nav::Route};
use nekopak_domain::RemoteEntry;
pub fn enter(_ctx: &Ctx, _route: &Route) {}
pub async fn batch_rename(_ctx: &Ctx, _entries: Vec<RemoteEntry>) {}
pub async fn create_reference(_ctx: &Ctx, _entries: Vec<RemoteEntry>) {}
pub fn on_scan_progress(_ctx: &Ctx, _event: serde_json::Value) {}
pub fn on_sync_changed(_ctx: &Ctx, _event: serde_json::Value, _review: bool) {}
