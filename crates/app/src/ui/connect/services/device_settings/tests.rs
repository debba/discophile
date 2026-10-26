use super::*;
use crate::ui::connect::services::tests::{fixture as base_fixture, input, setup};
use discophile_core::connect::Entitlement;
use discophile_core::l10n::{self, Language};

fn fixture() -> App {
    let mut app = base_fixture();
    app.connect.services.devices.truncate(1);
    let id = app.connect.services.devices[0].id;
    app.connect.services.devices[0].name = "Studio".into();
    let state = app.connect.services.states.get_mut(&id).unwrap();
    state.captured_at = discophile_core::now_millis();
    state.device_kind = service::DeviceKind::Linux;
    for id in ["playback.handoff", "playback.remote"] {
        app.connect.account.as_mut().unwrap().plan.entitlements.push(Entitlement { id: id.into(), available: true, limit: None });
    }
    app
}

fn render(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(input(ctx, events), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| show(app, ui, false));
    })
}

#[test]
fn settings_controls_are_named_compact_and_have_large_targets_in_both_languages_and_themes() {
    for language in Language::ALL {
        l10n::set_language(language);
        for dark in [false, true] {
            for zoom in [1.0, 1.5, 2.0] {
                let mut app = fixture();
                // Remote controls also work for content that cannot be handed off.
                app.connect.services.states.values_mut().next().unwrap().can_handoff = false;
                let ctx = setup(&app, dark, zoom);
                let mut tree = None;
                for _ in 0..3 {
                    let mut output = render(&mut app, &ctx, vec![]);
                    output.textures_delta.clear();
                    tree = output.platform_output.accesskit_update;
                }
                let tree = tree.unwrap();
                for (key, role) in [("Pause on %@", egui::Role::Button), ("Previous on %@", egui::Role::Button),
                    ("Next on %@", egui::Role::Button), ("Device options: %@", egui::Role::Button),
                    ("Volume on %@", egui::Role::Slider), ("Playback position on %@", egui::Role::Slider)] {
                    let name = tf(key, &[&"Studio"]);
                    let node = tree.nodes.iter().map(|(_, n)| n).find(|n| n.label() == Some(&name)).unwrap_or_else(|| panic!("missing {name}"));
                    assert_eq!(node.role(), role);
                    assert!(!node.is_disabled());
                    let bounds = node.bounds().unwrap();
                    assert!(bounds.height() >= 44.0, "{name}: {bounds:?}");
                    assert!(bounds.x0 >= 0.0 && bounds.x1 <= 900.0 / zoom as f64, "{name}: {bounds:?}");
                }
                for key in ["Play on %@", "Set device volume", "Seek on device", "Revoke device access"] {
                    let name = if key.contains("%@") { tf(key, &[&"Studio"]) } else { t(key) };
                    assert!(!tree.nodes.iter().any(|(_, n)| n.label() == Some(&name)), "hide duplicate transport and advanced actions: {name}");
                }
                assert!(tree.nodes.iter().any(|(_, n)| n.role() == egui::Role::Image && n.label() == Some("Linux")));
            }
        }
    }
    l10n::set_language(Language::English);
}

#[test]
fn free_offline_suspended_and_stale_devices_do_not_offer_live_remote_controls() {
    l10n::set_language(Language::English);
    for scenario in ["free", "offline", "suspended", "stale"] {
        let mut app = fixture();
        match scenario {
            "free" => app.connect.account.as_mut().unwrap().plan.entitlements.clear(),
            "offline" => app.connect.services.devices[0].online = false,
            "suspended" => app.connect.services.devices[0].suspended = true,
            _ => app.connect.services.states.values_mut().next().unwrap().captured_at = 0,
        }
        let ctx = setup(&app, true, 1.0);
        let mut output = render(&mut app, &ctx, vec![]);
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        for label in ["Pause on Studio", "Volume on Studio", "Playback position on Studio"] {
            assert!(tree.nodes.iter().filter(|(_, n)| n.label() == Some(label)).all(|(_, n)| n.is_disabled()), "{scenario}: {label}");
        }
    }
}

#[test]
fn all_four_device_icons_use_announced_platform_metadata_with_custom_names() {
    l10n::set_language(Language::English);
    let mut app = fixture();
    let state = app.connect.services.states.values().next().unwrap().clone();
    let device = app.connect.services.devices[0].clone();
    app.connect.services.devices.clear();
    for (index, kind) in [service::DeviceKind::Linux, service::DeviceKind::Mac, service::DeviceKind::Windows, service::DeviceKind::Smartphone].into_iter().enumerate() {
        let mut device = device.clone();
        device.id = Uuid::new_v4();
        device.name = format!("Room {index}");
        let mut state = state.clone();
        state.device_kind = kind;
        app.connect.services.states.insert(device.id, state);
        app.connect.services.devices.push(device);
    }
    let ctx = setup(&app, true, 1.0);
    let mut output = render(&mut app, &ctx, vec![]);
    output.textures_delta.clear();
    let tree = output.platform_output.accesskit_update.unwrap();
    for label in ["Linux", "macOS", "Windows", "Smartphone"] {
        assert!(tree.nodes.iter().any(|(_, n)| n.role() == egui::Role::Image && n.label() == Some(label)));
    }
}

#[test]
fn device_options_menu_opens_with_keyboard_and_escape_restores_focus() {
    let mut app = fixture();
    let ctx = setup(&app, true, 1.0);
    let key = |key| vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }];
    for _ in 0..3 { render(&mut app, &ctx, vec![]).drop_without_applying_deltas(); }
    render(&mut app, &ctx, key(egui::Key::Tab)).drop_without_applying_deltas();
    let trigger = ctx.memory(|memory| memory.focused());
    assert!(trigger.is_some());
    render(&mut app, &ctx, key(egui::Key::Enter)).drop_without_applying_deltas();
    assert!(egui::Popup::is_any_open(&ctx));
    render(&mut app, &ctx, key(egui::Key::ArrowDown)).drop_without_applying_deltas();
    render(&mut app, &ctx, vec![]).drop_without_applying_deltas();
    assert_ne!(ctx.memory(|memory| memory.focused()), trigger);
    render(&mut app, &ctx, key(egui::Key::Escape)).drop_without_applying_deltas();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(ctx.memory(|memory| memory.focused()), trigger);
    assert!(app.connect.services.revoke_target.is_none());
}

#[test]
fn remote_slider_keeps_its_draft_during_poll_updates_and_commits_on_release_or_keyboard() {
    let app = fixture();
    let ctx = setup(&app, true, 1.0);
    let draw = |value, events| {
        let mut result = None;
        let output = ctx.run_ui(input(&ctx, events), |ui| {
            result = slider(ui, "Volume", "Volume on Studio", value, 0.0..=100.0, |v| format!("{v:.0}%"));
        });
        (output, result)
    };
    let (mut output, _) = draw(50.0, vec![]);
    output.textures_delta.clear();
    let tree = output.platform_output.accesskit_update.unwrap();
    let (slider_id, node) = tree.nodes.iter().find(|(_, n)| n.role() == egui::Role::Slider).unwrap();
    let slider_id = *slider_id;
    assert_eq!(node.min_numeric_value(), Some(0.0));
    assert_eq!(node.max_numeric_value(), Some(100.0));
    let bounds = node.bounds().unwrap();
    let point = |fraction| egui::pos2((bounds.x0 + bounds.width() * fraction) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    draw(50.0, vec![egui::Event::PointerMoved(point(0.5))]).0.drop_without_applying_deltas();
    let button = |pressed, pos| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    let (output, change) = draw(50.0, vec![button(true, point(0.5))]);
    output.drop_without_applying_deltas();
    assert!(change.is_none());
    let (output, change) = draw(50.0, vec![egui::Event::PointerMoved(point(0.8))]);
    output.drop_without_applying_deltas();
    assert!(change.is_none());
    let (output, change) = draw(10.0, vec![button(false, point(0.8))]);
    output.drop_without_applying_deltas();
    let value = change.expect("release applies the draft even after an older presence value arrived");
    assert!(value > 70.0 && value < 90.0, "{value}");
    let (output, change) = draw(value, vec![egui::Event::Key {
        key: egui::Key::ArrowLeft, physical_key: None, pressed: true, repeat: false, modifiers: Default::default(),
    }]);
    output.drop_without_applying_deltas();
    assert!(change.is_some_and(|new| new < value));
    let keyboard_value = change.unwrap();
    let (output, change) = draw(keyboard_value, vec![egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Increment, target_node: slider_id, target_tree: egui::accesskit::TreeId::ROOT, data: None,
    })]);
    output.drop_without_applying_deltas();
    assert_eq!(change, Some(keyboard_value + 5.0));
    let (output, change) = draw(value, vec![egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::SetValue, target_node: slider_id, target_tree: egui::accesskit::TreeId::ROOT, data: Some(egui::accesskit::ActionData::NumericValue(73.0)),
    })]);
    output.drop_without_applying_deltas();
    assert_eq!(change, Some(73.0));
}
