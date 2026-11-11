//! Helper for pages rendered by the generic panel component.
use crate::{
    app::{App, Ctx},
    overlay::MenuItem,
    Choice, Panel, PanelRow,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct Row {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub value: String,
    pub icon: &'static str,
    pub file_kind: Option<i32>,
    pub tone: &'static str,
    pub checked: bool,
    pub disabled: bool,
    pub progress: f32,
    pub action: String,
    pub action2: String,
    pub danger: bool,
    pub menu: bool,
    pub selected: bool,
    pub options: Vec<(String, String, bool)>,
}

impl Row {
    fn of(id: &str, kind: &'static str, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            title: title.into(),
            ..Self::default()
        }
    }
    pub fn header(title: impl Into<String>) -> Self {
        Self::of("", "header", title)
    }
    pub fn note(text: impl Into<String>, tone: &'static str) -> Self {
        Self {
            tone,
            ..Self::of("", "note", text)
        }
    }
    pub fn info(title: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::of("", "info", title)
        }
    }
    pub fn toggle(id: &str, title: impl Into<String>, checked: bool) -> Self {
        Self {
            checked,
            ..Self::of(id, "toggle", title)
        }
    }
    pub fn select(id: &str, title: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::of(id, "select", title)
        }
    }
    pub fn button(id: &str, title: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ..Self::of(id, "button", title)
        }
    }
    pub fn item(id: &str, title: impl Into<String>) -> Self {
        Self::of(id, "item", title)
    }
    pub fn progress(id: &str, title: impl Into<String>, value: f32) -> Self {
        Self {
            progress: value,
            ..Self::of(id, "progress", title)
        }
    }
    pub fn segment(
        id: &str,
        title: impl Into<String>,
        options: Vec<(String, String, bool)>,
    ) -> Self {
        Self {
            options,
            ..Self::of(id, "segment", title)
        }
    }
    pub fn detail(mut self, text: impl Into<String>) -> Self {
        self.detail = text.into();
        self
    }
    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.value = text.into();
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }
    pub fn file_kind(mut self, kind: i32) -> Self {
        self.file_kind = Some(kind);
        self
    }
    pub fn tone(mut self, tone: &'static str) -> Self {
        self.tone = tone;
        self
    }
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = label.into();
        self
    }
    pub fn action2(mut self, label: impl Into<String>) -> Self {
        self.action2 = label.into();
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn menu(mut self) -> Self {
        self.menu = true;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Default)]
pub struct View {
    pub title: String,
    pub subtitle: String,
    pub icon: &'static str,
    pub back: bool,
    pub actions: Vec<MenuItem>,
    pub tabs: Vec<(String, String, String, bool)>,
    pub rows: Vec<Row>,
}

#[allow(dead_code)] // coordinates are provided for handlers that open menus
pub enum Event {
    Action(String, f32, f32),
    Tab(String),
    Row(String, String, f32, f32),
    StateAction,
}

type Handler = Rc<dyn Fn(&Ctx, Event)>;

#[derive(Default)]
pub struct PanelState {
    handler: Option<Handler>,
    /// Identifies the page that owns the panel so stale loads cannot repaint it.
    pub owner: u64,
}

pub fn install(ctx: &Ctx) {
    let panel = ctx.ui.global::<Panel>();
    let weak = Rc::downgrade(ctx);
    let dispatch = move |event: Event| {
        if let Some(ctx) = weak.upgrade() {
            let handler = ctx.panel.borrow().handler.clone();
            if let Some(handler) = handler {
                handler(&ctx, event);
            }
        }
    };
    panel.on_action({
        let dispatch = dispatch.clone();
        move |id, x, y| dispatch(Event::Action(id.into(), x, y))
    });
    panel.on_tab({
        let dispatch = dispatch.clone();
        move |id| dispatch(Event::Tab(id.into()))
    });
    panel.on_row({
        let dispatch = dispatch.clone();
        move |id, what, x, y| dispatch(Event::Row(id.into(), what.into(), x, y))
    });
    panel.on_state_action_clicked({
        let dispatch = dispatch.clone();
        move || dispatch(Event::StateAction)
    });
    let weak = Rc::downgrade(ctx);
    panel.on_back(move || {
        if let Some(ctx) = weak.upgrade() {
            ctx.navigate_history(-1);
        }
    });
}

impl App {
    /// Claims the panel for a page and returns a token for `panel_owned`.
    pub fn panel_begin(&self, handler: impl Fn(&Ctx, Event) + 'static) -> u64 {
        let mut panel = self.panel.borrow_mut();
        panel.owner += 1;
        panel.handler = Some(Rc::new(handler));
        panel.owner
    }

    pub fn panel_owned(&self, token: u64) -> bool {
        self.panel.borrow().owner == token
    }

    pub fn panel_loading(&self, title: &str, icon: &'static str) {
        let panel = self.ui.global::<Panel>();
        panel.set_title(title.into());
        panel.set_subtitle(Default::default());
        panel.set_icon(self.icon(icon));
        panel.set_can_back(self.can_go(-1));
        panel.set_actions(ModelRc::default());
        panel.set_tabs(ModelRc::default());
        panel.set_rows(ModelRc::default());
        panel.set_phase("loading".into());
    }

    pub fn panel_state(
        &self,
        error: bool,
        icon: &'static str,
        title: &str,
        body: &str,
        action: &str,
    ) {
        let panel = self.ui.global::<Panel>();
        panel.set_phase(if error { "error" } else { "empty" }.into());
        panel.set_state_error(error);
        panel.set_state_icon(self.icon(icon));
        panel.set_state_title(title.into());
        panel.set_state_body(body.into());
        panel.set_state_action(action.into());
    }

    pub fn panel_show(&self, view: View) {
        let panel = self.ui.global::<Panel>();
        panel.set_title(view.title.into());
        panel.set_subtitle(view.subtitle.into());
        panel.set_icon(if view.icon.is_empty() {
            slint::Image::default()
        } else {
            self.icon(view.icon)
        });
        panel.set_can_back(view.back && self.can_go(-1));
        panel.set_actions(ModelRc::new(VecModel::from(
            self.menu_entries(&view.actions),
        )));
        panel.set_tabs(ModelRc::new(VecModel::from(
            view.tabs
                .into_iter()
                .map(|(id, label, detail, selected)| Choice {
                    id: id.into(),
                    label: label.into(),
                    detail: detail.into(),
                    selected,
                    enabled: true,
                })
                .collect::<Vec<_>>(),
        )));
        let rows: Vec<PanelRow> = view
            .rows
            .into_iter()
            .map(|row| PanelRow {
                id: row.id.into(),
                kind: row.kind.into(),
                title: row.title.into(),
                detail: row.detail.into(),
                value: row.value.into(),
                icon: if row.icon.is_empty() {
                    slint::Image::default()
                } else {
                    self.icon(row.icon)
                },
                file_kind: row.file_kind.unwrap_or(-1),
                tone: row.tone.into(),
                checked: row.checked,
                enabled: !row.disabled,
                progress: row.progress,
                action: row.action.into(),
                action2: row.action2.into(),
                danger: row.danger,
                menu: row.menu,
                selected: row.selected,
                options: ModelRc::new(VecModel::from(
                    row.options
                        .into_iter()
                        .map(|(id, label, selected)| Choice {
                            id: id.into(),
                            label: label.into(),
                            detail: Default::default(),
                            selected,
                            enabled: true,
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        panel.set_rows(ModelRc::new(VecModel::from(rows)));
        panel.set_phase("ready".into());
    }
}
