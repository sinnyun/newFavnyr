use super::*;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, IDataObject,
};
use windows::Win32::UI::Shell::{
    ASSOC_FILTER_NONE, ASSOC_FILTER_RECOMMENDED, BHID_DataObject, IAssocHandler, IShellItem,
    SHAssocEnumHandlers, SHCreateItemFromParsingName,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::core::{PCWSTR, PWSTR};

use crate::winutil::{hbitmap_to_rgba, wide};

/// Retrieves + frees a `PWSTR` allocated by the shell (CoTaskMem).
unsafe fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        if p.is_null() {
            return String::new();
        }
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        s
    }
}

/// Enumerates handlers for a `.ext` extension. `recommended_only` filters.
unsafe fn enum_keys(ext_dot: &str, recommended_only: bool) -> Vec<(String, String)> {
    unsafe {
        let ext = wide(ext_dot);
        let filter = if recommended_only {
            ASSOC_FILTER_RECOMMENDED
        } else {
            ASSOC_FILTER_NONE
        };
        let Ok(en) = SHAssocEnumHandlers(PCWSTR(ext.as_ptr()), filter) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            let mut slot: [Option<IAssocHandler>; 1] = [None];
            let mut fetched = 0u32;
            if en.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(h) = slot[0].take() else { break };
            let name = h.GetName().map(|p| take_pwstr(p)).unwrap_or_default();
            let ui = h.GetUIName().map(|p| take_pwstr(p)).unwrap_or_default();
            if !name.is_empty() {
                out.push((if ui.is_empty() { name.clone() } else { ui }, name));
            }
        }
        out
    }
}

pub fn handlers_for_ext(ext: &str, _include_without_mime: bool) -> Vec<AppHandler> {
    if ext.is_empty() {
        return Vec::new();
    }
    let ext_dot = format!(".{ext}");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // Recommended set (keys) used to mark `recommended`.
        let recommended: std::collections::HashSet<String> = enum_keys(&ext_dot, true)
            .into_iter()
            .map(|(_, k)| k)
            .collect();
        enum_keys(&ext_dot, false)
            .into_iter()
            .map(|(name, key)| {
                let exe = {
                    let p = Path::new(&key);
                    if p.is_file() { Some(key.clone()) } else { None }
                };
                AppHandler {
                    recommended: recommended.contains(&key),
                    name,
                    key,
                    exe,
                }
            })
            .collect()
    }
}

/// Launches handler `key` on `path`. Classic apps (exe) → `Command`;
/// otherwise re-enumerates the extension, finds the handler, and calls `Invoke`.
pub fn launch(key: &str, ext: &str, path: &Path) -> Result<()> {
    // Fast path: key = a valid exe path.
    if Path::new(key).is_file() {
        return crate::actions::spawn_program(
            Path::new(key),
            &[path.to_string_lossy().into_owned()],
        );
    }
    let ext_dot = format!(".{ext}");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let ext_w = wide(&ext_dot);
        let en = SHAssocEnumHandlers(PCWSTR(ext_w.as_ptr()), ASSOC_FILTER_NONE)
            .map_err(|e| anyhow::anyhow!("SHAssocEnumHandlers: {e}"))?;
        let path_w = wide(&path.to_string_lossy());
        loop {
            let mut slot: [Option<IAssocHandler>; 1] = [None];
            let mut fetched = 0u32;
            if en.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(h) = slot[0].take() else { break };
            let name = h.GetName().map(|p| take_pwstr(p)).unwrap_or_default();
            if name == key {
                let item: IShellItem =
                    SHCreateItemFromParsingName(PCWSTR(path_w.as_ptr()), None)
                        .map_err(|e| anyhow::anyhow!("SHCreateItemFromParsingName: {e}"))?;
                let data: IDataObject = item
                    .BindToHandler(None, &BHID_DataObject)
                    .map_err(|e| anyhow::anyhow!("BindToHandler: {e}"))?;
                h.Invoke(&data)
                    .map_err(|e| anyhow::anyhow!("Invoke: {e}"))?;
                return Ok(());
            }
        }
    }
    Err(anyhow::anyhow!("handler not found: {key}"))
}

/// Extracts the icon associated with `path` as RGBA pixels `(buf, w, h)` (or `None`).
/// `SHGetFileInfoW` (shell icon) → `GetIconInfo` → shared GDI extraction
/// (`winutil::hbitmap_to_rgba`).
pub fn icon_rgba(path: &str) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
    use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGetFileInfoW};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};
    if path.is_empty() || !Path::new(path).is_file() {
        return None;
    }
    unsafe {
        let wpath = wide(path);
        let mut shfi = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_SMALLICON,
        );
        if ok == 0 || shfi.hIcon.is_invalid() {
            return None;
        }
        let hicon = shfi.hIcon;
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            let _ = DestroyIcon(hicon);
            return None;
        }
        let out = hbitmap_to_rgba(ii.hbmColor);
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        let _ = DestroyIcon(hicon);
        out
    }
}

/// Icon associated with the EXTENSION `ext` (no dot) as RGBA pixels, WITHOUT
/// disk I/O. `SHGFI_USEFILEATTRIBUTES` resolves the registered icon from a fake
/// `x.<ext>` name + `FILE_ATTRIBUTE_NORMAL` (registry lookup, no file
/// access). `big=false` → 32 px (`SHGFI_LARGEICON`, list mode); `big=true`
/// → **256 px** via the system's JUMBO image list (preview mode, where the row can
/// grow up to ~216 px). Same GDI conversion (`hbitmap_to_rgba`).
pub fn icon_rgba_for_ext(ext: &str, big: bool) -> Option<(Vec<u8>, u32, u32)> {
    if ext.is_empty() {
        return None;
    }
    // Fake name + `use_attrs` → pure REGISTRY lookup, without touching disk.
    shell_icon(&format!("x.{ext}"), true, big)
}

/// Icon SPECIFIC to a file: same mechanism, but on the REAL PATH
/// and without `SHGFI_USEFILEATTRIBUTES` → the shell reads the file's resources.
/// Essential for `.exe` files (embedded icon, different per binary):
/// the "by extension" path can only return the generic registry icon.
/// Costs a disk I/O → the caller caches it (cf. `self_icon` on the bridge side).
pub fn icon_rgba_for_path(path: &str, big: bool) -> Option<(Vec<u8>, u32, u32)> {
    if path.is_empty() {
        return None;
    }
    shell_icon(path, false, big)
}

/// SHARED core of both paths: `SHGetFileInfoW` on `target`.
/// `use_attrs` = resolve from the name alone (registry, no I/O) vs reading the
/// file. `big` = 256 px via the JUMBO image list, otherwise 32 px directly.
fn shell_icon(target: &str, use_attrs: bool, big: bool) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES,
        SHGetFileInfoW, SHGetImageList, SHIL_JUMBO,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    unsafe {
        let wpath = wide(target);
        let attrs = if use_attrs {
            FILE_ATTRIBUTE_NORMAL
        } else {
            Default::default()
        };
        let mut flags = if big {
            SHGFI_SYSICONINDEX
        } else {
            SHGFI_ICON | SHGFI_LARGEICON
        };
        if use_attrs {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        let mut shfi = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            attrs,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if ok == 0 {
            return None;
        }
        if big {
            // 256 px: system index → shared JUMBO image list.
            let list: IImageList = SHGetImageList(SHIL_JUMBO as i32).ok()?;
            let hicon = list.GetIcon(shfi.iIcon, ILD_TRANSPARENT.0).ok()?;
            let out = hicon_to_rgba(hicon);
            let _ = DestroyIcon(hicon);
            out
        } else {
            if shfi.hIcon.is_invalid() {
                return None;
            }
            let out = hicon_to_rgba(shfi.hIcon);
            let _ = DestroyIcon(shfi.hIcon);
            out
        }
    }
}

/// `HICON` → RGBA pixels `(buf, w, h)`: `GetIconInfo` (color bitmap) → GDI.
unsafe fn hicon_to_rgba(hicon: HICON) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
        use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
        if hicon.is_invalid() {
            return None;
        }
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            return None;
        }
        let out = hbitmap_to_rgba(ii.hbmColor);
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        out
    }
}

/// Opens a native file picker (IFileOpenDialog) filtered on
/// executables. Returns the chosen path, or `None` (cancelled/error).
pub fn browse_for_exe(lang: Lang) -> Option<String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dlg: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let f_name = wide(&i18n::tr(lang, "ow_dialog_programs"));
        let f_spec = wide("*.exe;*.com;*.bat;*.cmd");
        let a_name = wide(&i18n::tr(lang, "ow_dialog_all_files"));
        let a_spec = wide("*.*");
        let filters = [
            COMDLG_FILTERSPEC {
                pszName: PCWSTR(f_name.as_ptr()),
                pszSpec: PCWSTR(f_spec.as_ptr()),
            },
            COMDLG_FILTERSPEC {
                pszName: PCWSTR(a_name.as_ptr()),
                pszSpec: PCWSTR(a_spec.as_ptr()),
            },
        ];
        let _ = dlg.SetFileTypes(&filters);
        if dlg.Show(None).is_err() {
            return None; // cancelled
        }
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        Some(take_pwstr(p))
    }
}

/// Resolves the TARGET of a Windows `.lnk` shortcut via `IShellLinkW` +
/// `IPersistFile::Load` (COM). Returns the raw target path (without
/// `Resolve`, which could search/display a UI). `None` if it's not
/// a valid link or the target is empty.
pub fn resolve_shortcut(path: &Path) -> Option<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
    use windows::core::Interface;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        let wpath = wide(&path.to_string_lossy());
        pf.Load(PCWSTR(wpath.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 260]; // MAX_PATH
        let mut fd = WIN32_FIND_DATAW::default();
        link.GetPath(&mut buf, &mut fd, SLGP_RAWPATH.0 as u32)
            .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if len == 0 {
            return None;
        }
        Some(std::path::PathBuf::from(OsString::from_wide(&buf[..len])))
    }
}

/// Creates a Windows `lnk_path` shortcut (`.lnk`) pointing to `target`, via
/// `IShellLinkW::SetPath` + `IPersistFile::Save` (COM). The shortcut's working
/// directory is the target's own directory.
pub fn create_shortcut(lnk_path: &Path, target: &Path) -> Result<()> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::Interface;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| anyhow::anyhow!("CoCreateInstance ShellLink: {e}"))?;
        let target_w = wide(&target.to_string_lossy());
        link.SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|e| anyhow::anyhow!("SetPath: {e}"))?;
        if let Some(dir) = target.parent() {
            let dir_w = wide(&dir.to_string_lossy());
            let _ = link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()));
        }
        let pf: IPersistFile = link
            .cast()
            .map_err(|e| anyhow::anyhow!("cast IPersistFile: {e}"))?;
        let lnk_w = wide(&lnk_path.to_string_lossy());
        pf.Save(PCWSTR(lnk_w.as_ptr()), true)
            .map_err(|e| anyhow::anyhow!("Save .lnk: {e}"))?;
        Ok(())
    }
}

/// Native picker to choose a shortcut's TARGET — `IFileOpenDialog`
/// filtered to "All files" (unlike `browse_for_exe`, which is restricted
/// to executables). Returns the chosen path, or `None`.
pub fn browse_for_target(lang: Lang) -> Option<String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dlg: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let a_name = wide(&i18n::tr(lang, "ow_dialog_all_files"));
        let a_spec = wide("*.*");
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(a_name.as_ptr()),
            pszSpec: PCWSTR(a_spec.as_ptr()),
        }];
        let _ = dlg.SetFileTypes(&filters);
        if dlg.Show(None).is_err() {
            return None; // cancelled
        }
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        Some(take_pwstr(p))
    }
}

/// Native FOLDER picker (`IFileOpenDialog` + `FOS_PICKFOLDERS`), for the
/// target of a shortcut to a folder. Returns the chosen path, or `None`.
pub fn browse_for_folder() -> Option<String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dlg: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().unwrap_or_default();
        let _ = dlg.SetOptions(opts | FOS_PICKFOLDERS);
        if dlg.Show(None).is_err() {
            return None; // cancelled
        }
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        Some(take_pwstr(p))
    }
}
