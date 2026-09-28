use super::*;

/// Splits the "arguments" field (a single line) into individual arguments.
///
/// Spaces separate arguments, and quotes — single or double — group a run into
/// ONE argument, the shell convention every user already knows. Without them a
/// literal containing a space, such as an archive named `My Archive.7z`, could
/// not be expressed at all: the quotes would reach the program verbatim and it
/// would create two mangled files.
///
/// Quotes may open mid-argument (`-o{dir}/"My Folder"`), and an unclosed quote
/// simply runs to the end of the line rather than being reported as an error —
/// the field is edited live, so it is briefly unbalanced on almost every
/// keystroke.
///
/// TAGS are substituted AFTER this split, so a path containing spaces stays one
/// argument without any quoting from the user.
pub(in crate::bridge) fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false; // distinguishes "" (an empty quoted argument) from no argument
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// Renders ONE argument so that [`split_args`] hands it back unchanged.
///
/// Needed because an argument list is stored split, and putting it back in
/// front of the user means writing a command line again: joining with plain
/// spaces would lose exactly what made a path with spaces a single argument,
/// and the next save would tear it into pieces.
///
/// `split_args` has no escape character, so an argument is protected by the
/// quote it does not itself contain.
pub(in crate::bridge) fn quote_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\'')
    {
        return arg.to_string();
    }
    if !arg.contains('"') {
        return format!("\"{arg}\"");
    }
    if !arg.contains('\'') {
        return format!("'{arg}'");
    }
    // Both quote characters: neither can wrap the whole argument. A quote may
    // open anywhere inside a token, though, so each awkward character is
    // wrapped in the other quote and the pieces are written with nothing
    // between them — `split_args` rejoins them into one argument.
    let mut out = String::with_capacity(arg.len() + 2);
    for c in arg.chars() {
        match c {
            '"' => out.push_str("'\"'"),
            '\'' => out.push_str("\"'\""),
            c if c.is_whitespace() => {
                out.push('"');
                out.push(c);
                out.push('"');
            }
            c => out.push(c),
        }
    }
    out
}

/// Writes an argument list back as an editable command line. Inverse of
/// [`split_args`]: what this produces, that one splits back identically.
pub(in crate::bridge) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders an `argv` for display, quoting whatever contains a space so that
/// argument boundaries stay readable. A plain join would show a single path
/// containing spaces exactly like two separate arguments.
pub(in crate::bridge) fn display_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.contains(char::is_whitespace) {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses a list of extensions "png, jpg ; .gif" → `["png","jpg","gif"]`.
pub(in crate::bridge) fn parse_exts(s: &str) -> Vec<String> {
    s.split([',', ';', ' '])
        .map(|x| {
            let x = x.trim();
            // `*.zip` is the shell spelling everyone reaches for, and it used to
            // be stored verbatim — matching nothing, with the entry silently
            // vanishing from the menu. A lone `*` is left alone: it IS the
            // wildcard (see `openers::CTX_EXT_ALL`).
            x.strip_prefix("*.")
                .unwrap_or(x)
                .trim_start_matches('.')
                .to_ascii_lowercase()
        })
        .filter(|x| !x.is_empty())
        .collect()
}

/// A ready-made command the user can install in one click.
pub(in crate::bridge) struct Recipe {
    /// Tool the command drives, used to report unavailable recipe families.
    /// Never translated because it is a program name. It is deliberately not
    /// included in the visible label: the built-in icon already identifies the
    /// family without repeating "7z —" or "tar —" before every action.
    pub(in crate::bridge) tool: &'static str,
    /// i18n key of the ACTION, e.g. "extract here". Deliberately shared between
    /// tools that do the same thing, so one translation serves all of them.
    pub(in crate::bridge) label_key: &'static str,
    /// Program candidates, first launchable one wins — see
    /// [`actions::resolve_program`]. Entries that make no sense on the running
    /// platform are simply skipped, so one list covers Linux and Windows.
    pub(in crate::bridge) programs: &'static [&'static str],
    pub(in crate::bridge) args: &'static str,
    /// Menus the command is pinned to by default (see `openers::CTX_*`).
    pub(in crate::bridge) ctx: u8,
    /// Extensions the FILE entry is restricted to; a lone [`openers::CTX_EXT_ALL`]
    /// means every file. The extract recipes narrow it to archives — offering
    /// "Extract here" on a text file is noise in the right-click menu.
    pub(in crate::bridge) ctx_exts: &'static [&'static str],
    /// Visual family persisted with commands created from this template.
    pub(in crate::bridge) icon: openers::OpenerIcon,
}

impl Recipe {
    /// Label shown on the row and used as the created command's name.
    pub(in crate::bridge) fn label(&self, lang: Lang) -> String {
        i18n::tr(lang, self.label_key)
    }
}

/// 7-Zip candidates. The installer registers a shell extension but adds
/// nothing to PATH on Windows, hence the absolute paths alongside the bare
/// names used on Linux (`7zz` is the upstream build, `7z`/`7za` come p7zip).
pub(in crate::bridge) const SEVEN_ZIP: &[&str] = &[
    "7z",
    "7zz",
    "7za",
    r"C:\Program Files\7-Zip\7z.exe",
    r"C:\Program Files (x86)\7-Zip\7z.exe",
];

/// Ready-made commands.
///
/// Archiving and sharing are the two jobs the custom-command system gets asked
/// to do most, and hand-writing the command line is precisely what these spare
/// the user. They stay editable templates: picking one opens the editor
/// prefilled instead of saving anything silently, so they double as a worked
/// example of what the tags can express.
///
/// Order matters — it is the display order: archivers first, grouped by tool,
/// then sharing.
///
/// None of these tools opens a format-chooser dialog: the archivers listed
/// here are driven entirely from their command line (on Windows such a dialog
/// belongs to the shell extension, which Favnyr already exposes through the
/// native context menu), and `tar` has no interface at all.
pub(in crate::bridge) const RECIPES: &[Recipe] = &[
    // ----- 7-Zip -----
    // One archive per selected item, named after it, created alongside it.
    Recipe {
        tool: "7z",
        label_key: "ow_recipe_compress_each",
        programs: SEVEN_ZIP,
        args: "a {dir}/{stem}.7z {file}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE | openers::CTX_DIR,
        icon: openers::OpenerIcon::SevenZip,
    },
    // Everything in ONE archive, which `{files}` exists to make expressible.
    // `{setname}` names it after the selected item when there is only one, and
    // after the folder they share otherwise — what archivers do.
    Recipe {
        tool: "7z",
        label_key: "ow_recipe_compress_zip",
        programs: SEVEN_ZIP,
        args: "a {dir}/{setname}.zip {files}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE | openers::CTX_DIR,
        icon: openers::OpenerIcon::SevenZip,
    },
    Recipe {
        tool: "7z",
        label_key: "ow_recipe_extract_here",
        programs: SEVEN_ZIP,
        args: "x -o{dir} {file}",
        ctx_exts: rfs::ARCHIVE_EXTENSIONS,
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::SevenZip,
    },
    // Into a subfolder named after the archive, so a messy archive does not
    // scatter its contents over the current folder. 7-Zip creates the folder.
    Recipe {
        tool: "7z",
        label_key: "ow_recipe_extract_folder",
        programs: SEVEN_ZIP,
        args: "x -o{dir}/{stem} {file}",
        ctx_exts: rfs::ARCHIVE_EXTENSIONS,
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::SevenZip,
    },
    // ----- tar -----
    // `-C {dir}` plus bare names: handed absolute paths, tar strips the leading
    // "/" and stores the whole tree leading to each file, so the archive held
    // `home/user/Docs/a.txt` instead of `a.txt`.
    Recipe {
        tool: "tar",
        label_key: "ow_recipe_compress_targz",
        programs: &["tar"],
        args: "-czf {dir}/{setname}.tar.gz -C {dir} {names}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE | openers::CTX_DIR,
        icon: openers::OpenerIcon::Archive,
    },
    // Own label key rather than the one 7z's identical-looking recipe uses:
    // this family carries a "Tar - " prefix in its wording, on top of the icon,
    // so its own args do not accidentally inherit changes made for 7z.
    // `-xf` alone detects gzip/bzip2/xz; `-C` needs the folder to exist, hence
    // the current one rather than a new subfolder.
    Recipe {
        tool: "tar",
        label_key: "ow_recipe_extract_here_tar",
        programs: &["tar"],
        args: "-xf {file} -C {dir}",
        ctx_exts: rfs::ARCHIVE_EXTENSIONS,
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::Archive,
    },
    // `--one-top-level` (bare) both creates the destination folder — unlike
    // `-C`, which needs it to exist — and derives its name from the archive.
    // That derivation is why this reads `{file}` rather than a manufactured
    // path: Rust's `{stem}` strips only the LAST extension, turning
    // "archive.tar.gz" into "archive.tar"; tar's own suffix stripping gets the
    // compound ".tar.gz" right. Verified against a real archive, including a
    // name with extra dots in it, not assumed from the option's description.
    Recipe {
        tool: "tar",
        label_key: "ow_recipe_extract_folder_tar",
        programs: &["tar"],
        args: "-C {dir} --one-top-level -xf {file}",
        ctx_exts: rfs::ARCHIVE_EXTENSIONS,
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::Archive,
    },
    // ----- Sharing -----
    // No cross-desktop standard exists on Linux: the entry Dolphin offers comes
    // from KDE's Purpose framework, a library with no command-line entry point.
    // These call the usual tools directly and read as actions, like the archive
    // recipes whose icon now carries the tool identity.
    //
    // One mail composer per item: `--attach` takes a single file, so a multiple
    // selection opens several windows — the run count shown under the preview
    // says so before anything is saved.
    Recipe {
        tool: "",
        label_key: "ow_recipe_share_email",
        programs: &["xdg-email"],
        args: "--attach {file}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::Email,
    },
    // The GUI handler, not `kdeconnect-cli`: it opens a picker limited to
    // paired, reachable devices, whereas the CLI demands a device id up front.
    Recipe {
        tool: "",
        label_key: "ow_recipe_share_kdeconnect",
        programs: &["kdeconnect-handler"],
        args: "{file}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::Device,
    },
    // Both take the files last and show a device chooser when none is given,
    // so the whole selection goes in one run.
    Recipe {
        tool: "",
        label_key: "ow_recipe_share_bluetooth",
        programs: &["bluetooth-sendto", "blueman-sendto"],
        args: "{files}",
        ctx_exts: &[openers::CTX_EXT_ALL],
        ctx: openers::CTX_FILE,
        icon: openers::OpenerIcon::Device,
    },
];

/// Publishes the recipes whose tool is actually installed. The others are left
/// out entirely rather than shown as unusable, and the block disappears when
/// none remains. Each row carries its index in [`RECIPES`], since filtering
/// makes the displayed order no longer match the table's.
///
/// Rows are dealt into two balanced COLUMNS, filled top to bottom so a tool's
/// recipes stay adjacent. The split lives here because Slint's `for` cannot
/// carry an inline slice.
///
/// Also words the tools that resolved to nothing. Naming them is what answers
/// "why is my archiver missing?", and deriving the list from the table keeps it
/// correct on both platforms — outside Windows a bare command resolves through
/// PATH, on Windows it never can, so a hard-coded list would promise tools that
/// are structurally unreachable there.
pub(in crate::bridge) fn push_recipes_ui(window: &MainWindow, state: &AppState) {
    let lang = state.snapshot_config().language;
    let mut rows: Vec<OwRecipe> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for (index, r) in RECIPES.iter().enumerate() {
        if actions::resolve_program(r.programs).is_some() {
            rows.push(OwRecipe {
                label: r.label(lang).into(),
                index: index as i32,
                icon_kind: r.icon.as_i32(),
            });
        } else if !r.tool.is_empty() && !missing.contains(&r.tool) {
            missing.push(r.tool);
        }
    }
    let mid = rows.len().div_ceil(2);
    let (col1, col2) = rows.split_at(mid);
    window
        .global::<crate::SettingsApi>()
        .set_ow_recipes_col1(ModelRc::new(VecModel::from(col1.to_vec())));
    window
        .global::<crate::SettingsApi>()
        .set_ow_recipes_col2(ModelRc::new(VecModel::from(col2.to_vec())));
    let hint = if missing.is_empty() {
        String::new()
    } else {
        i18n::tr(lang, "ow_recipes_missing")
            .replace("{tools}", &i18n::process_list(lang, &missing, false))
    };
    window
        .global::<crate::SettingsApi>()
        .set_ow_recipes_missing(hint.into());
}

/// Recomputes the command's live preview + the program's validity (popup).
pub(in crate::bridge) fn recompute_ow_preview(window: &MainWindow, state: &AppState) {
    let program = window
        .global::<crate::SettingsApi>()
        .get_ow_popup_program()
        .to_string();
    let args = split_args(&window.global::<crate::SettingsApi>().get_ow_popup_args());
    // An OS association app (Windows UWP/Store, Linux `.desktop`) has no exe
    // to validate; otherwise the program must exist (path) OR be a PATH
    // command on Linux — see `program_is_valid`.
    window.global::<crate::SettingsApi>().set_ow_popup_valid(
        window
            .global::<crate::SettingsApi>()
            .get_ow_popup_is_store()
            || actions::program_is_valid(&program),
    );
    let selection = selected_paths(state);
    // Sample used to resolve the tags. A bare "example.txt" would leave `{dir}`
    // empty, so an argument like `{dir}/out.7z` would render as "/out.7z" and
    // suggest the file lands at the filesystem root. The stand-in is therefore
    // built inside the current folder, which is what the command will really see.
    let sample = selection.first().cloned().unwrap_or_else(|| {
        let dir = state.current_path();
        if dir.as_os_str().is_empty() {
            PathBuf::from("example.txt")
        } else {
            dir.join("example.txt")
        }
    });
    let temp = openers::Opener {
        id: String::new(),
        label: String::new(),
        program: program.clone(),
        assoc: None,
        icon: openers::OpenerIcon::None,
        args,
        default_exts: Vec::new(),
        used_exts: Vec::new(),
        use_count: 0,
        last_used: 0,
        elevated: false, // has no effect on the preview (renders the argv only)
        ctx_menu: 0,
        ctx_exts: Vec::new(),
    };
    let ctx = openers::TagContext::from_path(&sample);
    // `{files}` runs once over the whole selection, so the preview must show
    // the expanded list rather than a single file — otherwise it would suggest
    // the wrong mode entirely.
    let argv = if temp.expands_list() {
        let refs: Vec<&Path> = if selection.is_empty() {
            vec![sample.as_path()]
        } else {
            selection.iter().map(PathBuf::as_path).collect()
        };
        temp.render_for_batch(&ctx, &refs)
    } else {
        temp.render_for_file(&ctx)
    };
    let prog_name = Path::new(&program)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.clone());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_preview(format!("{prog_name} {}", display_argv(&argv)).into());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_runs(run_count_text(
            state.snapshot_config().language,
            temp.has_tag() && !temp.expands_list(),
            selection.len(),
        ));
}

/// Sentence telling how many processes the command will start.
///
/// The count is not a detail: an argument template that mentions a tag runs
/// ONCE PER SELECTED FILE, otherwise a single process receives them all. That
/// rule was previously invisible — the preview always rendered one file, so a
/// command about to start twenty processes looked exactly like one starting a
/// single process.
pub(in crate::bridge) fn run_count_text(
    lang: Lang,
    per_file: bool,
    selected: usize,
) -> SharedString {
    if !per_file {
        return i18n::tr(lang, "ow_runs_once").into();
    }
    match selected {
        // Nothing selected (typically from Settings): state the rule instead of
        // a count that would only be true right now.
        0 => i18n::tr(lang, "ow_runs_per_file"),
        1 => i18n::tr(lang, "ow_runs_once"),
        n => i18n::tr(lang, "ow_runs_n_times").replace("{n}", &n.to_string()),
    }
    .into()
}

/// Replaces an opener's list of "Favnyr default" extensions.
pub(in crate::bridge) fn apply_default_exts(state: &AppState, id: &str, exts: &[String]) {
    let mut s = state.openers.borrow_mut();
    if let Some(o) = s.openers.iter_mut().find(|o| o.id == id) {
        o.default_exts.clear();
    }
    for e in exts {
        s.set_default_ext(id, e, true);
    }
}
