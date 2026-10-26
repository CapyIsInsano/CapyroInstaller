//! Capyro - Installer
//!
//! Instalador de Capyro para Windows, con la misma cara que Capyro: elegís
//! dónde se instala y dónde va la configuración, tocás Instalar y listo.
//! Con `--uninstall` (lo que corre "Aplicaciones instaladas") lo desinstala.
//!
//! Fuera de Windows se compila igual, pero en "simulación": instala en una
//! carpeta de prueba (sirve para trabajar en la interfaz).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod install;
mod sound;
mod theme;
mod ui;

use std::sync::Arc;

use eframe::egui;

const LOGO_PNG: &[u8] = include_bytes!("../assets/capyro_logo_256.png");

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width: w, height: h })
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uninstall = args.iter().any(|a| a == "--uninstall");
    // "--uninstall --quiet": sin ventana (lo usa Windows para desinstalar en silencio)
    if uninstall && args.iter().any(|a| a == "--quiet" || a == "/S") {
        let code = match install::installed() {
            Some((dir, _)) => match install::uninstall(&dir) {
                Ok(()) => {
                    install::delete_dir_later(&dir);
                    0
                }
                Err(e) => {
                    eprintln!("CapyroInstaller: {e}");
                    1
                }
            },
            None => 0,
        };
        std::process::exit(code);
    }
    let mode = if uninstall { ui::Mode::Uninstall } else { ui::Mode::Install };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if uninstall { "Capyro - Uninstaller" } else { "Capyro - Installer" })
        .with_app_id("CapyroInstaller")
        .with_inner_size([560.0, 640.0])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_decorations(false)
        .with_transparent(true);
    if let Some(i) = icon() {
        viewport = viewport.with_icon(Arc::new(i));
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native(
        "Capyro - Installer",
        options,
        Box::new(move |cc| Ok(Box::new(ui::InstallerApp::new(cc, mode)))),
    )
}
