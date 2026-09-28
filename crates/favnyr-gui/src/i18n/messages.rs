use super::*;

pub fn access_denied(lang: Lang) -> String {
    tr(lang, "access_denied")
}

/// Renders a diagnosed process list as one readable fragment. The ellipsis
/// marks a deliberately bounded diagnostic, so the sentence never claims to
/// name every holder. Empty when nothing could be attributed.
pub fn process_list<S: AsRef<str>>(lang: Lang, items: &[S], truncated: bool) -> String {
    let separator = tr(lang, "list_separator");
    let mut list = String::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            list.push_str(&separator);
        }
        list.push_str(item.as_ref());
    }
    if truncated && !list.is_empty() {
        list.push_str(&separator);
        list.push_str(&tr(lang, "list_ellipsis"));
    }
    list
}

/// Human-readable message after an operation is refused by a Windows lock.
/// The ownerless template stays honest when Restart Manager confirms the
/// conflict but can't inspect the process (permissions, folder handle).
pub fn item_in_use(lang: Lang, item: &str, processes: &[String], truncated: bool) -> String {
    let process = process_list(lang, processes, truncated);
    let key = if process.is_empty() {
        "op_in_use_unknown"
    } else {
        "op_in_use_by"
    };
    tr(lang, key)
        .replace("{item}", item)
        .replace("{process}", &process)
}

/// Message for an entry an operation could not process and stepped over. Used
/// when no holding program could be named: `reason` then carries the system
/// error, which stays the only thing known about the refusal.
pub fn item_skipped(lang: Lang, item: &str, reason: &str) -> String {
    tr(lang, "op_item_skipped")
        .replace("{item}", item)
        .replace("{reason}", reason)
}

/// Message for a move whose copy succeeded but whose original could not be
/// removed. The item then exists in both places, which the user has to be told
/// explicitly: the action was requested as a move and behaved as a copy.
/// `reason` names the holding process when one could be attributed, and
/// carries the system error otherwise.
pub fn move_source_kept(lang: Lang, item: &str, reason: &str) -> String {
    tr(lang, "move_source_kept")
        .replace("{item}", item)
        .replace("{reason}", reason)
}

/// Translated fallback when a rename fails without an attributable Windows
/// lock. `reason` keeps the detail provided by the OS (permissions, invalid
/// name, network…).
pub fn rename_failed(lang: Lang, item: &str, reason: &str) -> String {
    tr(lang, "rename_failed")
        .replace("{item}", item)
        .replace("{reason}", reason)
}

/// Translated reason for a device removal / network disconnection failure.
/// `err` carries no text of its own (see [`favnyr_core::eject::EjectError`]);
/// this is the only place that turns it into a sentence.
pub fn eject_error_message(lang: Lang, err: &favnyr_core::eject::EjectError) -> String {
    use favnyr_core::eject::EjectError;
    match err {
        EjectError::UnknownDevice => tr(lang, "eject_unknown_device"),
        EjectError::BlockedByProcesses(processes) => tr(lang, "eject_blocked_by_processes")
            .replace("{processes}", &process_list(lang, processes, false)),
        EjectError::BlockedByService => tr(lang, "eject_blocked_by_service"),
        EjectError::BlockedByApplication => tr(lang, "eject_blocked_by_application"),
        EjectError::BlockedByOpenFile => tr(lang, "eject_blocked_by_open_file"),
        EjectError::DeviceBusy => tr(lang, "eject_device_busy"),
        EjectError::ToolNotFound => tr(lang, "eject_tool_not_found"),
        EjectError::ToolFailed {
            bin,
            detail: Some(detail),
        } => tr(lang, "eject_tool_failed_detail")
            .replace("{bin}", bin)
            .replace("{detail}", detail),
        EjectError::ToolFailed { bin, detail: None } => {
            tr(lang, "eject_tool_failed").replace("{bin}", bin)
        }
        EjectError::DisconnectFailed(code) => {
            tr(lang, "eject_disconnect_failed").replace("{code}", &code.to_string())
        }
        EjectError::LinuxNetworkDisconnectUnsupported => {
            tr(lang, "eject_linux_network_unsupported")
        }
        EjectError::SystemQueryFailed => tr(lang, "eject_system_query_failed"),
        EjectError::PlatformUnsupported => tr(lang, "eject_platform_unsupported"),
    }
}

/// Translated reason for a mount failure. `err` carries no text of its own
/// (see [`favnyr_core::mount::MountError`]); this is the only place that turns
/// it into a sentence.
pub fn mount_error_message(lang: Lang, err: &favnyr_core::mount::MountError) -> String {
    use favnyr_core::mount::MountError;
    match err {
        MountError::UnknownDevice => tr(lang, "mount_unknown_device"),
        MountError::ToolNotFound => tr(lang, "mount_tool_not_found"),
        MountError::NotAuthorized => tr(lang, "mount_not_authorized"),
        MountError::AlreadyMounted => tr(lang, "mount_already_mounted"),
        MountError::NoMountPoint => tr(lang, "mount_no_mount_point"),
        MountError::ToolFailed {
            detail: Some(detail),
        } => tr(lang, "mount_failed_detail").replace("{detail}", detail),
        MountError::ToolFailed { detail: None } => tr(lang, "mount_failed"),
        MountError::PlatformUnsupported => tr(lang, "mount_platform_unsupported"),
    }
}

/// Translated reason for a trash-restore failure. `err` carries no text of
/// its own (see [`favnyr_core::fs::ops::TrashError`]); this is the only place
/// that turns it into a sentence.
pub fn trash_error_message(lang: Lang, err: &favnyr_core::fs::ops::TrashError) -> String {
    use favnyr_core::fs::ops::TrashError;
    match err {
        TrashError::ListFailed(detail) => {
            tr(lang, "trash_reason_list_failed").replace("{detail}", detail)
        }
        TrashError::ItemNotFound => tr(lang, "trash_reason_item_not_found"),
        TrashError::RestoreFailed(detail) => {
            tr(lang, "trash_reason_restore_failed").replace("{detail}", detail)
        }
        TrashError::PlatformUnsupported => tr(lang, "trash_reason_platform_unsupported"),
    }
}
