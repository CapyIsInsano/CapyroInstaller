//! Capyro - Installer
//!
//! Instalador de Capyro para Windows, con la misma cara que Capyro: elegís
//! dónde se instala y dónde va la configuración, tocás Instalar y listo.
//! Con `--uninstall` (lo que corre "Aplicaciones instaladas") lo desinstala.
//!
//! Fuera de Windows se compila igual, pero en "simulación": instala en una
//! carpeta de prueba (sirve para trabajar en la interfaz).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod errlog;
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
    errlog::install_panic_hook();
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

    // Windows: primero OpenGL y, si la placa/driver no lo soporta, wgpu
    // (Direct3D 12). Con --wgpu o --opengl se fuerza uno. Linux: OpenGL.
    #[cfg(windows)]
    let renderers: Vec<eframe::Renderer> = if args.iter().any(|a| a == "--wgpu") {
        vec![eframe::Renderer::Wgpu]
    } else if args.iter().any(|a| a == "--opengl") {
        vec![eframe::Renderer::Glow]
    } else {
        vec![eframe::Renderer::Glow, eframe::Renderer::Wgpu]
    };
    #[cfg(not(windows))]
    let renderers = vec![eframe::Renderer::Glow];
    errlog::log(&format!(
        "CapyroInstaller {} · abriendo la ventana ({renderers:?})",
        env!("CARGO_PKG_VERSION")
    ));

    let mut errors = Vec::new();
    for renderer in renderers {
        let result = eframe::run_native(
            "Capyro - Installer",
            native_options(uninstall, renderer),
            Box::new(move |cc| Ok(Box::new(ui::InstallerApp::new(cc, mode)))),
        );
        match result {
            Ok(()) => {
                errlog::log(&format!("cerrado bien ({renderer:?})"));
                return Ok(());
            }
            Err(e) => {
                let msg = format!("{renderer:?}: {e}");
                errlog::log(&format!("no se pudo abrir la ventana con {msg}"));
                errors.push(msg);
            }
        }
    }
    errlog::fatal(&format!(
        "No se pudo abrir la ventana del instalador.\n\n{}\n\nProbá actualizar los drivers de video.",
        errors.join("\n")
    ));
    std::process::exit(1);
}

fn native_options(uninstall: bool, renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if uninstall { "Capyro - Uninstaller" } else { "Capyro - Installer" })
        .with_app_id("CapyroInstaller")
        .with_inner_size([560.0, 640.0])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_decorations(false)
        // ventana transparente (esquinas redondeadas) solo en Linux: en
        // Windows muchos drivers no la soportan y la ventana no aparece
        .with_transparent(!cfg!(windows));
    if let Some(i) = icon() {
        viewport = viewport.with_icon(Arc::new(i));
    }
    eframe::NativeOptions { viewport, centered: true, renderer, ..Default::default() }
}
