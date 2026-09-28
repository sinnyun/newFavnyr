use super::*;

// ---------- i18n helpers ----------

pub(super) fn apply_language(window: &MainWindow, lang: Lang) {
    window
        .global::<crate::ApplicationApi>()
        .set_strings(i18n::strings_for(lang));

    let lang_labels: Vec<SharedString> = i18n::language_labels();
    let lang_idx = Lang::all().iter().position(|l| *l == lang).unwrap_or(0) as i32;
    window
        .global::<crate::ApplicationApi>()
        .set_language_labels(ModelRc::new(VecModel::from(lang_labels)));
    window
        .global::<crate::ApplicationApi>()
        .set_language_index(lang_idx);

    let theme_labels: Vec<SharedString> = i18n::theme_labels(lang);
    window
        .global::<crate::ApplicationApi>()
        .set_theme_labels(ModelRc::new(VecModel::from(theme_labels)));

    // "Default tab bar position" setting: same
    // labels as the per-view menu.
    let tabbar_labels: Vec<SharedString> = i18n::tabbar_labels(lang);
    window
        .global::<crate::ApplicationApi>()
        .set_tabbar_default_labels(ModelRc::new(VecModel::from(tabbar_labels)));

    // The per-panel footer is recomputed by update_panels_ui (called after
    // a language change from the on_language_changed callback).
}
