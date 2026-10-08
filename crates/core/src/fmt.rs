/// Human readable size, e.g. `1.4 GB` (binary units, like Windows).
pub fn size(b: u64) -> String {
    const U: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    if b < 1024 {
        return format!("{b} bytes");
    }
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", U[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", U[i])
    } else {
        format!("{v:.2} {}", U[i])
    }
}

/// `YYYY-MM-DD HH:MM` from a Unix timestamp (UTC), or empty if unknown.
pub fn date(t: u32) -> String {
    if t == 0 {
        return String::new();
    }
    let days = (t / 86400) as i64;
    let secs = t % 86400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}

pub fn ext(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => &name[i + 1..],
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::size(10), "10 bytes");
        assert_eq!(super::size(1536), "1.50 KB");
        assert_eq!(super::size(5 * 1024 * 1024 * 1024), "5.00 GB");
    }
    #[test]
    fn dates() {
        assert_eq!(super::date(86400 * 365), "1971-01-01 00:00");
        assert_eq!(super::date(1_700_000_000), "2023-11-14 22:13");
    }
}
