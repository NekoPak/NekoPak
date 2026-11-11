//! Translation of developer-authored copy. Simplified Chinese is the source
//! language; the catalogs are generated from the web UI's locale modules so
//! both front ends ship identical wording.
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
}

pub const LANGUAGES: [(&str, &str); 6] = [
    ("system", "跟随系统"),
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("en", "English"),
    ("ja-JP", "日本語"),
    ("ko-KR", "한국어"),
];

static CURRENT: RwLock<Locale> = RwLock::new(Locale::ZhCn);

type Catalog = HashMap<String, String>;

fn catalog(locale: Locale) -> Option<&'static Catalog> {
    static CATALOGS: [OnceLock<Catalog>; 4] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let (slot, key, shared) = match locale {
        Locale::ZhCn => return None,
        Locale::En => (0, "en", include_str!("../locales/en.json")),
        Locale::ZhTw => (1, "zh-TW", include_str!("../locales/zh-TW.json")),
        Locale::Ja => (2, "ja", include_str!("../locales/ja.json")),
        Locale::Ko => (3, "ko", include_str!("../locales/ko.json")),
    };
    Some(CATALOGS[slot].get_or_init(|| {
        let mut table: Catalog = serde_json::from_str(shared).unwrap_or_default();
        // Copy that only exists in the native desktop UI.
        let native: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(include_str!("../locales/native.json")).unwrap_or_default();
        for (source, translations) in native {
            if let Some(text) = translations.get(key) {
                table.insert(source, text.clone());
            }
        }
        table
    }))
}

/// Maps an OS or preference tag (`zh_CN.UTF-8`, `zh-Hant-TW`, `ja`) to a shipped locale.
pub fn resolve(tag: &str) -> Locale {
    let tag = tag.replace('_', "-").to_ascii_lowercase();
    if tag.starts_with("zh") {
        if ["hant", "-tw", "-hk", "-mo"]
            .iter()
            .any(|part| tag.contains(part))
        {
            Locale::ZhTw
        } else {
            Locale::ZhCn
        }
    } else if tag.starts_with("ja") {
        Locale::Ja
    } else if tag.starts_with("ko") {
        Locale::Ko
    } else {
        Locale::En
    }
}

pub fn system_tag() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
        let mut buffer = [0u16; 85];
        let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
        if length > 1 {
            return String::from_utf16_lossy(&buffer[..length as usize - 1]);
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .unwrap_or_else(|| "en".into())
}

/// Applies a stored preference (`system` or a locale tag).
pub fn apply_preference(preference: &str) {
    let locale = if preference == "system" {
        resolve(&system_tag())
    } else {
        resolve(preference)
    };
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = locale;
}

pub fn locale() -> Locale {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub fn tr(source: &str) -> String {
    catalog(locale())
        .and_then(|table| table.get(source))
        .cloned()
        .unwrap_or_else(|| source.to_owned())
}

/// Translates the template first, then inserts `{0}`, `{1}`… values verbatim.
pub fn tx(source: &str, values: &[&str]) -> String {
    let mut text = tr(source);
    for (index, value) in values.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), value);
    }
    text
}

#[cfg(test)]
pub fn has_translation(locale: Locale, source: &str) -> bool {
    catalog(locale).is_some_and(|table| table.contains_key(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_system_tags_like_the_web_ui() {
        assert_eq!(resolve("zh_CN.UTF-8"), Locale::ZhCn);
        assert_eq!(resolve("zh-Hant-HK"), Locale::ZhTw);
        assert_eq!(resolve("zh-TW"), Locale::ZhTw);
        assert_eq!(resolve("ja-JP"), Locale::Ja);
        assert_eq!(resolve("ko"), Locale::Ko);
        assert_eq!(resolve("de-DE"), Locale::En);
    }

    /// Every developer-authored string used by the native UI must be
    /// translated into all shipped languages.
    #[test]
    fn native_ui_copy_is_translated_in_every_locale() {
        fn literals(text: &str, marker: &str, out: &mut Vec<String>) {
            let mut rest = text;
            while let Some(at) = rest.find(marker) {
                rest = &rest[at + marker.len()..];
                let mut value = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some('n') => value.push('\n'),
                            Some(other) => value.push(other),
                            None => break,
                        },
                        '"' => break,
                        other => value.push(other),
                    }
                }
                out.push(value);
            }
        }
        fn walk(directory: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(directory).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("rs" | "slint")
                ) {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for marker in ["tr(\"", "tx(\"", "Tr.t(\"", "Tr.x(\"", "Tr.x2(\""] {
                        literals(&text, marker, out);
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources = Vec::new();
        walk(&root.join("src"), &mut sources);
        walk(&root.join("ui"), &mut sources);
        sources.sort();
        sources.dedup();
        // Only copy that contains CJK needs translating; symbols and names do not.
        sources.retain(|text| text.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)));
        let mut missing = Vec::new();
        for (locale, name) in [
            (Locale::En, "en"),
            (Locale::ZhTw, "zh-TW"),
            (Locale::Ja, "ja"),
            (Locale::Ko, "ko"),
        ] {
            for source in &sources {
                if !has_translation(locale, source) {
                    missing.push(format!("{name}\t{source}"));
                }
            }
        }
        if let Ok(path) = std::env::var("NEKOPAK_WRITE_MISSING") {
            std::fs::write(path, missing.join("\n")).unwrap();
        }
        assert!(
            missing.is_empty(),
            "{} untranslated strings, e.g. {:?}",
            missing.len(),
            &missing[..missing.len().min(8)]
        );
    }

    #[test]
    fn interpolation_happens_after_translation() {
        assert_eq!(tx("{0} 项", &["3"]), "3 项");
        assert_eq!(tx("a {0} b {1}", &["{1}", "x"]), "a x b x");
    }
}
