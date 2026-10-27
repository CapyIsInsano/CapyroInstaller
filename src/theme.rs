//! Colores y letras: los mismos de Capyro.

use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};

pub const BG_WINDOW: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x20);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x22, 0x23, 0x28);
pub const BG_DEEP: Color32 = Color32::from_rgb(0x14, 0x15, 0x19);
pub const TRACK: Color32 = Color32::from_rgb(0x3A, 0x3B, 0x40);
pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xEE, 0xF0);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xA8, 0xA9, 0xAE);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6E, 0x6F, 0x75);
pub const ICON: Color32 = Color32::from_rgb(0xD6, 0xD7, 0xDB);
pub const ERROR: Color32 = Color32::from_rgb(0xF0, 0x5A, 0x4F);
/// Naranja de Capyro.
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x74, 0x17);
/// El botón Instalar con el mouse encima.
pub const YELLOW: Color32 = Color32::from_rgb(0xFF, 0xC9, 0x3C);
/// Gris del botón Instalar en reposo.
pub const BUTTON_GRAY: Color32 = Color32::from_rgb(0x3D, 0x3E, 0x44);
/// La barra de progreso.
pub const GREEN: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x7A);

/// En Windows la ventana no es transparente: esquinas rectas.
pub const WINDOW_RADIUS: u8 = if cfg!(windows) { 0 } else { 14 };

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn apply_visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG_WINDOW;
    v.window_fill = BG_PANEL;
    v.selection.bg_fill = ACCENT;
    v.override_text_color = Some(TEXT);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.set_theme(egui::Theme::Dark);
}

/// Linux: DejaVu Sans. Windows: Segoe UI (+ Segoe UI Symbol para ▸, ✓...).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    #[cfg(not(windows))]
    let (regular, bold, symbols): (&[&str], &[&str], &[&str]) = (
        &[
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ],
        &[
            "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ],
        &[],
    );
    #[cfg(windows)]
    let (regular, bold, symbols): (&[&str], &[&str], &[&str]) =
        (&["segoeui.ttf", "arial.ttf"], &["segoeuib.ttf", "arialbd.ttf"], &["seguisym.ttf"]);

    let load = |list: &[&str]| {
        list.iter().find_map(|p| {
            #[cfg(windows)]
            let p = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                .join("Fonts")
                .join(p);
            std::fs::read(p).ok()
        })
    };
    let mut prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(b) = load(regular) {
        fonts.font_data.insert("inst-regular".into(), Arc::new(FontData::from_owned(b)));
        prop.insert(0, "inst-regular".into());
    }
    let mut bold_fam = prop.clone();
    if let Some(b) = load(bold) {
        fonts.font_data.insert("inst-bold".into(), Arc::new(FontData::from_owned(b)));
        bold_fam.insert(0, "inst-bold".into());
    }
    if let Some(b) = load(symbols) {
        fonts.font_data.insert("inst-symbols".into(), Arc::new(FontData::from_owned(b)));
        prop.insert(1.min(prop.len()), "inst-symbols".into());
        bold_fam.insert(1.min(bold_fam.len()), "inst-symbols".into());
    }
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    ctx.set_fonts(fonts);
}
