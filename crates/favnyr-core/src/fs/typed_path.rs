use super::*;

// ---------- What the user types in the address bar ----------

/// Turns an address typed by the user into a path.
///
/// Two conveniences, each written the way its own platform writes it:
///   - `~` and `~/…` for the home folder, on every platform;
///   - `%VAR%` on Windows, `$VAR` and `${VAR}` elsewhere.
///
/// **A name that is not a defined variable is left exactly as typed.** That one
/// rule covers the three cases that matter: a real variable expands, a typo
/// reaches the caller untouched so the usual "not listable" message can name
/// it, and a folder whose name merely carries a percent sign or a dollar is
/// never mangled.
///
/// Case follows the platform for free: a variable name is matched without
/// regard to case on Windows and with it elsewhere, because that is how the
/// system itself answers.
///
/// A folder CAN legitimately be named after a variable. Expanding wins, as it
/// does in the system's own file manager; such a folder stays reachable by
/// navigating into it rather than by typing its name.
pub fn expand_typed_path(raw: &str) -> PathBuf {
    // The tilde goes first, as a shell does it: it is only special at the very
    // start, and what a variable expands to must not be re-read for one.
    match strip_home_prefix(raw) {
        Some((home, rest)) => {
            let rest = expand_variables(rest);
            if rest.is_empty() {
                home
            } else {
                home.join(rest)
            }
        }
        None => PathBuf::from(expand_variables(raw)),
    }
}

/// Splits a leading `~` off, returning the home folder and what followed it.
/// Both separators are accepted on Windows, where a user types either.
fn strip_home_prefix(raw: &str) -> Option<(PathBuf, &str)> {
    let home = || dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    if raw == "~" {
        return Some((home(), ""));
    }
    let rest = raw.strip_prefix("~/").or_else(|| {
        if cfg!(windows) {
            raw.strip_prefix(r"~\")
        } else {
            None
        }
    })?;
    Some((home(), rest))
}

/// Replaces every `%NAME%` that names a defined variable. A `%` that opens
/// nothing, or opens a name the environment does not know, stays where it is —
/// which is also what leaves `%%` alone.
#[cfg(windows)]
fn expand_variables(text: &str) -> String {
    expand_variables_with(text, |name| std::env::var(name).ok())
}

/// The rules above, with the environment handed in: what a variable resolves to
/// is the only thing here that depends on the machine, so injecting it is what
/// makes the parsing testable.
#[cfg(windows)]
pub(super) fn expand_variables_with(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('%') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // The name runs to the next `%`. Anything may sit between the two but a
        // `%` itself: two real variables carry parentheses
        // (`%ProgramFiles(x86)%`), so letters alone would not do.
        match after.find('%').map(|close| (close, &after[..close])) {
            Some((close, name)) if !name.is_empty() => match lookup(name) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[close + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Replaces every `$NAME` and `${NAME}` that names a defined variable. A `$`
/// followed by something else, or by a name the environment does not know,
/// stays where it is.
#[cfg(not(windows))]
fn expand_variables(text: &str) -> String {
    expand_variables_with(text, |name| std::env::var(name).ok())
}

/// The rules above, with the environment handed in: what a variable resolves to
/// is the only thing here that depends on the machine, so injecting it is what
/// makes the parsing testable.
#[cfg(not(windows))]
pub(super) fn expand_variables_with(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(dollar) = rest.find('$') {
        out.push_str(&rest[..dollar]);
        let after = &rest[dollar + 1..];
        let braced = after.strip_prefix('{');
        let found = match braced {
            // `${NAME}`: the braces say where the name ends, so it may hold
            // anything, which is the reason the form exists.
            Some(body) => body
                .find('}')
                .map(|close| (&body[..close], &body[close + 1..])),
            // `$NAME`: the name ends at the first character a variable name
            // cannot hold, which is how `$USER/Documents` finds `USER`.
            None => {
                let end = after
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                Some((&after[..end], &after[end..]))
            }
        };
        match found {
            Some((name, tail)) if !name.is_empty() => match lookup(name) {
                Some(value) => {
                    out.push_str(&value);
                    rest = tail;
                }
                None => {
                    out.push('$');
                    rest = after;
                }
            },
            _ => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
