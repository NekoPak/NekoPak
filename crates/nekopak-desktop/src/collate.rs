//! Name ordering. Windows uses the system's linguistic comparison (pinyin for
//! Chinese locales, digits as numbers) so lists match File Explorer. Other
//! platforms fall back to a case-insensitive natural ordering.
use std::cmp::Ordering;

/// Precomputed key; build once per entry and compare many times.
pub struct SortKey {
    #[cfg(windows)]
    wide: Vec<u16>,
    #[cfg(not(windows))]
    folded: String,
}

impl SortKey {
    pub fn new(text: &str) -> Self {
        #[cfg(windows)]
        {
            Self {
                wide: text.encode_utf16().collect(),
            }
        }
        #[cfg(not(windows))]
        {
            Self {
                folded: text.to_lowercase(),
            }
        }
    }

    pub fn compare(&self, other: &Self) -> Ordering {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Globalization::{
                CompareStringEx, LINGUISTIC_IGNORECASE, SORT_DIGITSASNUMBERS,
            };
            // CSTR_LESS_THAN = 1, CSTR_EQUAL = 2, CSTR_GREATER_THAN = 3; 0 means failure.
            let result = unsafe {
                CompareStringEx(
                    std::ptr::null(),
                    LINGUISTIC_IGNORECASE | SORT_DIGITSASNUMBERS,
                    self.wide.as_ptr(),
                    self.wide.len() as i32,
                    other.wide.as_ptr(),
                    other.wide.len() as i32,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                )
            };
            match result {
                1 => Ordering::Less,
                2 => Ordering::Equal,
                3 => Ordering::Greater,
                _ => self.wide.cmp(&other.wide),
            }
        }
        #[cfg(not(windows))]
        {
            natural(&self.folded, &other.folded)
        }
    }
}

/// Compares runs of ASCII digits by value and everything else by code point.
#[cfg_attr(windows, allow(dead_code))]
pub fn natural(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let start_i = i;
            let start_j = j;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let x = trim_zeros(&a[start_i..i]);
            let y = trim_zeros(&b[start_j..j]);
            let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

#[cfg_attr(windows, allow(dead_code))]
fn trim_zeros(digits: &[u8]) -> &[u8] {
    let first = digits
        .iter()
        .position(|d| *d != b'0')
        .unwrap_or(digits.len().saturating_sub(1));
    &digits[first..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_sort_by_value() {
        let mut names = vec![
            "第10讲.mp4",
            "第2讲.mp4",
            "第1讲.mp4",
            "a100",
            "a20",
            "a020",
            "A3",
        ];
        names.sort_by(|a, b| natural(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(
            names,
            [
                "a3",
                "a20",
                "a020",
                "a100",
                "第1讲.mp4",
                "第2讲.mp4",
                "第10讲.mp4"
            ]
            .map(|n| {
                if n == "a3" {
                    "A3"
                } else {
                    n
                }
            })
        );
        assert_eq!(natural("file", "file2"), Ordering::Less);
        assert_eq!(natural("9", "10"), Ordering::Less);
        assert_eq!(natural("007", "7"), Ordering::Equal);
    }
}
