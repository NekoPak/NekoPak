use crate::app::Ctx;
use nekopak_domain::RemoteEntry;
pub async fn add_link_dialog(_ctx: &Ctx, _prefill: Option<String>) {}
pub async fn share_dialog(_ctx: &Ctx, _entries: Vec<RemoteEntry>) {}
