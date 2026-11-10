//! Routes, workspace tabs with per-tab history, and the sidebar model.
use crate::{
    app::{App, Ctx},
    format,
    i18n::{tr, tx},
    overlay::MenuItem,
    NavEntry, Shell, TabEntry,
};
use nekopak_domain::*;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

pub const ROOT_ID: &str = "root";
const MAX_TABS: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Files { folder: String },
    Search { query: String },
    Recent,
    Starred,
    Trash,
    Shares,
    Transfers { tab: String },
    Sync,
    Mount,
    Local,
    References,
    Settings { section: String },
    Duplicates { folder: String },
    Space { folder: String },
    Archive { file: String, path: String },
    SharedFolder { access: String, parent: String, title: String },
}

impl Route {
    pub fn root() -> Self {
        Self::Files { folder: ROOT_ID.into() }
    }

    /// Identifier of the Slint page that renders this route.
    pub fn page(&self) -> &'static str {
        match self {
            Self::Files { .. } | Self::Search { .. } | Self::Recent | Self::Starred | Self::Trash
            | Self::SharedFolder { .. } => "files",
            Self::Shares => "shares",
            Self::Transfers { .. } => "transfers",
            Self::Sync => "sync",
            Self::Mount => "mount",
            Self::Local => "local",
            Self::References => "references",
            Self::Settings { .. } => "settings",
            Self::Duplicates { .. } => "duplicates",
            Self::Space { .. } => "space",
            Self::Archive { .. } => "archive",
        }
    }

    pub fn default_title(&self) -> String {
        match self {
            Self::Files { folder } if folder == ROOT_ID => tr("我的资料"),
            Self::Files { .. } => tr("文件夹"),
            Self::Search { query } => tx("搜索“{0}”", &[query]),
            Self::Recent => tr("最近"),
            Self::Starred => tr("星标"),
            Self::Trash => tr("回收站"),
            Self::Shares => tr("我的分享"),
            Self::Transfers { .. } => tr("下载"),
            Self::Sync => tr("自动同步"),
            Self::Mount => tr("挂载"),
            Self::Local => tr("本地文件夹"),
            Self::References => tr("引用归档"),
            Self::Settings { .. } => tr("设置"),
            Self::Duplicates { .. } => tr("查找重复"),
            Self::Space { .. } => tr("空间分布"),
            Self::Archive { .. } => tr("压缩包"),
            Self::SharedFolder { title, .. } => title.clone(),
        }
    }
}

pub struct Tab {
    pub id: i32,
    pub history: Vec<Route>,
    pub at: usize,
    /// Title resolved after load (folder name); cleared on navigation.
    pub title: Option<String>,
}

impl Tab {
    pub fn route(&self) -> &Route {
        &self.history[self.at]
    }
}

pub struct Nav {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: i32,
}

impl Default for Nav {
    fn default() -> Self {
        Self { tabs: vec![Tab { id: 1, history: vec![Route::root()], at: 0, title: None }], active: 0, next_id: 2 }
    }
}

impl Nav {
    pub fn current(&self) -> &Tab {
        &self.tabs[self.active]
    }
}

pub fn install(ctx: &Ctx) {
    let shell = ctx.ui.global::<Shell>();
    let weak = Rc::downgrade(ctx);
    macro_rules! handler {
        (|$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = weak.clone();
            move |$($arg),*| {
                if let Some($ctx) = weak.upgrade() {
                    $body
                }
            }
        }};
    }
    shell.on_navigate(handler!(|ctx, id| ctx.navigate_id(&id)));
    shell.on_nav_context(handler!(|ctx, id, x, y| {
        if let Some(pin) = id.strip_prefix("pin:") {
            let pin = pin.to_owned();
            ctx.spawn({
                let ctx = ctx.clone();
                async move { pin_menu(&ctx, pin, x, y).await }
            });
        }
    }));
    shell.on_toggle_sidebar(handler!(|ctx| ctx.toggle_sidebar()));
    shell.on_open_transfers(handler!(|ctx| ctx.navigate(Route::Transfers { tab: "all".into() })));
    shell.on_open_account(handler!(|ctx| ctx.navigate(Route::Settings { section: "account".into() })));
    shell.on_open_settings(handler!(|ctx| ctx.navigate(Route::Settings { section: "general".into() })));
    shell.on_tab_activate(handler!(|ctx, id| ctx.activate_tab(id)));
    shell.on_tab_close(handler!(|ctx, id| ctx.close_tab(Some(id))));
    shell.on_tab_new(handler!(|ctx| {
        ctx.open_tab(Route::root(), None);
    }));
}

async fn pin_menu(ctx: &Ctx, folder: String, x: f32, y: f32) {
    let name = ctx
        .prefs
        .borrow()
        .account
        .pins
        .iter()
        .find(|pin| pin.id == folder)
        .map(|pin| pin.name.clone())
        .unwrap_or_default();
    let choice = ctx
        .menu(
            x,
            y,
            &name,
            vec![
                MenuItem::new("open", tr("打开")).icon("folder"),
                MenuItem::new("tab", tr("在新标签页打开")).icon("plus"),
                MenuItem::separator(),
                MenuItem::new("unpin", tr("取消固定")).icon("unpin"),
            ],
        )
        .await;
    match choice.as_deref() {
        Some("open") => ctx.navigate(Route::Files { folder }),
        Some("tab") => {
            ctx.open_tab(Route::Files { folder }, Some(name));
        }
        Some("unpin") => {
            crate::actions::toggle_pin(ctx, &folder, &name);
        }
        _ => {}
    }
}

impl App {
    pub fn route(&self) -> Route {
        self.nav.borrow().current().route().clone()
    }

    pub fn tab_id(&self) -> i32 {
        self.nav.borrow().current().id
    }

    pub fn navigate(self: &Rc<Self>, route: Route) {
        {
            let mut nav = self.nav.borrow_mut();
            let active = nav.active;
            let tab = &mut nav.tabs[active];
            if *tab.route() != route {
                tab.history.truncate(tab.at + 1);
                tab.history.push(route);
                // Bound memory for very long sessions.
                if tab.history.len() > 200 {
                    tab.history.remove(0);
                }
                tab.at = tab.history.len() - 1;
                tab.title = None;
            }
        }
        self.route_changed();
    }

    /// Replaces the current history entry (e.g. switching a settings section).
    pub fn replace_route(self: &Rc<Self>, route: Route) {
        {
            let mut nav = self.nav.borrow_mut();
            let active = nav.active;
            let tab = &mut nav.tabs[active];
            let at = tab.at;
            tab.history[at] = route;
        }
        self.route_changed();
    }

    pub fn navigate_history(self: &Rc<Self>, delta: i32) {
        let moved = {
            let mut nav = self.nav.borrow_mut();
            let active = nav.active;
            let tab = &mut nav.tabs[active];
            let next = tab.at as i32 + delta;
            if next >= 0 && (next as usize) < tab.history.len() {
                tab.at = next as usize;
                tab.title = None;
                true
            } else {
                false
            }
        };
        if moved {
            self.route_changed();
        }
    }

    pub fn can_go(&self, delta: i32) -> bool {
        let nav = self.nav.borrow();
        let tab = nav.current();
        let next = tab.at as i32 + delta;
        next >= 0 && (next as usize) < tab.history.len()
    }

    pub fn open_tab(self: &Rc<Self>, route: Route, title: Option<String>) -> bool {
        {
            let mut nav = self.nav.borrow_mut();
            if nav.tabs.len() >= MAX_TABS {
                drop(nav);
                self.toast(tr("最多同时打开12个标签页"), "info");
                return false;
            }
            let id = nav.next_id;
            nav.next_id += 1;
            let at = nav.active + 1;
            nav.tabs.insert(at, Tab { id, history: vec![route], at: 0, title });
            nav.active = at;
        }
        self.route_changed();
        true
    }

    pub fn close_tab(self: &Rc<Self>, id: Option<i32>) {
        {
            let mut nav = self.nav.borrow_mut();
            if nav.tabs.len() <= 1 {
                return;
            }
            let index = match id {
                Some(id) => match nav.tabs.iter().position(|tab| tab.id == id) {
                    Some(index) => index,
                    None => return,
                },
                None => nav.active,
            };
            nav.tabs.remove(index);
            if nav.active > index || nav.active >= nav.tabs.len() {
                nav.active = nav.active.saturating_sub(1);
            }
        }
        self.route_changed();
    }

    pub fn activate_tab(self: &Rc<Self>, id: i32) {
        let changed = {
            let mut nav = self.nav.borrow_mut();
            match nav.tabs.iter().position(|tab| tab.id == id) {
                Some(index) if index != nav.active => {
                    nav.active = index;
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.route_changed();
        }
    }

    pub fn cycle_tab(self: &Rc<Self>, delta: i32) {
        {
            let mut nav = self.nav.borrow_mut();
            let count = nav.tabs.len() as i32;
            nav.active = (nav.active as i32 + delta).rem_euclid(count) as usize;
        }
        self.route_changed();
    }

    /// Records the resolved title (folder name) of the active tab's current page.
    pub fn set_tab_title(&self, route: &Route, title: &str) {
        {
            let mut nav = self.nav.borrow_mut();
            let active = nav.active;
            let tab = &mut nav.tabs[active];
            if tab.route() != route {
                return;
            }
            tab.title = Some(title.to_owned());
        }
        self.refresh_tabs();
    }

    fn navigate_id(self: &Rc<Self>, id: &str) {
        let route = match id {
            "files" => Route::root(),
            "recent" => Route::Recent,
            "starred" => Route::Starred,
            "shares" => Route::Shares,
            "trash" => Route::Trash,
            "sync" => Route::Sync,
            "mount" => Route::Mount,
            "local" => Route::Local,
            "references" => Route::References,
            other => match other.strip_prefix("pin:") {
                Some(folder) => Route::Files { folder: folder.into() },
                None => return,
            },
        };
        self.navigate(route);
    }

    pub fn toggle_sidebar(self: &Rc<Self>) {
        let collapsed = {
            let mut prefs = self.prefs.borrow_mut();
            prefs.device.sidebar_collapsed = !prefs.device.sidebar_collapsed;
            prefs.save_device();
            prefs.device.sidebar_collapsed
        };
        self.ui.global::<Shell>().set_collapsed_pref(collapsed);
    }

    /// Applies the active route: shell chrome first, then the page controller.
    pub fn route_changed(self: &Rc<Self>) {
        let route = self.route();
        self.finish_menu(None);
        self.ui.global::<Shell>().set_page(route.page().into());
        self.refresh_tabs();
        self.refresh_shell();
        crate::pages::enter(self, &route);
    }

    pub fn refresh_tabs(&self) {
        let nav = self.nav.borrow();
        let tabs: Vec<TabEntry> = nav
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabEntry {
                id: tab.id,
                title: tab.title.clone().unwrap_or_else(|| tab.route().default_title()).into(),
                icon: slint::Image::default(),
                active: index == nav.active,
            })
            .collect();
        self.ui.global::<Shell>().set_tabs(ModelRc::new(VecModel::from(tabs)));
    }

    /// Rebuilds the sidebar: navigation, live transfer summary and account line.
    pub fn refresh_shell(&self) {
        let shell = self.ui.global::<Shell>();
        let route = self.route();
        let prefs = self.prefs.borrow();
        let current_folder = match &route {
            Route::Files { folder } => Some(folder.as_str()),
            _ => None,
        };
        let pin_active = current_folder.is_some_and(|folder| prefs.is_pinned(folder));
        let entry = |id: &str, label: String, icon: &str, active: bool, tip: &str| NavEntry {
            id: id.into(),
            label: label.into(),
            icon: self.icon(icon),
            icon_active: self.icon(&format!("{icon}-fill")),
            active,
            badge: Default::default(),
            tip: tip.into(),
            pinned: id.starts_with("pin:"),
        };
        let mut quick = vec![entry(
            "files",
            tr("网盘"),
            "cloud",
            (matches!(route, Route::Files { .. }) && !pin_active) || matches!(route, Route::Search { .. }),
            "Ctrl+1",
        )];
        for pin in &prefs.account.pins {
            quick.push(entry(
                &format!("pin:{}", pin.id),
                pin.name.clone(),
                "folder",
                current_folder == Some(pin.id.as_str()),
                "",
            ));
        }
        let library = vec![
            entry("recent", tr("最近"), "clock", route == Route::Recent, "Ctrl+3"),
            entry("starred", tr("星标"), "star", route == Route::Starred, "Ctrl+4"),
            entry("shares", tr("我的分享"), "share", route == Route::Shares, "Ctrl+5"),
            entry("trash", tr("回收站"), "trash", route == Route::Trash, "Ctrl+6"),
        ];
        let tools = vec![
            entry("sync", tr("自动同步"), "sync", route == Route::Sync, "Ctrl+7"),
            entry("mount", tr("挂载"), "drive", route == Route::Mount, "Ctrl+8"),
            entry("local", tr("本地文件夹"), "folder", route == Route::Local, ""),
            entry("references", tr("引用归档"), "archive", route == Route::References, ""),
        ];
        shell.set_nav_quick(ModelRc::new(VecModel::from(quick)));
        shell.set_nav_library(ModelRc::new(VecModel::from(library)));
        shell.set_nav_tools(ModelRc::new(VecModel::from(tools)));
        shell.set_transfers_active(matches!(route, Route::Transfers { .. }));
        shell.set_collapsed_pref(prefs.device.sidebar_collapsed);
        shell.set_demo(self.mock);
        drop(prefs);

        let core = self.core.borrow();
        let account_page = matches!(&route, Route::Settings { section } if section == "account");
        shell.set_account_active(account_page);
        shell.set_settings_active(matches!(route, Route::Settings { .. }) && !account_page);
        let auth = core.auth.as_ref();
        let signed_in = auth.is_some_and(|auth| auth.authenticated);
        let expired = auth.is_some_and(|auth| auth.state == AuthStatus::Expired);
        let name = if self.mock {
            tr("未连接 PikPak")
        } else if signed_in {
            auth.and_then(|auth| auth.display_name.clone()).unwrap_or_else(|| tr("已登录"))
        } else if expired {
            tr("登录已过期")
        } else {
            tr("未登录")
        };
        let quota_line = core.quota.as_ref().and_then(|quota| {
            ["download_daily", "download"].iter().find_map(|wanted| {
                let dimension = quota.dimensions.iter().find(|d| d.dimension == *wanted)?;
                let remaining = dimension.remaining_bytes?;
                let period = if *wanted == "download_daily" { tr("今日") } else { tr("本月") };
                Some(tx("{0}剩余 {1}", &[&period, &format::bytes(remaining.0)]))
            })
        });
        let line = if self.mock {
            tr("离线模拟 · 无真实账号")
        } else if !signed_in {
            if expired {
                tr("登录已过期")
            } else {
                tr("未登录")
            }
        } else if let Some(line) = quota_line {
            line
        } else {
            match auth.map(|auth| auth.account_tier) {
                Some(AccountTier::Premium) => "Premium".into(),
                Some(AccountTier::Free) => "Free".into(),
                _ => tr("账号类型未提供"),
            }
        };
        let exhausted = core.quota.as_ref().is_some_and(|quota| {
            quota.dimensions.iter().any(|dimension| dimension.remaining_bytes == Some(DecimalU64(0)))
        });
        shell.set_account_initial(if signed_in && !self.mock {
            name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into()
        } else {
            Default::default()
        });
        shell.set_account_name(name.into());
        shell.set_account_sub(line.into());
        shell.set_account_warn(exhausted || expired);
    }
}
