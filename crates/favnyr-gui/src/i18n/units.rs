use super::*;

/// "Notice" toast message when access to a folder is denied.
/// Size wording for `lang`, ready for [`favnyr_core::fs::format_size`]. The
/// core crate holds no translations, so the vocabulary is resolved here and
/// handed down — decimal mark included, since a language that writes `Ko` also
/// writes `1,0`.
///
/// Memoized like [`catalog`]: this is called once per listed row, so resolving
/// six keys through the map every time would show on a large folder.
pub fn size_units(lang: Lang) -> favnyr_core::fs::SizeUnits<'static> {
    type Units = favnyr_core::fs::SizeUnits<'static>;
    fn build(lang: Lang) -> Units {
        let c = catalog(lang);
        let g = |k: &'static str| -> &'static str { c.get(k).map(|s| s.as_str()).unwrap_or(k) };
        favnyr_core::fs::SizeUnits {
            steps: [
                g("unit_byte"),
                g("unit_kb"),
                g("unit_mb"),
                g("unit_gb"),
                g("unit_tb"),
            ],
            decimal: g("decimal_separator").chars().next().unwrap_or('.'),
        }
    }
    static EN: OnceLock<Units> = OnceLock::new();
    static FR: OnceLock<Units> = OnceLock::new();
    static ES: OnceLock<Units> = OnceLock::new();
    static DE: OnceLock<Units> = OnceLock::new();
    static IT: OnceLock<Units> = OnceLock::new();
    static ZH: OnceLock<Units> = OnceLock::new();
    let cell = match lang {
        Lang::En => &EN,
        Lang::Fr => &FR,
        Lang::Es => &ES,
        Lang::De => &DE,
        Lang::It => &IT,
        Lang::Zh => &ZH,
    };
    *cell.get_or_init(|| build(lang))
}

/// Age-column wording for `lang`, ready for [`favnyr_core::fs::format_age`].
/// Memoized for the same reason as [`size_units`].
pub fn age_units(lang: Lang) -> favnyr_core::fs::AgeUnits<'static> {
    type Units = favnyr_core::fs::AgeUnits<'static>;
    fn build(lang: Lang) -> Units {
        let c = catalog(lang);
        let g = |k: &'static str| -> &'static str { c.get(k).map(|s| s.as_str()).unwrap_or(k) };
        favnyr_core::fs::AgeUnits {
            minute: g("age_minute"),
            day: g("age_day"),
            month: g("age_month"),
            year: g("age_year"),
            now: g("age_now"),
        }
    }
    static EN: OnceLock<Units> = OnceLock::new();
    static FR: OnceLock<Units> = OnceLock::new();
    static ES: OnceLock<Units> = OnceLock::new();
    static DE: OnceLock<Units> = OnceLock::new();
    static IT: OnceLock<Units> = OnceLock::new();
    static ZH: OnceLock<Units> = OnceLock::new();
    let cell = match lang {
        Lang::En => &EN,
        Lang::Fr => &FR,
        Lang::Es => &ES,
        Lang::De => &DE,
        Lang::It => &IT,
        Lang::Zh => &ZH,
    };
    *cell.get_or_init(|| build(lang))
}
