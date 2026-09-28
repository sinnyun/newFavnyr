use super::*;

/// Launches an **opener** ("Open with") on `paths`, detached and WITHOUT a shell
/// (security: `argv` as a list, never a concatenated string). `{file}` (or
/// no tag at all) → one instance per file / all paths queued together.
pub fn run_opener(opener: &Opener, paths: &[PathBuf]) -> Result<()> {
    // App based on an OS association handler (Windows UWP/Store, Linux
    // `.desktop`): no exe to launch via `Command` → we delegate to the OS, one
    // file at a time (the current file's extension resolves the right handler).
    if let Some(assoc) = &opener.assoc {
        if paths.is_empty() {
            return Err(anyhow!("no file to open with {}", opener.label));
        }
        for p in paths {
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            crate::openwith::launch(assoc, &ext, p)?;
        }
        return Ok(());
    }
    // Same rule as the editor's validation: an absolute path, or (outside
    // Windows) a bare command resolved through PATH. Testing `is_file()` here
    // would reject `7z` — a name accepted at save time, and one `Command` knows
    // how to resolve on its own.
    if !program_is_valid(&opener.program) {
        return Err(anyhow!("program not found: {}", opener.program));
    }
    let program = PathBuf::from(&opener.program);
    if paths.is_empty() {
        return spawn_detached(&program, opener.args.clone(), opener.elevated);
    }
    if opener.expands_list() {
        // A single instance covering the whole selection: `{files}` expands
        // where it stands. The other tags describe the first item — a selection
        // always comes from one folder, so `{dir}` and `{dirname}` designate
        // the folder shared by every path.
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let ctx = TagContext::from_path(&paths[0]);
        spawn_detached(
            &program,
            opener.render_for_batch(&ctx, &refs),
            opener.elevated,
        )
    } else if opener.has_tag() {
        // One instance per file (the template references the file).
        for p in paths {
            spawn_detached(
                &program,
                opener.render_for_file(&TagContext::from_path(p)),
                opener.elevated,
            )?;
        }
        Ok(())
    } else {
        // A single instance: fixed arguments + all paths queued.
        let mut argv = opener.args.clone();
        argv.extend(paths.iter().map(|p| p.display().to_string()));
        spawn_detached(&program, argv, opener.elevated)
    }
}

/// Launches `program` with `args` (detached). Exposed for the "Open
/// with" picker, which resolves the program and arguments itself.
pub fn spawn_program(program: &Path, args: &[String]) -> Result<()> {
    spawn_detached(program, args.to_vec(), false)
}

/// [`spawn_program`] started in `cwd`, for a launcher that names the folder its
/// application expects to run from (`Path=` in a desktop entry).
#[cfg_attr(windows, allow(dead_code))]
pub fn spawn_program_in(program: &Path, args: &[String], cwd: Option<&Path>) -> Result<()> {
    spawn_detached_in(program, args.to_vec(), false, cwd)
}

/// Launches an opener on a folder from a command pinned to the view's
/// background, e.g. "Git Bash here". In this context, `{dir}` and `{file}`
/// both equal the current folder (in `TagContext::from_path`, `{dir}` would be
/// the PARENT — counter-intuitive for an "here" command). OS association apps:
/// delegated to the OS on the folder.
pub fn run_opener_dir(opener: &Opener, dir: &Path) -> Result<()> {
    if let Some(assoc) = &opener.assoc {
        return crate::openwith::launch(assoc, "", dir);
    }
    // Same rule as the editor's validation: an absolute path, or (outside
    // Windows) a bare command resolved through PATH. Testing `is_file()` here
    // would reject `7z` — a name accepted at save time, and one `Command` knows
    // how to resolve on its own.
    if !program_is_valid(&opener.program) {
        return Err(anyhow!("program not found: {}", opener.program));
    }
    let program = PathBuf::from(&opener.program);
    let mut ctx = TagContext::from_path(dir);
    ctx.dir = ctx.file.clone();
    spawn_detached(&program, opener.render_for_file(&ctx), opener.elevated)
}

/// Opens a NEW Favnyr instance on `dir` (browser-style tab tear-off).
/// The binary is relaunched with the explicit internal protocol
/// `--detached-tab <path> ...`, so this path isn't confused with the
/// new public "workspace name" argument (cf. `main.rs`).
/// The path is passed as `OsStr` (no loss on non-UTF-8 paths).
///
/// `at` = SCREEN (physical) position where the window should open — the
/// cursor's drop point, so the window appears under the mouse. `None` → default
/// (centered) position. `tab_bar_mode` = tab bar position inherited from
/// the source view (0 top / 1 left / 2 right).
pub fn spawn_new_instance(dir: &Path, at: Option<(i32, i32)>, tab_bar_mode: u8) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| anyhow!("Favnyr executable not found: {e}"))?;
    info!(dir = %dir.display(), at = ?at, "spawn new Favnyr instance (tab tear-off)");
    let mut cmd = Command::new(&exe);
    cmd.arg("--detached-tab").arg(dir);
    // Position + bar mode as positional args after the internal marker.
    if let Some((x, y)) = at {
        cmd.arg(x.to_string()).arg(y.to_string());
        if tab_bar_mode > 0 {
            cmd.arg(tab_bar_mode.to_string());
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("new Favnyr instance: {e}"))
}

/// Opens a NEW Favnyr instance holding a whole VIEW: all its tabs, in order,
/// the FIRST being the one that stays active.
///
/// Its own marker rather than an extension of `--detached-tab`, which is left
/// untouched — so the proven single-tab path cannot regress, and two builds of
/// different versions keep understanding each other.
///
/// Every folder travels in the SAME launch, so the operation succeeds whole or
/// fails whole: there is no state where some tabs have moved and the others are
/// stranded. Paths go as `OsStr`, so no shell interprets them and a name that
/// is not valid UTF-8 survives the trip.
pub fn spawn_detached_view(dirs: &[PathBuf], at: (i32, i32), tab_bar_mode: u8) -> Result<()> {
    if dirs.is_empty() {
        return Err(anyhow!("a detached view needs at least one tab"));
    }
    let exe = std::env::current_exe().map_err(|e| anyhow!("Favnyr executable not found: {e}"))?;
    info!(tabs = dirs.len(), at = ?at, "spawn new Favnyr instance (view tear-off)");
    let mut cmd = Command::new(&exe);
    // Position and bar mode FIRST, fixed arity; the folders last, however many
    // there are. The list can then be read without any ambiguity.
    cmd.arg("--detached-view")
        .arg(at.0.to_string())
        .arg(at.1.to_string())
        .arg(tab_bar_mode.to_string());
    for dir in dirs {
        cmd.arg(dir);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("new Favnyr instance: {e}"))
}

/// Detached spawn (stdio cut off; on Windows, no ghost console).
///
/// `elevated` (Windows only): launches via `ShellExecuteW("runas", …)` →
/// UAC prompt. On other OSes the flag is ignored (elevating a graphical
/// app isn't a standard mechanism there): normal launch.
fn spawn_detached(program: &Path, argv: Vec<String>, elevated: bool) -> Result<()> {
    spawn_detached_in(program, argv, elevated, None)
}

/// [`spawn_detached`] with a working directory for the child. Split out rather
/// than added to every call site, which has no folder to impose and passes
/// `None`.
fn spawn_detached_in(
    program: &Path,
    argv: Vec<String>,
    elevated: bool,
    cwd: Option<&Path>,
) -> Result<()> {
    info!(program = %program.display(), argc = argv.len(), elevated, "run opener");
    #[cfg(windows)]
    if elevated {
        return shell_execute_runas(program, &argv);
    }
    let mut cmd = Command::new(program);
    cmd.args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Startup-notification tokens are valid for ONE launch: the desktop hands
    // one to Favnyr, and a child inheriting it would have its own startup
    // credited to Favnyr's window. Unset everywhere — they do not exist on
    // Windows, where removing them is a no-op.
    cmd.env_remove("DESKTOP_STARTUP_ID")
        .env_remove("XDG_ACTIVATION_TOKEN");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("launching {}: {e}", program.display()))
}

/// Launches executable `exe` with `files` as ARGUMENTS (the "drag a file
/// onto a .cmd/.bat/.exe" pattern). Not elevated.
///
/// Windows: `ShellExecuteW("open", …)` → handles `.cmd`/`.bat` (via their
/// `cmd.exe /c` association) just like `.exe`, which `CreateProcess`/`Command`
/// does NOT do for a script. Other OSes: detached `Command::new(exe).args(files)`
/// (the executable — shebang script or binary — receives the paths).
#[cfg(windows)]
pub fn run_with_files(exe: &Path, files: &[PathBuf]) -> Result<()> {
    info!(exe = %exe.display(), argc = files.len(), "run executable with dropped files");
    let mut params = String::new();
    for (i, f) in files.iter().enumerate() {
        if i > 0 {
            params.push(' ');
        }
        win_append_arg(&mut params, &f.to_string_lossy());
    }
    // Working directory = the executable's folder (like a double-click).
    let r = shell_execute(
        SHELL_VERB_OPEN,
        exe.as_os_str(),
        Some(OsStr::new(&params)),
        exe.parent(),
    );
    if r > 32 {
        Ok(())
    } else {
        Err(anyhow!(
            "launching {} failed (ShellExecute code {r})",
            exe.display()
        ))
    }
}
#[cfg(not(windows))]
pub fn run_with_files(exe: &Path, files: &[PathBuf]) -> Result<()> {
    info!(exe = %exe.display(), argc = files.len(), "run executable with dropped files");
    let mut cmd = Command::new(exe);
    for f in files {
        cmd.arg(f);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("launching {}: {e}", exe.display()))
}
