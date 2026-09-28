use super::*;

/// Opens `path` ELEVATED (UAC prompt) — the context menu's "Run as
/// administrator". On an `.exe`: "run as administrator"; on
/// a document, fails cleanly if the type has no registered "runas" verb.
/// Windows only (the menu entry is hidden elsewhere).
#[cfg(windows)]
pub fn open_elevated(path: &Path) -> Result<()> {
    shell_execute_runas(path, &[])
}
#[cfg(not(windows))]
pub fn open_elevated(path: &Path) -> Result<()> {
    let _ = path;
    Err(anyhow!("elevation: Windows only"))
}

/// Launches `program` ELEVATED (UAC prompt) via `ShellExecuteW` with the
/// "runas" verb — Windows only. `Command`/`CreateProcess` can't elevate;
/// only the Shell can. The arguments are reassembled into ONE command line
/// using `CommandLineToArgvW`'s quoting (backslashes/quotes) — the
/// target program thus receives the SAME `argv` as with a direct spawn.
#[cfg(windows)]
pub(super) fn shell_execute_runas(program: &Path, argv: &[String]) -> Result<()> {
    const SE_ERR_ACCESSDENIED: isize = 5; // includes cancellation of the UAC prompt
    info!(program = %program.display(), argc = argv.len(), "run opener elevated (runas)");
    let mut params = String::new();
    for (i, a) in argv.iter().enumerate() {
        if i > 0 {
            params.push(' ');
        }
        win_append_arg(&mut params, a);
    }
    // Fire-and-forget, like `open_path`: working directory inherited from Favnyr
    // (parity with the non-elevated spawn, which also doesn't set the cwd).
    let r = shell_execute(
        SHELL_VERB_RUNAS,
        program.as_os_str(),
        (!argv.is_empty()).then(|| OsStr::new(&params)),
        None,
    );
    if r > 32 {
        Ok(())
    } else if r == SE_ERR_ACCESSDENIED {
        Err(anyhow!("elevation refused (UAC) for {}", program.display()))
    } else {
        Err(anyhow!(
            "admin launch of {} failed (ShellExecute code {r})",
            program.display()
        ))
    }
}

/// Appends `arg` to a Windows command line using `CommandLineToArgvW`'s
/// quoting rules (quotes if space/tab/quote; backslashes doubled
/// before a quote or the end of a token). Ported from `std::sys::…::append_arg`.
#[cfg(windows)]
pub(super) fn win_append_arg(cmd: &mut String, arg: &str) {
    let quote = arg.is_empty() || arg.contains([' ', '\t', '"']);
    if quote {
        cmd.push('"');
    }
    let mut backslashes: usize = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // N backslashes + the quote → 2N+1 backslashes then the `"`.
                for _ in 0..=backslashes {
                    cmd.push('\\');
                }
            }
            backslashes = 0;
        }
        cmd.push(c);
    }
    if quote {
        // Trailing backslashes doubled before the closing quote.
        for _ in 0..backslashes {
            cmd.push('\\');
        }
        cmd.push('"');
    }
}

/// Opens the OS's native **"Open with"** picker for `path`.
///
/// Windows: standard Shell dialog ("How do you want to open this
/// file?") via `rundll32 shell32.dll,OpenAs_RunDLL <path>`. The path is
/// passed as a **raw argument** (`raw_arg`) to prevent `std` from wrapping it
/// in quotes — `OpenAs_RunDLL` takes the entire rest of the command line
/// as-is (quotes would break opening paths containing spaces).
///
/// Other OSes: not supported (no universal picker) — the corresponding
/// menu entry is hidden outside Windows.
#[cfg(windows)]
pub fn open_with(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    info!(path = %path.display(), "open-with dialog");
    Command::new("rundll32.exe")
        .raw_arg(format!("shell32.dll,OpenAs_RunDLL {}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("spawn rundll32 (open with): {e}"))
}
#[cfg(not(windows))]
pub fn open_with(_path: &Path) -> Result<()> {
    Err(anyhow!("\"Open with\" is not supported on this platform"))
}
