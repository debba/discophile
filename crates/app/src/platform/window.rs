/// Keep the window in the desktop's task switcher and leave playback running.
pub fn handle_close(ctx: &egui::Context, minimize_on_close: bool) {
    if minimize_on_close && ctx.input(|input| input.viewport().close_requested()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_intercepts_close_requests_when_enabled() {
        for enabled in [false, true] {
            for close_requested in [false, true] {
                let ctx = egui::Context::default();
                let mut input = egui::RawInput::default();
                if close_requested {
                    input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
                }
                let mut output = ctx.run_ui(input, |ui| handle_close(ui.ctx(), enabled));
                output.textures_delta.clear();
                let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
                let minimize = enabled && close_requested;
                assert_eq!(commands.contains(&egui::ViewportCommand::CancelClose), minimize);
                assert_eq!(commands.contains(&egui::ViewportCommand::Minimized(true)), minimize);
            }
        }
    }
}
