//! Display formatting. Byte counts stay in integer math so values above 2^53 keep precision.
use crate::i18n::{locale, tr, tx, Locale};
use chrono::{DateTime, Local, Utc};

const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

/// Windows Explorer convention: base 1024, three significant figures.
pub fn bytes(value: u64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    let mut unit = 0;
    let mut divisor = 1u128;
    while unit < UNITS.len() - 1 && value as u128 >= divisor * 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let hundredths = (value as u128 * 100 / divisor) as f64 / 100.0;
    let digits = if hundredths >= 100.0 {
        0
    } else if hundredths >= 10.0 {
        1
    } else {
        2
    };
    format!("{hundredths:.digits$} {}", UNITS[unit])
}

pub fn bytes_opt(value: Option<u64>) -> String {
    value.map(bytes).unwrap_or_else(|| "—".into())
}

pub fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn bytes_exact(value: u64) -> String {
    tx("{0} 字节", &[&group_digits(value)])
}

pub fn speed(bytes_per_second: Option<f64>) -> String {
    match bytes_per_second {
        Some(value) if value.is_finite() && value >= 0.0 => format!("{}/s", bytes(value.round() as u64)),
        _ => tr("未提供"),
    }
}

/// One decimal place, clamped to 100; `None` when the total is unknown or zero.
pub fn percent(done: u64, total: Option<u64>) -> Option<f32> {
    let total = total.filter(|total| *total > 0)?;
    Some((done.min(total) as u128 * 1000 / total as u128) as f32 / 10.0)
}

pub fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".into();
    }
    let s = seconds.round() as u64;
    let units: [&str; 4] = match locale() {
        Locale::En => ["s", "min", "h", "d"],
        Locale::Ja => ["秒", "分", "時間", "日"],
        Locale::Ko => ["초", "분", "시간", "일"],
        Locale::ZhTw => ["秒", "分", "小時", "天"],
        Locale::ZhCn => ["秒", "分", "小时", "天"],
    };
    let m = s / 60;
    let h = m / 60;
    if s < 60 {
        format!("{s} {}", units[0])
    } else if m < 60 {
        format!("{m} {} {} {}", units[1], s % 60, units[0])
    } else if h < 48 {
        format!("{h} {} {} {}", units[2], m % 60, units[1])
    } else {
        format!("{} {} {} {}", h / 24, units[3], h % 24, units[2])
    }
}

pub fn eta(done: u64, total: Option<u64>, bytes_per_second: Option<f64>) -> Option<String> {
    let total = total?;
    let rate = bytes_per_second.filter(|rate| *rate > 0.0)?;
    (done < total).then(|| duration((total - done) as f64 / rate))
}

fn parse(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso).ok().map(|value| value.with_timezone(&Utc))
}

pub fn date_time(iso: Option<&str>) -> String {
    let Some(value) = iso.and_then(parse) else {
        return "—".into();
    };
    let local = value.with_timezone(&Local);
    let pattern = match locale() {
        Locale::En => "%m/%d/%Y, %H:%M",
        Locale::Ko => "%Y. %m. %d. %H:%M",
        _ => "%Y/%m/%d %H:%M",
    };
    local.format(pattern).to_string()
}

pub fn relative(iso: Option<&str>) -> String {
    let Some(value) = iso.and_then(parse) else {
        return "—".into();
    };
    let seconds = (Utc::now() - value).num_seconds();
    if seconds < 0 || seconds >= 86_400 {
        return date_time(iso);
    }
    if seconds < 45 {
        return tr("刚刚");
    }
    let (count, hours) = if seconds < 3600 {
        (((seconds as f64 / 60.0).round() as i64).max(1), false)
    } else {
        ((seconds as f64 / 3600.0).round() as i64, true)
    };
    match (locale(), hours) {
        (Locale::ZhCn, false) => format!("{count} 分钟前"),
        (Locale::ZhCn, true) => format!("{count} 小时前"),
        (Locale::ZhTw, false) => format!("{count} 分鐘前"),
        (Locale::ZhTw, true) => format!("{count} 小時前"),
        (Locale::Ja, false) => format!("{count} 分前"),
        (Locale::Ja, true) => format!("{count} 時間前"),
        (Locale::Ko, false) => format!("{count}분 전"),
        (Locale::Ko, true) => format!("{count}시간 전"),
        (Locale::En, false) => format!("{count} minute{} ago", if count == 1 { "" } else { "s" }),
        (Locale::En, true) => format!("{count} hour{} ago", if count == 1 { "" } else { "s" }),
    }
}

/// `01:02:03.4` style media position.
pub fn media_time(milliseconds: u64) -> String {
    let total = milliseconds / 1000;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_match_the_web_formatter() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.00 KB");
        assert_eq!(bytes(2 * 1024 * 1024), "2.00 MB");
        assert_eq!(bytes(12 * 1024 * 1024 + 10_000), "12.0 MB");
        assert_eq!(bytes(220 * 1024 * 1024), "220 MB");
        assert_eq!(bytes(u64::MAX), "16.0 EB");
        assert_eq!(group_digits(9_007_199_254_740_993), "9,007,199,254,740,993");
    }

    #[test]
    fn percent_is_clamped_and_never_invented() {
        assert_eq!(percent(50, Some(200)), Some(25.0));
        assert_eq!(percent(300, Some(200)), Some(100.0));
        assert_eq!(percent(1, Some(0)), None);
        assert_eq!(percent(1, None), None);
        assert_eq!(eta(10, Some(10), Some(5.0)), None);
        assert_eq!(eta(0, Some(100), None), None);
    }

    #[test]
    fn durations_roll_over_units() {
        assert_eq!(duration(59.4), "59 秒");
        assert_eq!(duration(61.0), "1 分 1 秒");
        assert_eq!(duration(3661.0), "1 小时 1 分");
        assert_eq!(duration(49.0 * 3600.0), "2 天 1 小时");
        assert_eq!(media_time(3_723_400), "1:02:03");
    }
}
