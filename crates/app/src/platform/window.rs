pub fn can_hide(cc: &eframe::CreationContext<'_>) -> bool {
    #[cfg(target_os = "linux")]
    {
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        cc.display_handle().is_ok_and(|handle| matches!(handle.as_raw(), RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cc;
        true
    }
}

/// Fall back to minimizing when hiding is unsupported or no tray host is available.
pub fn handle_close(ctx: &egui::Context, minimize_on_close: bool, tray_available: bool, quitting: bool) -> bool {
    if minimize_on_close && !quitting && ctx.input(|input| input.viewport().close_requested()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if tray_available {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return true;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_intercepts_close_requests_when_enabled() {
        for enabled in [false, true] {
            for close_requested in [false, true] {
                for tray_available in [false, true] {
                    for quitting in [false, true] {
                        let ctx = egui::Context::default();
                        let mut input = egui::RawInput::default();
                        if close_requested {
                            input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
                        }
                        let mut hidden = false;
                        let mut output = ctx.run_ui(input, |ui| hidden = handle_close(ui.ctx(), enabled, tray_available, quitting));
                        output.textures_delta.clear();
                        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
                        let minimize = enabled && close_requested && !quitting;
                        assert_eq!(commands.contains(&egui::ViewportCommand::CancelClose), minimize);
                        assert_eq!(commands.contains(&egui::ViewportCommand::Minimized(true)), minimize && !tray_available);
                        assert_eq!(commands.contains(&egui::ViewportCommand::Visible(false)), minimize && tray_available);
                        assert_eq!(hidden, minimize && tray_available);
                    }
                }
            }
        }
    }
}
