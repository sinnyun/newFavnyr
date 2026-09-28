// ----- Formatting ----------------------------------------------------------

/// Wording of a formatted size, supplied by the CALLER.
///
/// This crate holds no translations: the algorithm lives here, the vocabulary
/// lives with the interface. That also keeps the two locale-dependent pieces
/// together — a language that writes `Ko` also writes `1,0`, and separating
/// them is how the decimal mark ended up applied to French alone while
/// Spanish, German and Italian kept an English point.
#[derive(Debug, Clone, Copy)]
pub struct SizeUnits<'a> {
    /// Units by increasing power of 1024: byte, kilo, mega, giga, tera.
    pub steps: [&'a str; 5],
    /// Mark between the integer and the decimal part.
    pub decimal: char,
}

impl SizeUnits<'_> {
    /// Renders `value` with `decimals` places, using the locale's mark.
    fn number(&self, value: f64, decimals: usize) -> String {
        let s = format!("{value:.decimals$}");
        if self.decimal == '.' {
            s
        } else {
            s.replace('.', &self.decimal.to_string())
        }
    }
}

/// Formats a size in bytes with the most appropriate unit.
/// Base-1024 convention (binary), one decimal beyond KB.
pub fn format_size(bytes: u64, units: SizeUnits<'_>) -> String {
    if bytes < 1024 {
        return format!("{bytes} {}", units.steps[0]);
    }
    let mut value = bytes as f64;
    let mut idx = 0usize;
    while value >= 1024.0 && idx + 1 < units.steps.len() {
        value /= 1024.0;
        idx += 1;
    }
    // One decimal beyond the byte step.
    format!("{} {}", units.number(value, 1), units.steps[idx])
}

/// How comfortable the remaining space on a volume is: `0` roomy, `1` low,
/// `2` critical. Used to colour the sidebar capacity badge.
///
/// A share alone is not enough: "90% used" leaves 400 GB on a 4 TB archive,
/// which is no problem at all. So each level pairs a ratio with a ceiling above
/// which no percentage justifies an alarm:
///
///   - the ratios (10% / 20%) are the classic filesystem health marks: ext4
///     reserves 5% for root, and both ext4 and NTFS lose allocation locality
///     below a tenth free, as do SSDs their spare area;
///   - the ceilings (128 / 256 GiB) keep a large archive quiet: 700 GB free is
///     not a warning, whatever fraction of the disk it represents.
///
/// The ratio is what makes this scale from a 3 GB USB stick to an 8 TB archive.
/// An earlier version also had absolute floors (< 4 / 16 GiB free = alarm), but
/// a small removable drive can never hold that much: a 3 GB stick two-thirds
/// EMPTY still has under 4 GiB free and lit up "critical" (red). The ratio
/// already flags a genuinely full small drive (a few % free), so the floors only
/// mis-fired — always on tiny drives, and redundant on large ones.
pub fn free_space_level(free: u64, total: u64) -> i32 {
    const GIB: u64 = 1024 * 1024 * 1024;
    if total == 0 {
        return 0;
    }
    if free < total / 10 && free < 128 * GIB {
        return 2;
    }
    if free < total / 5 && free < 256 * GIB {
        return 1;
    }
    0
}

/// Formats a used/total pair for the capacity gauge, both figures sharing the
/// **total's** unit so they can be compared at a glance: `26.0 / 57.3 GB`.
///
/// [`format_size`] cannot do this — called twice it picks each number's own
/// unit, so a nearly empty disk would read "900 Mo/1,8 To" and force the reader
/// to convert before judging anything. Same base-1024 convention and same
/// localized unit names, so the gauge agrees with the size column.
///
/// Used rather than free, and it must stay that way: the bar beside these
/// figures grows as the volume fills, so a figure counting DOWN while the bar
/// counts up left the reader with two contradictory readings of one fact. The
/// free space has its own, unambiguous place in the hover hint.
pub fn format_used_total(used: u64, total: u64, units: SizeUnits<'_>) -> String {
    // Unit of the total: the larger of the two, so the pair stays comparable.
    let mut scale = 1.0_f64;
    let mut idx = 0usize;
    while (total as f64) / scale >= 1024.0 && idx + 1 < units.steps.len() {
        scale *= 1024.0;
        idx += 1;
    }
    let decimals = if idx == 0 { 0 } else { 1 };
    // Spaces around the slash: at this size the two figures ran into the
    // separator and read as one long number.
    format!(
        "{} / {} {}",
        units.number(used as f64 / scale, decimals),
        units.number(total as f64 / scale, decimals),
        units.steps[idx]
    )
}

/// ISO-like format, locale-independent: `YYYY-MM-DD HH:MM`. It stays
/// readable in every language without depending on `chrono`.
///
/// `offset_secs` shifts the Unix (UTC) timestamp before decomposition: `0` = UTC,
/// a local offset (set by the GUI) = local time. The timezone is a user
/// setting — the core stays agnostic by receiving the already-resolved offset.
pub fn format_mtime(unix: i64, offset_secs: i64) -> String {
    let unix = unix + offset_secs;
    let days_from_epoch = unix.div_euclid(86_400);
    let secs_in_day = unix.rem_euclid(86_400);
    let hh = (secs_in_day / 3600) as u32;
    let mm = ((secs_in_day % 3600) / 60) as u32;

    let (y, mo, d) = civil_from_days(days_from_epoch);
    format!("{y:04}-{mo:02}-{d:02} {hh:02}:{mm:02}")
}

/// Compact units for the "age" column depending on the language:
/// `(minute, day, month, year, "just now")`. The hour uses `h` and
/// minutes the apostrophe `'` (compact and language-neutral, for example "2 h 50'").
#[derive(Debug, Clone, Copy)]
pub struct AgeUnits<'a> {
    pub minute: &'a str,
    pub day: &'a str,
    pub month: &'a str,
    pub year: &'a str,
    /// Whole wording for "less than a minute ago" — a sentence, not a unit.
    pub now: &'a str,
}

/// Formats the **age** (now − mtime) compactly, readable at a glance:
/// `42 min`, `2 h 50'`, `25d`, `3mo`, `2y`. `now_unix` and
/// `mtime_unix` are Unix seconds.
pub fn format_age(mtime_unix: i64, now_unix: i64, units: AgeUnits<'_>) -> String {
    let secs = (now_unix - mtime_unix).max(0);
    if secs < 60 {
        return units.now.to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins} {}", units.minute);
    }
    let hours = mins / 60;
    if hours < 24 {
        let m = mins % 60;
        return format!("{hours} h {m:02}'");
    }
    let days = hours / 24;
    if days < 31 {
        return format!("{days}{}", units.day);
    }
    if days < 365 {
        let mo = days / 30;
        return format!("{mo}{}", units.month);
    }
    let years = days / 365;
    format!("{years}{}", units.year)
}

/// "Heat" index of the age, for the hot→cold color gradient of the
/// "age" column: `0` = very recent (hot) … `6` = old (cold).
pub fn age_bucket(mtime_unix: i64, now_unix: i64) -> i32 {
    let secs = (now_unix - mtime_unix).max(0);
    let hours = secs / 3600;
    let days = hours / 24;
    if hours < 1 {
        0
    } else if hours < 6 {
        1
    } else if days < 1 {
        2
    } else if days < 7 {
        3
    } else if days < 30 {
        4
    } else if days < 365 {
        5
    } else {
        6
    }
}

/// Converts days-since-1970 → (year, month, day) (Howard Hinnant's
/// algorithm, public domain). Avoids the dependency on `chrono` for this
/// minimal need.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
