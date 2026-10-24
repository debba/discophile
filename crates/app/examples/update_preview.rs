//! Native UI rehearsal in a separate profile. Never downloads or replaces an application.
//! DISCOPHILE_DATA_DIR and DISCOPHILE_CACHE_DIR must point to disposable preview folders.

#[cfg(debug_assertions)]
fn main() -> eframe::Result {
    use discophile::features::software_update::UpdateStatus;
    use discophile::platform;
    use discophile_core::updates::Release;

    assert!(std::env::var_os("DISCOPHILE_DATA_DIR").is_some(), "A separate preview profile is required");
    assert!(std::env::var_os("DISCOPHILE_CACHE_DIR").is_some(), "A separate preview cache is required");
    let mut settings = discophile::settings::Settings::load();
    settings.check_for_updates = false;
    settings.save();
    let restart_marker = std::path::PathBuf::from(std::env::var_os("DISCOPHILE_DATA_DIR").unwrap()).join("preview-restarted");
    let restarted = restart_marker.exists();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Discophile · Prova aggiornamenti (simulazione)")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/AppIcon.png")).unwrap()),
        ..Default::default()
    };
    eframe::run_native("Discophile Update Preview", options, Box::new(move |cc| {
        let mut app = discophile::App::new(cc);
        app.software_update.preview_install = Some(simulate_install);
        app.radio.stations.push(discophile_core::InternetRadioStation::new(
            None, "Anteprima · nessuna riproduzione", "https://example.invalid/preview", false,
        ).unwrap());
        app.nav.reset(discophile::nav::Route::Settings(discophile::nav::SettingsPage::About));
        let release = Release {
            version: "0.1.2".into(),
            page_url: format!("{}/releases", platform::GITHUB),
            notes: "# Discophile 0.1.2\n\n> Prova desktop: versione e note di esempio.\n\n## Novità\n\n- **Aggiornamenti con un clic.** Download, installazione e riavvio dall’app.\n- **Avviso all’apertura** e icona sempre disponibile nella barra, anche dopo *Più tardi*.\n\n## Miglioramenti\n\n- Changelog con titoli, elenchi, **grassetto**, `codice` e [link alle PR](https://github.com/debba/discophile/pulls).\n- GitHub in fondo alla barra laterale e in **Impostazioni → Informazioni**.\n\n---\n\nLe note reali saranno quelle delle release pubblicate, con le PR integrate.\n".into(),
            assets: vec![],
        };
        if restarted {
            app.software_update.changelog = Some(release);
            app.software_update.status = UpdateStatus::UpToDate;
        } else {
            app.software_update.status = UpdateStatus::Available(release.clone());
            app.software_update.available = Some(release);
        }
        Ok(Box::new(app))
    }))
}

#[cfg(debug_assertions)]
fn simulate_install(app: &mut discophile::App) {
    use discophile::features::software_update::UpdateStatus;
    use discophile::tasks::update;

    app.software_update.dialog = true;
    app.software_update.error = None;
    app.software_update.notice_dismissed = true;
    app.software_update.status = UpdateStatus::Downloading { received: 0, total: 100 };
    let tasks = app.tasks.clone();
    app.tasks.spawn(async move {
        for received in (5..=100).step_by(5) {
            discophile::platform::sleep_ms(300).await;
            tasks.send(update(move |app| app.software_update.status = UpdateStatus::Downloading { received, total: 100 }));
        }
        tasks.send(update(|app| app.software_update.status = UpdateStatus::Installing));
        discophile::platform::sleep_ms(1_000).await;
        tasks.send(update(|app| app.software_update.status = UpdateStatus::Restarting));
        discophile::platform::sleep_ms(700).await;
        update(|app| {
            let marker = std::path::PathBuf::from(std::env::var_os("DISCOPHILE_DATA_DIR").unwrap()).join("preview-restarted");
            let result = std::fs::write(marker, b"1").and_then(|_| std::env::current_exe()).and_then(|executable| {
                std::process::Command::new(executable).env("DISCOPHILE_UPDATE_PREVIEW_RESTARTED", "1").spawn()
            });
            match result {
                Ok(_) => app.request_quit(),
                Err(error) => {
                    app.software_update.status = UpdateStatus::Available(app.software_update.available.clone().unwrap());
                    app.software_update.error = Some(error.to_string());
                }
            }
        })
    });
}

#[cfg(not(debug_assertions))]
fn main() { panic!("The update preview is only available in debug builds."); }
