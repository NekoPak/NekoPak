use crate::app::Ctx;
use nekopak_domain::RemoteEntry;
pub async fn play(_ctx: &Ctx, _entry: RemoteEntry) {}
pub async fn download_quality(_ctx: &Ctx, _entries: Vec<RemoteEntry>) {}
pub async fn clip_dialog(_ctx: &Ctx, _entry: RemoteEntry) {}
