use super::*;

#[implement(IDropTarget)]
pub(super) struct FavnyrDropTarget {
    pub(super) handler: DropHandler,
    /// Cheap format classification retained between `DragEnter` and `Drop`.
    /// No source-owned data is rendered until the user actually drops it.
    pub(super) incoming_kind: Cell<IncomingDataKind>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum IncomingDataKind {
    ShellPaths,
    ApplicationPaths,
    VirtualFiles,
    #[default]
    None,
}

impl IncomingDataKind {
    fn accepted(self) -> bool {
        !matches!(self, Self::None)
    }

    fn copy_only(self) -> bool {
        matches!(self, Self::ApplicationPaths | Self::VirtualFiles)
    }
}

impl FavnyrDropTarget {
    fn ctrl_down(keys: MODIFIERKEYS_FLAGS) -> bool {
        // MK_CONTROL, defined in winuser.h. The generated type doesn't provide
        // a dedicated constant across every windows-rs feature combination.
        keys.0 & 0x0008 != 0
    }

    pub(super) fn choose_copy_effect(
        allowed: windows::Win32::System::Ole::DROPEFFECT,
        accepted: bool,
    ) -> windows::Win32::System::Ole::DROPEFFECT {
        if accepted && allowed.0 & DROPEFFECT_COPY.0 != 0 {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    }

    /// Selects COPY only when the source actually offered it. `pdwEffect` is
    /// an input/output parameter: returning an effect outside the input mask
    /// violates the OLE contract and can produce misleading cursor feedback.
    fn set_effect(effect: *mut windows::Win32::System::Ole::DROPEFFECT, accepted: bool) -> bool {
        if effect.is_null() {
            return false;
        }
        unsafe {
            *effect = Self::choose_copy_effect(*effect, accepted);
            *effect == DROPEFFECT_COPY
        }
    }

    fn effect_value(effect: *mut windows::Win32::System::Ole::DROPEFFECT) -> u32 {
        if effect.is_null() {
            0
        } else {
            unsafe { (*effect).0 }
        }
    }
}

impl IDropTarget_Impl for FavnyrDropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let kind = data
            .as_ref()
            .map(classify_incoming_data)
            .unwrap_or_default();
        self.incoming_kind.set(kind);
        let accepted = FavnyrDropTarget::set_effect(effect, kind.accepted());
        if accepted {
            (self.handler)(IncomingFileDrag::Hover {
                screen_x: point.x,
                screen_y: point.y,
                copy: kind.copy_only() || FavnyrDropTarget::ctrl_down(keys),
            });
        }
        Ok(())
    }

    fn DragOver(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let kind = self.incoming_kind.get();
        let accepted = FavnyrDropTarget::set_effect(effect, kind.accepted());
        if accepted {
            (self.handler)(IncomingFileDrag::Hover {
                screen_x: point.x,
                screen_y: point.y,
                copy: kind.copy_only() || FavnyrDropTarget::ctrl_down(keys),
            });
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.incoming_kind.set(IncomingDataKind::None);
        (self.handler)(IncomingFileDrag::Leave);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let allowed_effect = FavnyrDropTarget::effect_value(effect);
        let kind = self.incoming_kind.replace(IncomingDataKind::None);
        let mut paths = Vec::new();
        let mut staging = None;
        let copy_allowed = allowed_effect & DROPEFFECT_COPY.0 != 0;
        if copy_allowed && let Some(data) = data.as_ref() {
            match kind {
                IncomingDataKind::ShellPaths => {
                    paths = file_paths(data);
                }
                IncomingDataKind::ApplicationPaths => {
                    let rendered_paths = file_paths(data);
                    if let Some(captured) = capture_application_paths(&rendered_paths) {
                        paths = captured.paths;
                        staging = Some(DropStaging {
                            temp_dir: captured.temp_dir,
                            copy_from_staging: captured.used_hard_links,
                        });
                    }
                }
                IncomingDataKind::VirtualFiles => {
                    if let Some(materialized) = materialize_virtual_files(data) {
                        paths = materialized.paths;
                        staging = Some(DropStaging {
                            temp_dir: materialized.temp_dir,
                            copy_from_staging: false,
                        });
                    }
                }
                IncomingDataKind::None => {}
            }
            // Some providers advertise CF_HDROP but render it only
            // conditionally. Keep the already-working email-attachment route as a
            // fallback when usable paths were not returned at Drop time.
            if paths.is_empty()
                && staging.is_none()
                && has_virtual_files(data)
                && let Some(materialized) = materialize_virtual_files(data)
            {
                paths = materialized.paths;
                staging = Some(DropStaging {
                    temp_dir: materialized.temp_dir,
                    copy_from_staging: false,
                });
            }
        }
        let accepted = FavnyrDropTarget::set_effect(effect, !paths.is_empty());
        if accepted {
            (self.handler)(IncomingFileDrag::Drop {
                paths,
                screen_x: point.x,
                screen_y: point.y,
                copy: kind.copy_only() || FavnyrDropTarget::ctrl_down(keys),
                staging,
            });
        } else if copy_allowed && kind.copy_only() {
            (self.handler)(IncomingFileDrag::ExternalDropFailed);
        } else {
            (self.handler)(IncomingFileDrag::Leave);
        }
        Ok(())
    }
}
