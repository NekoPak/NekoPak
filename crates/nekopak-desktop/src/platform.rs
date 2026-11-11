//! Operating-system integration: native file dialogs, clipboard, opening
//! folders and links.
use crate::app::{App, Ctx};

/// Native folder chooser; `None` when cancelled.
pub async fn pick_folder(ctx: &Ctx, title: &str, start: &str) -> Option<String> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if !start.is_empty() && std::path::Path::new(start).is_dir() {
        dialog = dialog.set_directory(start);
    }
    let _ = ctx;
    let folder = dialog.pick_folder().await?;
    folder.path().to_str().map(str::to_owned)
}

pub async fn pick_files(ctx: &Ctx, title: &str) -> Vec<String> {
    let _ = ctx;
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .pick_files()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|file| file.path().to_str().map(str::to_owned))
        .collect()
}

pub async fn pick_file(
    ctx: &Ctx,
    title: &str,
    filter: (&str, &[&str]),
) -> Option<std::path::PathBuf> {
    let _ = ctx;
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(filter.0, filter.1)
        .pick_file()
        .await
        .map(|file| file.path().to_owned())
}

/// Slint exposes the clipboard through text inputs; a hidden one bridges it.
pub fn copy_text(ctx: &Ctx, text: &str) {
    ctx.ui.set_clipboard_text(text.into());
    ctx.ui.invoke_clipboard_copy();
    ctx.ui.set_clipboard_text(Default::default());
}

/// Opens an HTTPS link in the default browser. Other schemes are refused.
pub fn open_url(url: &str) -> bool {
    let safe = url.starts_with("https://")
        && !url[8..].split('/').next().unwrap_or_default().contains('@');
    safe && open::that_detached(url).is_ok()
}

pub fn open_path(path: &std::path::Path) -> bool {
    open::that_detached(path).is_ok()
}

impl App {
    pub fn open_link(self: &std::rc::Rc<Self>, url: &str) {
        if !open_url(url) {
            copy_text(self, url);
            self.toast(crate::i18n::tr("无法打开浏览器，链接已复制"), "warning");
        }
    }
}
