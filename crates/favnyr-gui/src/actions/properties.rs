use super::*;

/// Shows Windows' **native properties dialog** for `path` (General /
/// Security / Details / Previous Versions tabs…). Via `ShellExecuteExW`
/// with the "properties" verb and `SEE_MASK_INVOKEIDLIST`. A direct FFI to
/// shell32 avoids an extra dependency. Only exists on Windows:
/// other OSes have no portable native properties dialog → the caller
/// falls back to the custom properties panel.
#[cfg(windows)]
pub fn show_native_properties(path: &Path) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Win32 layout of SHELLEXECUTEINFOW (x64). The last field before
    // `h_process` is the hIcon/hMonitor union (a HANDLE → a pointer).
    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut core::ffi::c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut core::ffi::c_void,
        lp_id_list: *mut core::ffi::c_void,
        lp_class: *const u16,
        hkey_class: *mut core::ffi::c_void,
        dw_hot_key: u32,
        h_icon_or_monitor: *mut core::ffi::c_void,
        h_process: *mut core::ffi::c_void,
    }

    // SEE_MASK_INVOKEIDLIST: allows the "properties" verb (goes through the IDList).
    const SEE_MASK_INVOKEIDLIST: u32 = 0x0000_000C;
    const SW_SHOW: i32 = 5;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }

    let verb: Vec<u16> = OsStr::new("properties")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: zero-initialized structure then filled in per Win32 docs;
    // `verb`/`file` (NUL-terminated) stay alive for the duration of the call.
    let mut info: ShellExecuteInfoW = unsafe { std::mem::zeroed() };
    info.cb_size = std::mem::size_of::<ShellExecuteInfoW>() as u32;
    info.f_mask = SEE_MASK_INVOKEIDLIST;
    info.lp_verb = verb.as_ptr();
    info.lp_file = file.as_ptr();
    info.n_show = SW_SHOW;

    info!(path = %path.display(), "native properties dialog");
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        return Err(anyhow!(
            "ShellExecuteExW(properties) failed for {}",
            path.display()
        ));
    }
    Ok(())
}

/// Encodes `path` as a strict `file://` URI (RFC 3986) for D-Bus: anything
/// that isn't "unreserved" (`A-Z a-z 0-9 - . _ ~`) or the `/` separator is
/// percent-encoded. Deliberately STRICTER than `openers::file_uri` (which
/// only encodes spaces, good enough for editors): the GVariant literal
/// passed to `gdbus` must stay unambiguous even with `'`, `"`, `#`, or `?`.
#[cfg(all(unix, not(target_os = "macos")))]
fn dbus_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(b as char)
            }
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri
}

/// True if a file manager exposing `org.freedesktop.FileManager1`
/// is reachable (already running or activatable on demand via D-Bus). A
/// **fast** check (~6 ms measured) that queries the bus daemon itself — definitely
/// not `ShowItemProperties`, which only returns once the dialog is closed.
#[cfg(all(unix, not(target_os = "macos")))]
fn file_manager1_available() -> bool {
    // `ListActivatableNames` covers installed handlers (`.service`
    // file), `ListNames` those already running.
    ["ListActivatableNames", "ListNames"].iter().any(|method| {
        Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.DBus",
                "--object-path",
                "/org/freedesktop/DBus",
                "--method",
            ])
            .arg(format!("org.freedesktop.DBus.{method}"))
            .output()
            .ok()
            .filter(|out| out.status.success())
            .is_some_and(|out| {
                String::from_utf8_lossy(&out.stdout).contains("org.freedesktop.FileManager1")
            })
    })
}

/// **Desktop-native** properties dialog on Linux, via the freedesktop
/// interface `org.freedesktop.FileManager1.ShowItemProperties` — implemented
/// by Dolphin (KDE), Nautilus (GNOME), Nemo (Cinnamon), Caja (MATE)… We go
/// through `gdbus` (glib2, present on all common desktops): **no
/// dependency added**, compliant with the cargo-deny policy.
///
/// Two EMPIRICALLY VERIFIED constraints dictate this design:
///  - the call **blocks until the dialog closes** (observed with
///    Dolphin) → the process is launched without waiting for it, and a detached
///    thread reaps it (no zombie, no UI freeze);
///  - no handler is guaranteed (minimal session, bare WM) → a fast
///    pre-check, and `Err` otherwise so the caller falls back to the internal panel.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn show_native_properties(path: &Path) -> Result<()> {
    if !file_manager1_available() {
        return Err(anyhow!(
            "org.freedesktop.FileManager1 unavailable (no desktop file manager)"
        ));
    }
    // GVariant literal: array of URIs (the URI is percent-encoded → safe within
    // quotes), then the startup identifier, empty.
    let uris = format!("[\"{}\"]", dbus_file_uri(path));
    info!(path = %path.display(), "native properties dialog (FileManager1)");
    let child = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItemProperties",
        ])
        .arg(&uris)
        .arg("")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("spawning gdbus (properties): {e}"))?;
    // The child lives as long as the dialog is open: we reap it separately
    // to avoid leaving a zombie, without ever blocking the UI thread.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

/// Platforms with no usable native properties dialog (macOS): the failure
/// is reported to the user, like for any missing desktop integration.
#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn show_native_properties(_path: &Path) -> Result<()> {
    Err(anyhow!(
        "native properties dialog not supported on this platform"
    ))
}
