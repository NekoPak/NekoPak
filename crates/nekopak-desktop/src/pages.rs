//! Dispatches a route to the controller that owns its page.
use crate::{app::Ctx, nav::Route};

pub fn enter(ctx: &Ctx, route: &Route) {
    match route.page() {
        "files" => crate::files::enter(ctx, route),
        "transfers" => crate::transfers::enter(ctx, route),
        "settings" => crate::settings::enter(ctx, route),
        "gallery" => crate::gallery::enter(ctx, route),
        _ => crate::tools::enter(ctx, route),
    }
}
