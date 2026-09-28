use super::*;

/// Language picker labels: **native names** (each language in its own
/// language → never translated).
pub fn language_labels() -> Vec<slint::SharedString> {
    Lang::all().iter().map(|l| l.native_name().into()).collect()
}

/// Theme picker labels (Auto / Light / Dark) in the given language.
pub fn theme_labels(lang: Lang) -> Vec<slint::SharedString> {
    vec![
        tr(lang, "theme_auto").into(),
        tr(lang, "theme_light").into(),
        tr(lang, "theme_dark").into(),
    ]
}

/// Tab bar position picker labels (top / left /
/// right) — same text as the per-view menu.
pub fn tabbar_labels(lang: Lang) -> Vec<slint::SharedString> {
    vec![
        tr(lang, "tabbar_top").into(),
        tr(lang, "tabbar_left").into(),
        tr(lang, "tabbar_right").into(),
    ]
}

/// Footer text based on the item count.
pub fn footer_items_text(lang: Lang, count: usize) -> String {
    match count {
        0 => tr(lang, "footer_empty"),
        1 => tr(lang, "footer_items_singular"),
        n => tr(lang, "footer_items_plural").replace("{n}", &n.to_string()),
    }
}

/// Summary chip standing in for the operation toasts the stack cannot show.
/// Only called with `count >= 1`.
pub fn op_more_text(lang: Lang, count: usize) -> String {
    match count {
        1 => tr(lang, "op_more_singular"),
        n => tr(lang, "op_more_plural").replace("{n}", &n.to_string()),
    }
}

/// Full footer: "N items", then (if `hidden > 0`) "· K hidden",
/// then (if there's a selection) "· M selected". The "hidden" segment is only
/// passed when hidden items are NOT shown (a subtle reminder).
pub fn footer_text(lang: Lang, total: usize, selected: usize, hidden: usize) -> String {
    let mut s = footer_items_text(lang, total);
    if hidden > 0 {
        let h = match hidden {
            1 => tr(lang, "footer_hidden_singular"),
            n => tr(lang, "footer_hidden_plural").replace("{n}", &n.to_string()),
        };
        s = format!("{s}{}{h}", tr(lang, "separator_dot"));
    }
    if selected > 0 {
        let sel = match selected {
            1 => tr(lang, "footer_selected_singular"),
            n => tr(lang, "footer_selected_plural").replace("{n}", &n.to_string()),
        };
        s = format!("{s}{}{sel}", tr(lang, "separator_dot"));
    }
    s
}

// Keyboard shortcuts: labels resolved from the catalog -----

/// Localized label of a shortcut group (`code` = `ActionGroup::code`).
pub fn shortcut_group_name(lang: Lang, code: &str) -> String {
    tr(lang, &format!("shortcut_group.{code}"))
}

/// Localized label of a shortcut action (`id` = `ActionDef::id`).
pub fn shortcut_action_name(lang: Lang, id: &str) -> String {
    tr(lang, &format!("shortcut_action.{id}"))
}

/// Localized conflict message: the template contains `{name}`.
pub fn shortcut_conflict_message(lang: Lang, other: &str) -> String {
    tr(lang, "shortcut_conflict.template").replace("{name}", other)
}
