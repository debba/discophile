use super::*;
use crate::ui::connect::services::tests::{fixture, input, setup};
use discophile_core::{
    connect::Entitlement,
    l10n::{self, Language},
};

fn remote_fixture() -> App {
    let mut app = fixture();
    app.connect.account.as_mut().unwrap().plan.entitlements = ["playback.remote", "playback.handoff"]
        .map(|id| Entitlement {
            id: id.into(),
            available: true,
            limit: None,
        })
        .to_vec();
    let id = app.connect.services.devices[0].id;
    app.connect.services.remote_device = Some(id);
    app.connect.services.states.get_mut(&id).unwrap().captured_at = discophile_core::now_millis();
    app
}

fn render(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
    ctx.run_ui(input(ctx, vec![]), |ui| {
        egui::Panel::bottom("remote-bar-fixture")
            .exact_size(crate::ui::player_bar::height(app))
            .frame(
                egui::Frame::new()
                    .fill(Palette::of_ui(ui).surface)
                    .inner_margin(egui::Margin::symmetric(24, 14)),
            )
            .show(ui, |ui| crate::ui::player_bar::show(app, ui));
    })
}

#[test]
fn remote_bar_names_the_active_device_and_controls_in_both_languages_themes_and_large_text() {
    for language in Language::ALL {
        l10n::set_language(language);
        for dark in [false, true] {
            for zoom in [1.0, 1.5, 2.0] {
                let mut app = remote_fixture();
                let ctx = setup(&app, dark, zoom);
                let mut tree = None;
                for _ in 0..3 {
                    let mut output = render(&mut app, &ctx);
                    output.textures_delta.clear();
                    tree = output.platform_output.accesskit_update;
                }
                let tree = tree.unwrap();
                assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Status
                    && node.value() == Some(tf("Playing on %@", &[&"Linux"]).as_str())
                    && node.live() == Some(egui::accesskit::Live::Polite)));
                for key in ["Previous on %@", "Pause on %@", "Next on %@"] {
                    let label = tf(key, &[&"Linux"]);
                    let node = tree.nodes.iter().map(|(_, node)| node).find(|node| node.label() == Some(&label)).unwrap();
                    assert!(!node.is_disabled());
                    let bounds = node.bounds().unwrap();
                    assert!(bounds.width() >= 44.0 && bounds.height() >= 44.0);
                    assert!(
                        bounds.x0 >= 0.0 && bounds.x1 <= 900.0 / zoom as f64 && bounds.y1 <= 600.0 / zoom as f64,
                        "{zoom}: {bounds:?}"
                    );
                }
                let label = tf("Playback position on %@", &[&"Linux"]);
                let slider = tree.nodes.iter().map(|(_, node)| node).find(|node| node.label() == Some(&label)).unwrap();
                assert_eq!(slider.role(), egui::Role::Slider);
                assert!(slider.bounds().unwrap().y1 <= 600.0 / zoom as f64);
                assert!(!app.player.output.status().playing);
            }
        }
    }
    l10n::set_language(Language::English);
}

#[test]
fn offline_device_is_announced_and_remote_transport_is_disabled() {
    l10n::set_language(Language::English);
    let mut app = remote_fixture();
    app.connect.services.devices[0].online = false;
    let ctx = setup(&app, false, 1.0);
    let mut tree = None;
    for _ in 0..3 {
        let mut output = render(&mut app, &ctx);
        output.textures_delta.clear();
        tree = output.platform_output.accesskit_update;
    }
    let tree = tree.unwrap();
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.role() == egui::Role::Status && node.value().is_some_and(|value| value.contains("Linux is unavailable")))
    );
    for label in ["Pause on Linux", "Previous on Linux", "Next on Linux"] {
        assert!(tree.nodes.iter().any(|(_, node)| node.label() == Some(label) && node.is_disabled()));
    }
}
