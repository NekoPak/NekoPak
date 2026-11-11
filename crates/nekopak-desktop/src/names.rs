//! File-name helpers: display kinds and validation of new remote names.
use crate::i18n::{tr, tx};
use nekopak_domain::EntryKind;

pub const KIND_FOLDER: i32 = 0;
pub const KIND_VIDEO: i32 = 1;
pub const KIND_IMAGE: i32 = 2;
pub const KIND_AUDIO: i32 = 3;
pub const KIND_DOC: i32 = 4;
pub const KIND_ARCHIVE: i32 = 5;
pub const KIND_CODE: i32 = 6;
pub const KIND_OTHER: i32 = 7;

/// Kinds in the order offered by the type filter.
pub const FILTER_KINDS: [i32; 8] = [
    KIND_FOLDER,
    KIND_VIDEO,
    KIND_AUDIO,
    KIND_IMAGE,
    KIND_DOC,
    KIND_ARCHIVE,
    KIND_CODE,
    KIND_OTHER,
];

/// Splits a visible extension off; dotfiles and odd endings keep their whole name.
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => {
            let extension = &name[dot..];
            if extension.chars().count() > 12 || extension.chars().any(char::is_whitespace) {
                (name, "")
            } else {
                (&name[..dot], extension)
            }
        }
        _ => (name, ""),
    }
}

pub fn file_kind(name: &str, kind: EntryKind, mime: Option<&str>) -> i32 {
    if kind == EntryKind::Folder {
        return KIND_FOLDER;
    }
    if let Some(mime) = mime {
        if mime.starts_with("video/") {
            return KIND_VIDEO;
        }
        if mime.starts_with("audio/") {
            return KIND_AUDIO;
        }
        if mime.starts_with("image/") {
            return KIND_IMAGE;
        }
    }
    let extension = split_name(name)
        .1
        .trim_start_matches('.')
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "ts" | "flv" | "wmv" => KIND_VIDEO,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" => KIND_AUDIO,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" | "svg" | "avif" => KIND_IMAGE,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf" | "odt"
        | "csv" | "epub" => KIND_DOC,
        "zip" | "7z" | "rar" | "tar" | "gz" | "zst" | "xz" | "iso" | "img" | "dmg" | "bz2" => {
            KIND_ARCHIVE
        }
        "rs" | "js" | "json" | "toml" | "yaml" | "yml" | "py" | "c" | "h" | "cpp" | "kt" | "go"
        | "sh" | "fig" => KIND_CODE,
        _ => KIND_OTHER,
    }
}

pub fn kind_label(kind: i32) -> String {
    tr(match kind {
        KIND_FOLDER => "文件夹",
        KIND_VIDEO => "视频",
        KIND_AUDIO => "音频",
        KIND_IMAGE => "图片",
        KIND_DOC => "文档",
        KIND_ARCHIVE => "压缩包/镜像",
        KIND_CODE => "代码/设计",
        _ => "其他",
    })
}

pub fn is_archive_browsable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".zip", ".rar", ".7z"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn reserved_on_windows(name: &str) -> bool {
    let stem = name
        .trim()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.chars().count() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem
                .chars()
                .nth(3)
                .is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)))
}

/// Issues that only matter when the remote name is materialised on Windows.
pub fn windows_issues(name: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if reserved_on_windows(name) {
        issues.push(tr("Windows 保留设备名"));
    }
    if name.chars().any(|c| "<>:\"\\|?*".contains(c)) {
        issues.push(tr("含 Windows 不允许的字符 < > : \" \\ | ? *"));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        issues.push(tr("以空格或句点结尾"));
    }
    issues
}

#[derive(Default, Debug, PartialEq)]
pub struct NameCheck {
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// Hard errors block submit; warnings explain how a later Windows download escapes the name.
pub fn validate_name(name: &str, siblings: &[String], current: Option<&str>) -> NameCheck {
    let fail = |message: &str| NameCheck {
        error: Some(tr(message)),
        warnings: Vec::new(),
    };
    if name.trim().is_empty() {
        return fail("名称不能为空");
    }
    if name == "." || name == ".." {
        return fail("不能使用 . 或 ..");
    }
    if name.contains('/') {
        return fail("名称不能包含 /");
    }
    if name.chars().any(char::is_control) {
        return fail("名称不能包含控制字符");
    }
    if name.len() > 255 {
        return fail("名称超过 255 字节（UTF-8）");
    }
    if current == Some(name) {
        return fail("新名称与原名称相同");
    }
    let mut warnings = Vec::new();
    for sibling in siblings {
        if Some(sibling.as_str()) == current {
            continue;
        }
        if sibling == name {
            return fail("此文件夹中已有同名项");
        }
        if sibling.to_lowercase() == name.to_lowercase() {
            warnings.push(tx(
                "与“{0}”仅大小写不同，Windows 本地会视为同名",
                &[sibling],
            ));
        }
    }
    for issue in windows_issues(name) {
        warnings.push(tx("{0}，下载到 Windows 时将被转义", &[&issue]));
    }
    NameCheck {
        error: None,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_extension_then_mime() {
        assert_eq!(file_kind("a.MKV", EntryKind::File, None), KIND_VIDEO);
        assert_eq!(
            file_kind("a.bin", EntryKind::File, Some("image/png")),
            KIND_IMAGE
        );
        assert_eq!(file_kind("a.zip", EntryKind::Folder, None), KIND_FOLDER);
        assert_eq!(file_kind(".gitignore", EntryKind::File, None), KIND_OTHER);
        assert_eq!(split_name("archive.tar.gz"), ("archive.tar", ".gz"));
        assert_eq!(split_name("name."), ("name.", ""));
    }

    #[test]
    fn validation_blocks_collisions_and_warns_about_windows() {
        let siblings = vec!["Notes.txt".to_owned(), "old".to_owned()];
        assert!(validate_name("  ", &siblings, None).error.is_some());
        assert!(validate_name("a/b", &siblings, None).error.is_some());
        assert!(validate_name("Notes.txt", &siblings, None).error.is_some());
        assert!(validate_name("old", &siblings, Some("old")).error.is_some());
        let check = validate_name("notes.txt", &siblings, None);
        assert!(check.error.is_none());
        assert_eq!(check.warnings.len(), 1);
        assert!(!validate_name("CON.txt", &[], None).warnings.is_empty());
        assert!(!validate_name("a:b", &[], None).warnings.is_empty());
        assert!(validate_name("com5", &[], None).warnings.len() == 1);
        assert!(validate_name("comet", &[], None).warnings.is_empty());
    }
}
