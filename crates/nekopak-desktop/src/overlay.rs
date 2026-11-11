//! Menus, modal dialogs and toasts. Each is exposed as an `async` call so
//! page logic reads top to bottom: `if ctx.confirm(..).await { .. }`.
use crate::{
    app::{error_text, request, App, Ctx},
    i18n::tr,
    Choice, Crumb, DialogSpec, Dialogs, FileRow, FormField, KeyValue, Menu, MenuEntry, ToastEntry,
    Toasts,
};
use nekopak_domain::*;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{collections::HashMap, rc::Rc, time::Duration};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Default)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub icon: &'static str,
    pub hint: &'static str,
    pub disabled: Option<String>,
    pub danger: bool,
    pub separator: bool,
    pub checked: bool,
}

impl MenuItem {
    pub fn new(id: &str, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ..Self::default()
        }
    }
    pub fn separator() -> Self {
        Self {
            separator: true,
            ..Self::default()
        }
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }
    pub fn hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    /// Disabled with the reason shown as a tooltip / secondary text.
    pub fn gate(mut self, gate: Result<(), String>) -> Self {
        self.disabled = gate.err();
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        if !enabled && self.disabled.is_none() {
            self.disabled = Some(String::new());
        }
        self
    }
}

type ToastAction = Box<dyn FnOnce(&Ctx)>;
/// Returns `(error, hint)` for the current text.
pub type Validator<'a> = Box<dyn Fn(&str) -> (Option<String>, String) + 'a>;

pub enum DialogEvent {
    Action(String, String),
    Edited(String),
    Option(bool),
    Row(usize),
    Crumb(String),
    Choice(String),
    Field(String, String),
    FieldToggle(String, bool),
    FieldAction(String, f32, f32),
}

#[derive(Default)]
pub struct Overlays {
    menu: Option<oneshot::Sender<Option<String>>>,
    menu_items: Vec<MenuItem>,
    dialogs: Option<Rc<VecModel<DialogSpec>>>,
    dialog_events: HashMap<i32, mpsc::UnboundedSender<DialogEvent>>,
    toasts: Option<Rc<VecModel<ToastEntry>>>,
    toast_actions: HashMap<i32, ToastAction>,
    next_id: i32,
}

pub struct ConfirmOptions {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub danger: bool,
    /// Optional checkbox; its final state is returned alongside the answer.
    pub option: Option<(String, bool)>,
}

pub struct PromptOptions<'a> {
    pub title: String,
    pub label: String,
    pub initial: String,
    pub placeholder: String,
    pub confirm: String,
    pub password: bool,
    /// Characters to preselect (0 selects everything), e.g. a name without its extension.
    pub select_to: usize,
    /// Returns `(error, hint)`; an error disables the confirm button.
    pub validate: Validator<'a>,
}

pub fn install(ctx: &Ctx) {
    let dialogs = Rc::new(VecModel::<DialogSpec>::default());
    let toasts = Rc::new(VecModel::<ToastEntry>::default());
    ctx.ui
        .global::<Dialogs>()
        .set_stack(ModelRc::from(dialogs.clone()));
    ctx.ui
        .global::<Toasts>()
        .set_items(ModelRc::from(toasts.clone()));
    {
        let mut overlays = ctx.overlays.borrow_mut();
        overlays.dialogs = Some(dialogs);
        overlays.toasts = Some(toasts);
    }
    let menu = ctx.ui.global::<Menu>();
    let weak = Rc::downgrade(ctx);
    menu.on_activated({
        let weak = weak.clone();
        move |id| {
            if let Some(ctx) = weak.upgrade() {
                ctx.finish_menu(Some(id.to_string()));
            }
        }
    });
    menu.on_dismissed({
        let weak = weak.clone();
        move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.finish_menu(None);
            }
        }
    });
    menu.on_move({
        let weak = weak.clone();
        move |delta| {
            if let Some(ctx) = weak.upgrade() {
                ctx.move_menu(delta);
            }
        }
    });
    menu.on_activate_current({
        let weak = weak.clone();
        move || {
            if let Some(ctx) = weak.upgrade() {
                let current = ctx.ui.global::<Menu>().get_current();
                let id = ctx
                    .overlays
                    .borrow()
                    .menu_items
                    .get(current.max(0) as usize)
                    .filter(|item| current >= 0 && !item.separator && item.disabled.is_none())
                    .map(|item| item.id.clone());
                if id.is_some() {
                    ctx.finish_menu(id);
                }
            }
        }
    });
    let send = |ctx: &Ctx, id: i32, event: DialogEvent| {
        if let Some(sender) = ctx.overlays.borrow().dialog_events.get(&id) {
            let _ = sender.send(event);
        }
    };
    let dialogs = ctx.ui.global::<Dialogs>();
    dialogs.on_action({
        let weak = weak.clone();
        move |id, action, value| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Action(action.into(), value.into()));
            }
        }
    });
    dialogs.on_edited({
        let weak = weak.clone();
        move |id, value| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Edited(value.into()));
            }
        }
    });
    dialogs.on_option_toggled({
        let weak = weak.clone();
        move |id, checked| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Option(checked));
            }
        }
    });
    dialogs.on_row_activated({
        let weak = weak.clone();
        move |id, row| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Row(row.max(0) as usize));
            }
        }
    });
    dialogs.on_crumb({
        let weak = weak.clone();
        move |id, crumb| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Crumb(crumb.into()));
            }
        }
    });
    dialogs.on_choice({
        let weak = weak.clone();
        move |id, choice| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Choice(choice.into()));
            }
        }
    });
    dialogs.on_field_edited({
        let weak = weak.clone();
        move |id, field, value| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::Field(field.into(), value.into()));
            }
        }
    });
    dialogs.on_field_toggled({
        let weak = weak.clone();
        move |id, field, checked| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::FieldToggle(field.into(), checked));
            }
        }
    });
    dialogs.on_field_action({
        let weak = weak.clone();
        move |id, field, x, y| {
            if let Some(ctx) = weak.upgrade() {
                send(&ctx, id, DialogEvent::FieldAction(field.into(), x, y));
            }
        }
    });
    let toasts = ctx.ui.global::<Toasts>();
    toasts.on_action({
        let weak = weak.clone();
        move |id| {
            if let Some(ctx) = weak.upgrade() {
                let action = ctx.overlays.borrow_mut().toast_actions.remove(&id);
                ctx.dismiss_toast(id);
                if let Some(action) = action {
                    action(&ctx);
                }
            }
        }
    });
    toasts.on_dismiss(move |id| {
        if let Some(ctx) = weak.upgrade() {
            ctx.dismiss_toast(id);
        }
    });
}

impl App {
    fn next_overlay_id(&self) -> i32 {
        let mut overlays = self.overlays.borrow_mut();
        overlays.next_id += 1;
        overlays.next_id
    }

    pub fn menu_entries(&self, items: &[MenuItem]) -> Vec<MenuEntry> {
        items
            .iter()
            .map(|item| MenuEntry {
                id: item.id.as_str().into(),
                label: item.label.as_str().into(),
                icon: if item.icon.is_empty() {
                    slint::Image::default()
                } else {
                    self.icon(item.icon)
                },
                hint: item.hint.into(),
                enabled: item.disabled.is_none(),
                danger: item.danger,
                separator: item.separator,
                checked: item.checked,
                reason: item.disabled.clone().unwrap_or_default().into(),
            })
            .collect()
    }

    /// Shows a menu at window coordinates and resolves with the chosen id.
    pub async fn menu(&self, x: f32, y: f32, title: &str, items: Vec<MenuItem>) -> Option<String> {
        let (sender, receiver) = oneshot::channel();
        let entries = self.menu_entries(&items);
        {
            let mut overlays = self.overlays.borrow_mut();
            // Replacing an open menu resolves the previous one as dismissed.
            overlays.menu = Some(sender);
            overlays.menu_items = items;
        }
        let menu = self.ui.global::<Menu>();
        menu.set_items(ModelRc::new(VecModel::from(entries)));
        menu.set_title(title.into());
        menu.set_x(x);
        menu.set_y(y);
        menu.set_current(-1);
        menu.set_open(true);
        receiver.await.ok().flatten()
    }

    pub fn menu_open(&self) -> bool {
        self.ui.global::<Menu>().get_open()
    }

    pub fn finish_menu(&self, choice: Option<String>) {
        self.ui.global::<Menu>().set_open(false);
        let sender = {
            let mut overlays = self.overlays.borrow_mut();
            overlays.menu_items.clear();
            overlays.menu.take()
        };
        if let Some(sender) = sender {
            let _ = sender.send(choice);
        }
    }

    fn move_menu(&self, delta: i32) {
        let overlays = self.overlays.borrow();
        let usable: Vec<usize> = overlays
            .menu_items
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.separator && item.disabled.is_none())
            .map(|(index, _)| index)
            .collect();
        if usable.is_empty() {
            return;
        }
        let menu = self.ui.global::<Menu>();
        let current = usable
            .iter()
            .position(|index| *index as i32 == menu.get_current());
        let next = match (current, delta) {
            (_, d) if d <= -1000 => 0,
            (_, d) if d >= 1000 => usable.len() - 1,
            (None, d) if d < 0 => usable.len() - 1,
            (None, _) => 0,
            (Some(at), d) => (at as i32 + d).rem_euclid(usable.len() as i32) as usize,
        };
        menu.set_current(usable[next] as i32);
    }

    /* ------------------------------ dialogs ------------------------------ */

    fn push_dialog(&self, mut spec: DialogSpec) -> (i32, mpsc::UnboundedReceiver<DialogEvent>) {
        let id = self.next_overlay_id();
        spec.id = id;
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut overlays = self.overlays.borrow_mut();
        overlays.dialog_events.insert(id, sender);
        if let Some(model) = &overlays.dialogs {
            model.push(spec);
        }
        (id, receiver)
    }

    pub(crate) fn update_dialog(&self, id: i32, change: impl FnOnce(&mut DialogSpec)) {
        let model = self.overlays.borrow().dialogs.clone();
        let Some(model) = model else { return };
        if let Some(row) = (0..model.row_count())
            .find(|row| model.row_data(*row).is_some_and(|spec| spec.id == id))
        {
            let mut spec = model.row_data(row).unwrap_or_default();
            change(&mut spec);
            model.set_row_data(row, spec);
        }
    }

    pub(crate) fn close_dialog(&self, id: i32) {
        let model = {
            let mut overlays = self.overlays.borrow_mut();
            overlays.dialog_events.remove(&id);
            overlays.dialogs.clone()
        };
        let Some(model) = model else { return };
        if let Some(row) = (0..model.row_count())
            .find(|row| model.row_data(*row).is_some_and(|spec| spec.id == id))
        {
            model.remove(row);
        }
    }

    pub fn dialog_open(&self) -> bool {
        self.overlays
            .borrow()
            .dialogs
            .as_ref()
            .is_some_and(|model| model.row_count() > 0)
    }

    /// Closes the top dialog as if it had been cancelled (Escape, session reset).
    pub fn cancel_top_dialog(&self) -> bool {
        let top = self.overlays.borrow().dialogs.as_ref().and_then(|model| {
            let count = model.row_count();
            (count > 0).then(|| model.row_data(count - 1)).flatten()
        });
        match top {
            Some(spec) if spec.dismissable && !spec.busy => {
                if let Some(sender) = self.overlays.borrow().dialog_events.get(&spec.id) {
                    let _ = sender.send(DialogEvent::Action("cancel".into(), String::new()));
                }
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    fn base_dialog(kind: &str, title: &str) -> DialogSpec {
        DialogSpec {
            kind: kind.into(),
            title: title.into(),
            cancel: tr("取消").into(),
            dismissable: true,
            confirm_enabled: true,
            ..DialogSpec::default()
        }
    }

    /// Returns `Some(option_checked)` when confirmed.
    pub async fn confirm(&self, options: ConfirmOptions) -> Option<bool> {
        let mut spec = Self::base_dialog("confirm", &options.title);
        spec.body = options.body.into();
        spec.confirm = options.confirm.into();
        spec.danger = options.danger;
        let mut checked = false;
        if let Some((label, initial)) = options.option {
            spec.option = label.into();
            spec.option_checked = initial;
            checked = initial;
        }
        let (id, mut events) = self.push_dialog(spec);
        let result = loop {
            match events.recv().await {
                Some(DialogEvent::Option(value)) => {
                    checked = value;
                    self.update_dialog(id, |spec| spec.option_checked = value);
                }
                Some(DialogEvent::Action(action, _)) if action == "confirm" => break Some(checked),
                Some(DialogEvent::Action(..)) | None => break None,
                Some(_) => {}
            }
        };
        self.close_dialog(id);
        result
    }

    pub async fn ask(&self, title: &str, body: &str, confirm: &str, danger: bool) -> bool {
        self.confirm(ConfirmOptions {
            title: title.into(),
            body: body.into(),
            confirm: confirm.into(),
            danger,
            option: None,
        })
        .await
        .is_some()
    }

    /// An acknowledgement with a single button.
    pub async fn notice(&self, title: &str, body: &str) {
        let mut spec = Self::base_dialog("confirm", title);
        spec.body = body.into();
        spec.confirm = tr("知道了").into();
        spec.cancel = SharedString::default();
        let (id, mut events) = self.push_dialog(spec);
        while let Some(event) = events.recv().await {
            if matches!(event, DialogEvent::Action(..)) {
                break;
            }
        }
        self.close_dialog(id);
    }

    pub async fn prompt(&self, options: PromptOptions<'_>) -> Option<String> {
        let mut spec = Self::base_dialog("prompt", &options.title);
        spec.label = options.label.into();
        spec.value = options.initial.as_str().into();
        spec.placeholder = options.placeholder.into();
        spec.confirm = options.confirm.into();
        spec.password = options.password;
        spec.select_to = options.select_to as i32;
        let (error, hint) = (options.validate)(&options.initial);
        // The initial value may be invalid (e.g. an unchanged name); do not
        // show an error before the user typed, only keep confirm disabled.
        spec.confirm_enabled = error.is_none();
        spec.hint = hint.into();
        let (id, mut events) = self.push_dialog(spec);
        let result = loop {
            match events.recv().await {
                Some(DialogEvent::Edited(text)) => {
                    let (error, hint) = (options.validate)(&text);
                    self.update_dialog(id, |spec| {
                        spec.confirm_enabled = error.is_none();
                        spec.error = error.unwrap_or_default().into();
                        spec.hint = hint.into();
                        spec.value = text.into();
                    });
                }
                Some(DialogEvent::Action(action, text)) if action == "confirm" => {
                    let (error, hint) = (options.validate)(&text);
                    match error {
                        None => break Some(text),
                        Some(error) => self.update_dialog(id, |spec| {
                            spec.confirm_enabled = false;
                            spec.error = error.into();
                            spec.hint = hint.into();
                        }),
                    }
                }
                Some(DialogEvent::Action(..)) | None => break None,
                Some(_) => {}
            }
        };
        self.close_dialog(id);
        result
    }

    /// Key/value sheet. Returns true when the optional `action` button was used.
    pub async fn details(
        &self,
        title: &str,
        rows: Vec<(String, String, bool)>,
        action: Option<String>,
    ) -> bool {
        let mut spec = Self::base_dialog("details", title);
        spec.details = ModelRc::new(VecModel::from(
            rows.into_iter()
                .map(|(key, value, mono)| KeyValue {
                    key: key.into(),
                    value: value.into(),
                    mono,
                })
                .collect::<Vec<_>>(),
        ));
        spec.cancel = tr("关闭").into();
        spec.confirm = action.unwrap_or_default().into();
        let (id, mut events) = self.push_dialog(spec);
        let result = loop {
            match events.recv().await {
                Some(DialogEvent::Action(action, _)) => break action == "confirm",
                None => break false,
                Some(_) => {}
            }
        };
        self.close_dialog(id);
        result
    }

    /// Single-choice list; resolves with the confirmed id.
    pub async fn choose(
        &self,
        title: &str,
        body: &str,
        confirm: &str,
        choices: Vec<(String, String, String)>,
        initial: &str,
    ) -> Option<String> {
        let mut selected = initial.to_owned();
        let build = |selected: &str| {
            ModelRc::new(VecModel::from(
                choices
                    .iter()
                    .map(|(id, label, detail)| Choice {
                        id: id.into(),
                        label: label.into(),
                        detail: detail.into(),
                        selected: id == selected,
                        enabled: true,
                    })
                    .collect::<Vec<_>>(),
            ))
        };
        let mut spec = Self::base_dialog("choice", title);
        spec.body = body.into();
        spec.confirm = confirm.into();
        spec.choices = build(&selected);
        spec.confirm_enabled = choices.iter().any(|(id, ..)| *id == selected);
        let (id, mut events) = self.push_dialog(spec);
        let result = loop {
            match events.recv().await {
                Some(DialogEvent::Choice(choice)) => {
                    selected = choice;
                    let model = build(&selected);
                    self.update_dialog(id, |spec| {
                        spec.choices = model;
                        spec.confirm_enabled = true;
                    });
                }
                Some(DialogEvent::Action(action, _)) if action == "confirm" => {
                    break Some(selected)
                }
                Some(DialogEvent::Action(..)) | None => break None,
                Some(_) => {}
            }
        };
        self.close_dialog(id);
        result
    }

    /// Remote folder chooser. Resolves with `(folder_id, display_path)`.
    pub async fn pick_folder(
        self: &Rc<Self>,
        title: &str,
        confirm: &str,
        start: Vec<(String, String)>,
        blocked: &[String],
    ) -> Option<(String, String)> {
        let mut trail = if start.is_empty() {
            vec![("root".to_owned(), tr("网盘"))]
        } else {
            start
        };
        let mut spec = Self::base_dialog("picker", title);
        spec.confirm = confirm.into();
        spec.alt = tr("新建文件夹").into();
        spec.phase = "loading".into();
        let (id, mut events) = self.push_dialog(spec);
        let session = self.session();
        let mut folders: Vec<RemoteEntry> = Vec::new();
        let mut reload = true;
        let result = loop {
            if reload {
                reload = false;
                let crumbs: Vec<Crumb> = trail
                    .iter()
                    .map(|(id, name)| Crumb {
                        id: id.into(),
                        name: name.into(),
                    })
                    .collect();
                let current = trail.last().map(|(id, _)| id.clone()).unwrap_or_default();
                let allowed = !blocked.contains(&current);
                self.update_dialog(id, |spec| {
                    spec.crumbs = ModelRc::new(VecModel::from(crumbs));
                    spec.phase = "loading".into();
                    spec.rows = ModelRc::default();
                    spec.confirm_enabled = false;
                    spec.hint = SharedString::default();
                });
                let listing = self
                    .call({
                        let current = current.clone();
                        move |service| async move {
                            let mut all = Vec::new();
                            let mut cursor: Option<String> = None;
                            // Folder pickers list a bounded number of pages.
                            for _ in 0..20 {
                                let page = service
                                    .files_list(&current, cursor.as_deref(), Some(500), &request())
                                    .await?;
                                all.extend(
                                    page.items
                                        .into_iter()
                                        .filter(|entry| entry.kind == EntryKind::Folder),
                                );
                                cursor = page.next_cursor;
                                if cursor.is_none() {
                                    break;
                                }
                            }
                            Ok::<_, ClientError>(all)
                        }
                    })
                    .await;
                if session != self.session() {
                    break None;
                }
                match listing {
                    Ok(mut list) => {
                        list.sort_by_key(|entry| entry.name.to_lowercase());
                        let rows: Vec<FileRow> = list
                            .iter()
                            .map(|entry| FileRow {
                                id: entry.id.as_str().into(),
                                name: entry.name.as_str().into(),
                                kind: crate::names::KIND_FOLDER,
                                folder: true,
                                ..FileRow::default()
                            })
                            .collect();
                        folders = list;
                        self.update_dialog(id, |spec| {
                            spec.rows = ModelRc::new(VecModel::from(rows));
                            spec.phase = "ready".into();
                            spec.confirm_enabled = allowed;
                            spec.hint = if allowed {
                                SharedString::default()
                            } else {
                                tr("不能移动或复制到自身").into()
                            };
                        });
                    }
                    Err(error) => {
                        folders.clear();
                        self.update_dialog(id, |spec| {
                            spec.phase = "error".into();
                            spec.error = error_text(&error).into();
                        });
                    }
                }
            }
            match events.recv().await {
                Some(DialogEvent::Row(row)) => {
                    if let Some(entry) = folders.get(row) {
                        trail.push((entry.id.clone(), entry.name.clone()));
                        reload = true;
                    }
                }
                Some(DialogEvent::Crumb(crumb)) => {
                    if let Some(at) = trail.iter().position(|(id, _)| *id == crumb) {
                        trail.truncate(at + 1);
                        reload = true;
                    }
                }
                Some(DialogEvent::Action(action, _)) if action == "alt" => {
                    let parent = trail.last().map(|(id, _)| id.clone()).unwrap_or_default();
                    let siblings: Vec<String> =
                        folders.iter().map(|entry| entry.name.clone()).collect();
                    if crate::actions::new_folder_in(self, &parent, siblings).await {
                        reload = true;
                    }
                }
                Some(DialogEvent::Action(action, _)) if action == "confirm" => {
                    let path = format!(
                        "/{}",
                        trail
                            .iter()
                            .skip(1)
                            .map(|(_, name)| name.as_str())
                            .collect::<Vec<_>>()
                            .join("/")
                    );
                    break trail.last().map(|(id, _)| (id.clone(), path));
                }
                Some(DialogEvent::Action(..)) | None => break None,
                Some(_) => {}
            }
        };
        self.close_dialog(id);
        result
    }

    /* ------------------------------- toasts ------------------------------ */

    pub fn toast(self: &Rc<Self>, text: impl Into<String>, tone: &str) {
        self.toast_with(text, tone, None);
    }

    pub fn toast_error(self: &Rc<Self>, error: &ClientError) {
        if error.code != ErrorCode::Cancelled {
            self.toast(error_text(error), "error");
        }
    }

    pub fn toast_action(
        self: &Rc<Self>,
        text: impl Into<String>,
        tone: &str,
        label: impl Into<String>,
        action: impl FnOnce(&Ctx) + 'static,
    ) {
        self.toast_with(text, tone, Some((label.into(), Box::new(action))));
    }

    fn toast_with(
        self: &Rc<Self>,
        text: impl Into<String>,
        tone: &str,
        action: Option<(String, ToastAction)>,
    ) {
        let id = self.next_overlay_id();
        let lifetime = Duration::from_millis(match (tone, action.is_some()) {
            ("error", _) => 9000,
            (_, true) => 8000,
            _ => 4200,
        });
        let entry = ToastEntry {
            id,
            text: text.into().into(),
            tone: tone.into(),
            action: action
                .as_ref()
                .map(|(label, _)| label.as_str().into())
                .unwrap_or_default(),
        };
        let model = {
            let mut overlays = self.overlays.borrow_mut();
            if let Some((_, run)) = action {
                overlays.toast_actions.insert(id, run);
            }
            overlays.toasts.clone()
        };
        let Some(model) = model else { return };
        while model.row_count() >= 3 {
            if let Some(oldest) = model.row_data(0) {
                self.overlays.borrow_mut().toast_actions.remove(&oldest.id);
            }
            model.remove(0);
        }
        model.push(entry);
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(lifetime, move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.dismiss_toast(id);
            }
        });
    }

    pub fn dismiss_toast(&self, id: i32) {
        let model = {
            let mut overlays = self.overlays.borrow_mut();
            overlays.toast_actions.remove(&id);
            overlays.toasts.clone()
        };
        let Some(model) = model else { return };
        if let Some(row) = (0..model.row_count())
            .find(|row| model.row_data(*row).is_some_and(|toast| toast.id == id))
        {
            model.remove(row);
        }
    }
}

/* -------------------------------- forms -------------------------------- */

#[derive(Clone, Default)]
pub struct Field {
    pub id: &'static str,
    pub kind: &'static str,
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub detail: String,
    pub error: String,
    pub checked: bool,
    pub disabled: bool,
    pub tone: &'static str,
}

impl Field {
    fn of(id: &'static str, kind: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            ..Self::default()
        }
    }
    pub fn text(id: &'static str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::of(id, "text", label)
        }
    }
    pub fn password(id: &'static str, label: impl Into<String>) -> Self {
        Self::of(id, "password", label)
    }
    pub fn multiline(id: &'static str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::of(id, "multiline", label)
        }
    }
    pub fn toggle(id: &'static str, label: impl Into<String>, checked: bool) -> Self {
        Self {
            checked,
            ..Self::of(id, "toggle", label)
        }
    }
    pub fn select(id: &'static str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::of(id, "select", label)
        }
    }
    pub fn note(id: &'static str, text: impl Into<String>, tone: &'static str) -> Self {
        Self {
            tone,
            ..Self::of(id, "note", text)
        }
    }
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }
    pub fn detail(mut self, text: impl Into<String>) -> Self {
        self.detail = text.into();
        self
    }
    pub fn focus(mut self) -> Self {
        self.tone = "focus";
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

pub enum FormEvent {
    /// A text field changed (already stored in the form).
    Edited,
    Toggled,
    /// A select field was opened; show a menu at the given point.
    Select(String, f32, f32),
    Submit,
    Alt,
    Cancel,
}

/// A live form dialog. Dropping it without `close` leaves the dialog open,
/// so every exit path calls `close`.
pub struct Form {
    ctx: Ctx,
    id: i32,
    events: mpsc::UnboundedReceiver<DialogEvent>,
    pub fields: Vec<Field>,
}

impl Form {
    fn push(&self) {
        let rows: Vec<FormField> = self
            .fields
            .iter()
            .map(|field| FormField {
                id: field.id.into(),
                kind: field.kind.into(),
                label: field.label.as_str().into(),
                value: field.value.as_str().into(),
                placeholder: field.placeholder.as_str().into(),
                detail: field.detail.as_str().into(),
                error: field.error.as_str().into(),
                checked: field.checked,
                enabled: !field.disabled,
                tone: field.tone.into(),
            })
            .collect();
        self.ctx.update_dialog(self.id, |spec| {
            spec.fields = ModelRc::new(VecModel::from(rows))
        });
    }

    pub async fn next(&mut self) -> FormEvent {
        loop {
            match self.events.recv().await {
                Some(DialogEvent::Field(id, value)) => {
                    // Typing only updates local state: re-pushing the model
                    // would reset the caret of the input being edited.
                    if let Some(field) = self.fields.iter_mut().find(|field| field.id == id) {
                        field.value = value;
                        if !field.error.is_empty() {
                            field.error.clear();
                            self.push();
                        }
                    }
                    return FormEvent::Edited;
                }
                Some(DialogEvent::FieldToggle(id, checked)) => {
                    if let Some(field) = self.fields.iter_mut().find(|field| field.id == id) {
                        field.checked = checked;
                    }
                    self.push();
                    return FormEvent::Toggled;
                }
                Some(DialogEvent::FieldAction(id, x, y)) => return FormEvent::Select(id, x, y),
                Some(DialogEvent::Action(action, _)) if action == "confirm" => {
                    return FormEvent::Submit
                }
                Some(DialogEvent::Action(action, _)) if action == "alt" => return FormEvent::Alt,
                Some(DialogEvent::Action(..)) | None => return FormEvent::Cancel,
                Some(_) => {}
            }
        }
    }

    pub fn value(&self, id: &str) -> String {
        self.fields
            .iter()
            .find(|field| field.id == id)
            .map(|field| field.value.clone())
            .unwrap_or_default()
    }

    pub fn checked(&self, id: &str) -> bool {
        self.fields
            .iter()
            .any(|field| field.id == id && field.checked)
    }

    /// Changes one field and refreshes the dialog.
    pub fn set(&mut self, id: &str, change: impl FnOnce(&mut Field)) {
        if let Some(field) = self.fields.iter_mut().find(|field| field.id == id) {
            change(field);
        }
        self.push();
    }

    pub fn set_busy(&self, busy: bool) {
        self.ctx.update_dialog(self.id, |spec| spec.busy = busy);
    }

    pub fn set_error(&self, message: impl Into<String>) {
        let message = message.into();
        self.ctx
            .update_dialog(self.id, |spec| spec.error = message.into());
    }

    pub fn set_confirm(&self, label: &str, enabled: bool) {
        self.ctx.update_dialog(self.id, |spec| {
            spec.confirm = label.into();
            spec.confirm_enabled = enabled;
        });
    }

    pub fn set_text(&self, title: &str, body: &str) {
        self.ctx.update_dialog(self.id, |spec| {
            spec.title = title.into();
            spec.body = body.into();
        });
    }

    pub fn close(self) {
        self.ctx.close_dialog(self.id);
    }
}

impl App {
    pub fn form(
        self: &Rc<Self>,
        title: &str,
        body: &str,
        confirm: &str,
        alt: &str,
        fields: Vec<Field>,
    ) -> Form {
        let mut spec = Self::base_dialog("form", title);
        spec.body = body.into();
        spec.confirm = confirm.into();
        spec.alt = alt.into();
        spec.width = 480.0;
        let (id, events) = self.push_dialog(spec);
        let form = Form {
            ctx: self.clone(),
            id,
            events,
            fields,
        };
        form.push();
        form
    }
}
