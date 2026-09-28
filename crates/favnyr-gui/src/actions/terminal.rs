use super::*;

/// Opens the Windows Recycle Bin in Explorer. The Recycle Bin
/// is a *virtual* folder there (no real path) → we go through the shell.
#[cfg(windows)]
pub fn open_trash() -> Result<()> {
    Command::new("explorer")
        .arg("shell:RecycleBinFolder")
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow!("spawn explorer (recycle bin): {e}"))
}

/// Builds the silent launch of the Windows Terminal alias.
#[cfg(windows)]
pub(super) fn windows_terminal_command(cwd: &Path) -> Command {
    use std::os::windows::process::CommandExt;

    let mut command = Command::new("wt.exe");
    command
        .arg("-d")
        .arg(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // `wt.exe` in WindowsApps is a very brief console alias/launcher.
        // Hiding it doesn't affect the actual Windows Terminal GUI window.
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
pub(super) fn command_prompt_command(cwd: &Path) -> Command {
    use std::os::windows::process::CommandExt;

    let mut command = Command::new("cmd.exe");
    command
        // Favnyr is a GUI application with no parent console: we explicitly
        // create THE final interactive console, at the right cwd.
        .current_dir(cwd)
        .creation_flags(CREATE_NEW_CONSOLE);
    command
}

/// Opens a terminal with `cwd` set to the folder containing `path`.
pub fn open_terminal(path: &Path) -> Result<()> {
    let cwd = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("no parent for {}", path.display()))?
    };

    #[cfg(windows)]
    {
        // Windows Terminal (`wt`) first: clean window, `-d` = cwd.
        match windows_terminal_command(&cwd).spawn() {
            Ok(_) => {
                info!(cwd = %cwd.display(), "opening Windows Terminal");
                return Ok(());
            }
            Err(error) => {
                debug!(error = %error, "Windows Terminal unavailable; falling back to cmd");
            }
        }
        // Direct fallback: the old `cmd /C start … cmd /K` chain created a
        // first intermediate cmd window visible for a few milliseconds, then the
        // final terminal. A single `cmd.exe` with `CREATE_NEW_CONSOLE` is enough.
        command_prompt_command(&cwd)
            .spawn()
            .map(|_| ())
            .map_err(|e| anyhow!("spawn cmd: {e}"))
    }
    #[cfg(not(windows))]
    {
        let term = pick_terminal()
            .ok_or_else(|| anyhow!("no terminal detected ($TERMINAL or known candidates)"))?;
        info!(terminal = %term, cwd = %cwd.display(), "opening a terminal");
        Command::new(&term)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| anyhow!("spawn '{term}' in '{}': {e}", cwd.display()))
    }
}
