//! La ventana del instalador, con la misma cara que Capyro.
//!
//! Pantallas: elegir carpetas → instalando (barra verde) → listo
//! ("¿Lanzar Capyro?" Sí/No). Con `--uninstall`: confirmar → desinstalado.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{
    self, epaint::RectShape, pos2, vec2, Align2, Color32, CursorIcon, Id, Key, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand,
};

use crate::install::{self, Msg, Plan};
use crate::sound;
use crate::theme::*;

const AVATAR_PNG: &[u8] = include_bytes!("../assets/arturo.png");
const AUTHOR: &str = "Arturo";
const GITHUB_USER: &str = "CapyIsInsano";
const GITHUB_URL: &str = "https://github.com/CapyIsInsano";
const CLAUDE_URL: &str = "https://www.anthropic.com/claude";
const INSTALLER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

enum Screen {
    Setup,
    Installing { started: f64 },
    Done { version: String, exe: PathBuf, at: f64 },
    Failed { message: String, at: f64 },
    UninstallConfirm { dir: Option<PathBuf>, config: Option<PathBuf> },
    Uninstalled { config: Option<PathBuf>, at: f64 },
}

/// Qué carpeta se está eligiendo.
#[derive(Clone, Copy)]
enum Pick {
    InstallDir,
    ConfigDir,
}

pub struct InstallerApp {
    screen: Screen,
    plan: Plan,
    logo: Option<TextureHandle>,
    avatar: Option<TextureHandle>,
    rx: Option<Receiver<Msg>>,
    picking: Option<(Pick, Receiver<Option<PathBuf>>)>,
    step: String,
    target: f32,
    shown: f32,
    /// Versión que trae el instalador adentro (None = baja la última).
    embedded: Option<String>,
    spark_angle: f32,
    spark_speed: f32,
    /// Lo que hay que hacer al terminar el frame (cerrar, lanzar...).
    close: bool,
    /// Carpeta de Capyro a borrar al cerrar (se desinstaló).
    delete_on_exit: Option<PathBuf>,
}

fn texture(ctx: &egui::Context, name: &str, png: &[u8]) -> Option<TextureHandle> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
    Some(ctx.load_texture(name, ci, TextureOptions::LINEAR))
}

fn uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Animación de botón: hover y press suaves, mano, y el clic de Capyro.
struct Anim {
    resp: egui::Response,
    hover: f32,
    press: f32,
}

fn button(ui: &Ui, id: Id, rect: Rect, enabled: bool) -> Anim {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let ctx = ui.ctx();
    let hovered = enabled && resp.hovered();
    let hover = ctx.animate_bool_with_time(id.with("h"), hovered, 0.14);
    let press = ctx.animate_bool_with_time(id.with("p"), enabled && resp.is_pointer_button_down_on(), 0.07);
    if hovered {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        sound::click();
    }
    Anim { resp, hover, press }
}

/// "C:\Users\...\Capyro" → recorta por el medio si no entra.
fn fit_path(p: &egui::Painter, text: &str, font: egui::FontId, max_w: f32) -> String {
    let w = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), TEXT).size().x;
    if w(text) <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 8 {
        keep -= 1;
        let head = keep * 2 / 5;
        let tail = keep - head;
        let s: String = chars[..head].iter().chain(['…'].iter()).chain(chars[chars.len() - tail..].iter()).collect();
        if w(&s) <= max_w {
            return s;
        }
    }
    text.chars().take(8).collect()
}

/* --- íconos chiquitos (dibujados, como en Capyro) --- */

fn icon_folder(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(c + vec2(0.0, s * 0.06), vec2(s * 0.84, s * 0.6));
    let st = Stroke::new(1.5, col);
    p.add(Shape::line(
        vec![
            pos2(r.left(), r.bottom()),
            pos2(r.left(), r.top() - s * 0.1),
            pos2(r.left() + s * 0.28, r.top() - s * 0.1),
            pos2(r.left() + s * 0.36, r.top()),
            pos2(r.right(), r.top()),
            pos2(r.right(), r.bottom()),
            pos2(r.left(), r.bottom()),
        ],
        st,
    ));
}

fn icon_file(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(c, vec2(s * 0.62, s * 0.8));
    let st = Stroke::new(1.5, col);
    let f = s * 0.2;
    p.add(Shape::line(
        vec![
            pos2(r.left(), r.top()),
            pos2(r.right() - f, r.top()),
            pos2(r.right(), r.top() + f),
            pos2(r.right(), r.bottom()),
            pos2(r.left(), r.bottom()),
            pos2(r.left(), r.top()),
        ],
        st,
    ));
    for i in 0..3 {
        let y = r.top() + s * (0.3 + 0.14 * i as f32);
        p.line_segment([pos2(r.left() + s * 0.12, y), pos2(r.right() - s * 0.12, y)], Stroke::new(1.2, col));
    }
}

fn icon_download(p: &egui::Painter, c: Pos2, s: f32, col: Color32, drop: f32) {
    let st = Stroke::new(2.0, col);
    let dy = s * 0.08 * drop;
    p.line_segment([c + vec2(0.0, -s * 0.36 + dy), c + vec2(0.0, s * 0.14 + dy)], st);
    p.add(Shape::line(vec![c + vec2(-s * 0.2, -s * 0.06 + dy), c + vec2(0.0, s * 0.14 + dy), c + vec2(s * 0.2, -s * 0.06 + dy)], st));
    p.line_segment([c + vec2(-s * 0.32, s * 0.36), c + vec2(s * 0.32, s * 0.36)], st);
}

fn icon_check(p: &egui::Painter, c: Pos2, s: f32, col: Color32, w: f32) {
    p.add(Shape::line(vec![c + vec2(-s * 0.3, 0.0), c + vec2(-s * 0.08, s * 0.24), c + vec2(s * 0.32, -s * 0.22)], Stroke::new(w, col)));
}

fn icon_cross(p: &egui::Painter, c: Pos2, s: f32, col: Color32, w: f32) {
    let st = Stroke::new(w, col);
    p.line_segment([c + vec2(-s * 0.3, -s * 0.3), c + vec2(s * 0.3, s * 0.3)], st);
    p.line_segment([c + vec2(-s * 0.3, s * 0.3), c + vec2(s * 0.3, -s * 0.3)], st);
}

fn arrow_out(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new(1.3, col);
    p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], st);
    p.line_segment([c + vec2(-s * 0.2, -s), c + vec2(s, -s)], st);
    p.line_segment([c + vec2(s, -s), c + vec2(s, s * 0.2)], st);
}

/// Destello de 4 puntas (el mismo del tutorial de Capyro).
fn sparkle(p: &egui::Painter, c: Pos2, r: f32, angle: f32, col: Color32) {
    for (rr, cc) in [(r + 0.9, col.gamma_multiply(0.35)), (r, col)] {
        let n = 96u32;
        let (sa, ca) = angle.sin_cos();
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, cc);
        for i in 0..n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            let (st, ct) = t.sin_cos();
            let x = rr * ct * ct.abs().powf(2.33);
            let y = rr * st * st.abs().powf(2.33);
            mesh.colored_vertex(pos2(c.x + x * ca - y * sa, c.y + x * sa + y * ca), cc);
        }
        for i in 1..=n {
            mesh.add_triangle(0, i, if i == n { 1 } else { i + 1 });
        }
        p.add(Shape::mesh(mesh));
    }
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe").arg(url).creation_flags(0x0800_0000).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

impl InstallerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mode: Mode) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_visuals(&cc.egui_ctx);
        let screen = match mode {
            Mode::Install => Screen::Setup,
            Mode::Uninstall => {
                let found = install::installed();
                Screen::UninstallConfirm {
                    dir: found.as_ref().map(|f| f.0.clone()),
                    config: found.and_then(|f| f.1),
                }
            }
        };
        Self {
            screen,
            plan: Plan {
                install_dir: install::default_install_dir(),
                config_path: install::default_config_path(),
                desktop_shortcut: true,
                start_menu: true,
            },
            logo: texture(&cc.egui_ctx, "logo", crate::LOGO_PNG),
            avatar: texture(&cc.egui_ctx, "avatar", AVATAR_PNG),
            rx: None,
            picking: None,
            step: String::new(),
            target: 0.0,
            shown: 0.0,
            embedded: install::embedded_version(),
            spark_angle: 0.0,
            spark_speed: 0.0,
            close: false,
            delete_on_exit: None,
        }
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let plan = self.plan.clone();
        let c = ctx.clone();
        std::thread::Builder::new()
            .name("instalar".into())
            .spawn(move || install::run(&plan, &tx, &move || c.request_repaint()))
            .expect("hilo del instalador");
        self.rx = Some(rx);
        self.step = "Preparando…".into();
        self.target = 0.0;
        self.shown = 0.0;
        self.screen = Screen::Installing { started: ctx.input(|i| i.time) };
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Step(s) => self.step = s,
                    Msg::Progress(k) => self.target = k.clamp(0.0, 1.0),
                    Msg::Done { version, exe } => {
                        self.target = 1.0;
                        self.screen = Screen::Done { version, exe, at: now + 0.45 };
                    }
                    Msg::Failed(message) => self.screen = Screen::Failed { message, at: now },
                }
            }
        }
        if let Some((what, rx)) = &self.picking {
            if let Ok(res) = rx.try_recv() {
                if let Some(dir) = res {
                    match what {
                        Pick::InstallDir => {
                            // "D:\Apps" → "D:\Apps\Capyro" (salvo que ya se llame Capyro)
                            let named = dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("capyro"));
                            self.plan.install_dir = if named { dir } else { dir.join("Capyro") };
                        }
                        Pick::ConfigDir => self.plan.config_path = dir.join("Capyro.lua"),
                    }
                }
                self.picking = None;
            }
        }
    }

    fn pick(&mut self, what: Pick) {
        if self.picking.is_some() {
            return;
        }
        let start = match what {
            Pick::InstallDir => self.plan.install_dir.parent().map(PathBuf::from),
            Pick::ConfigDir => self.plan.config_path.parent().map(PathBuf::from),
        };
        let title = match what {
            Pick::InstallDir => "¿Dónde instalo Capyro?",
            Pick::ConfigDir => "¿Dónde guardo la configuración (Capyro.lua)?",
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new().set_title(title);
            if let Some(s) = start.filter(|s| s.exists()) {
                d = d.set_directory(s);
            }
            let _ = tx.send(d.pick_folder());
        });
        self.picking = Some((what, rx));
    }
}

impl eframe::App for InstallerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(dir) = self.delete_on_exit.take() {
            install::delete_dir_later(&dir);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let full = ctx.content_rect();
        let p = ui.painter().clone();
        let now = ctx.input(|i| i.time);

        // ventana redondeada con el brillo naranja de Capyro arriba
        p.rect_filled(full, WINDOW_RADIUS, BG_WINDOW);
        {
            let c = pos2(full.center().x, full.top() + 40.0);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(c, Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 30));
            let n = 72;
            for i in 0..=n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                mesh.colored_vertex(c + vec2(a.cos() * 340.0, a.sin() * 220.0), Color32::TRANSPARENT);
            }
            for i in 1..=n as u32 {
                mesh.add_triangle(0, i, i + 1);
            }
            p.with_clip_rect(full.shrink(1.0)).add(Shape::mesh(mesh));
        }
        p.rect_stroke(full, WINDOW_RADIUS, Stroke::new(1.0, Color32::from_white_alpha(16)), StrokeKind::Inside);

        self.header(ui, &p, full);
        match &self.screen {
            Screen::Setup => self.setup(ui, &p, full),
            Screen::Installing { .. } => self.installing(ui, &p, full, now),
            Screen::Done { .. } => self.done(ui, &p, full, now),
            Screen::Failed { .. } => self.failed(ui, &p, full, now),
            Screen::UninstallConfirm { .. } => self.uninstall_confirm(ui, &p, full),
            Screen::Uninstalled { .. } => self.uninstalled(ui, &p, full, now),
        }
        if !matches!(self.screen, Screen::UninstallConfirm { .. } | Screen::Uninstalled { .. }) {
            self.credits(ui, &p, full, now);
        }

        // la barra verde avanza suave hasta donde va la instalación
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.shown += (self.target - self.shown) * (dt * 6.0).min(1.0);
        if (self.target - self.shown).abs() > 0.001
            || matches!(self.screen, Screen::Installing { .. })
            || self.spark_speed.abs() > 0.01
        {
            ctx.request_repaint();
        }
        if self.close {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

impl InstallerApp {
    /// Logo, "Capyro - Installer", minimizar y cerrar. Se arrastra de acá.
    fn header(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect) {
        let bar = Rect::from_min_size(full.min, vec2(full.width(), 70.0));
        let drag = ui.interact(bar, Id::unique("inst-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if let Some(logo) = &self.logo {
            p.image(logo.id(), Rect::from_min_size(pos2(full.left() + 24.0, full.top() + 16.0), Vec2::splat(42.0)), uv(), Color32::WHITE);
        }
        let title = if matches!(self.screen, Screen::UninstallConfirm { .. } | Screen::Uninstalled { .. }) {
            "Capyro - Uninstaller"
        } else {
            "Capyro - Installer"
        };
        p.text(pos2(full.left() + 76.0, full.top() + 30.0), Align2::LEFT_CENTER, title, bold(17.0), TEXT);
        p.text(pos2(full.left() + 76.0, full.top() + 50.0), Align2::LEFT_CENTER, "Just press play.", regular(11.5), TEXT_FAINT);

        let busy = matches!(self.screen, Screen::Installing { .. });
        for (i, close) in [(0, false), (1, true)] {
            let c = pos2(full.right() - 60.0 + 32.0 * i as f32, full.top() + 32.0);
            let r = Rect::from_center_size(c, Vec2::splat(28.0));
            let a = button(ui, Id::unique(("inst-win", i)), r, !(close && busy));
            if a.hover > 0.001 {
                let col = if close { ERROR.gamma_multiply(0.85 * a.hover) } else { Color32::from_white_alpha((22.0 * a.hover) as u8) };
                p.circle_filled(c, 14.0, col);
            }
            let col = if close && busy { TEXT_FAINT.gamma_multiply(0.5) } else { ICON.lerp_to_gamma(Color32::WHITE, a.hover) };
            if close {
                icon_cross(p, c, 14.0, col, 1.6);
            } else {
                p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], Stroke::new(1.6, col));
            }
            if a.resp.clicked() {
                if close {
                    self.close = true;
                } else {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
            }
        }
    }

    /// Caja con una ruta y el botón "Cambiar".
    #[allow(clippy::too_many_arguments)]
    fn path_box(&mut self, ui: &Ui, p: &egui::Painter, rect: Rect, label: &str, path: &str, file: bool, which: Pick) {
        p.text(rect.left_top() - vec2(0.0, 12.0), Align2::LEFT_CENTER, label, bold(10.5), TEXT_FAINT);
        p.rect_filled(rect, 10.0, BG_PANEL);
        p.rect_stroke(rect, 10.0, Stroke::new(1.0, Color32::from_white_alpha(12)), StrokeKind::Inside);
        let ic = pos2(rect.left() + 20.0, rect.center().y);
        if file {
            icon_file(p, ic, 18.0, ACCENT);
        } else {
            icon_folder(p, ic, 18.0, ACCENT);
        }
        let btn = Rect::from_center_size(pos2(rect.right() - 48.0, rect.center().y), vec2(78.0, 28.0));
        let shown = fit_path(p, path, regular(12.5), btn.left() - rect.left() - 46.0);
        p.text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, shown, regular(12.5), TEXT);
        let a = button(ui, Id::unique(("inst-pick", label)), btn, self.picking.is_none());
        let b = Rect::from_center_size(btn.center(), btn.size() * (1.0 + 0.03 * a.hover - 0.05 * a.press));
        p.rect_filled(b, 14.0, Color32::from_white_alpha((14.0 + 16.0 * a.hover) as u8));
        p.text(b.center(), Align2::CENTER_CENTER, "Cambiar", regular(12.0), TEXT.lerp_to_gamma(Color32::WHITE, a.hover));
        if a.resp.clicked() {
            self.pick(which);
        }
    }

    fn toggle(ui: &Ui, p: &egui::Painter, pos: Pos2, label: &str, on: &mut bool) {
        let rect = Rect::from_min_size(pos, vec2(260.0, 22.0));
        let a = button(ui, Id::unique(("inst-toggle", label)), rect, true);
        let k = ui.ctx().animate_bool_with_time(Id::unique(("inst-toggle-k", label)), *on, 0.12);
        let bx = Rect::from_center_size(pos2(pos.x + 9.0, rect.center().y), Vec2::splat(18.0));
        p.rect_filled(bx, 5.0, Color32::from_white_alpha((10.0 + 10.0 * a.hover) as u8).lerp_to_gamma(ACCENT, k));
        p.rect_stroke(bx, 5.0, Stroke::new(1.0, Color32::from_white_alpha(40).lerp_to_gamma(ACCENT, k)), StrokeKind::Inside);
        if k > 0.01 {
            let mut pc = p.clone();
            pc.multiply_opacity(k);
            icon_check(&pc, bx.center(), 14.0 * (0.7 + 0.3 * k), Color32::WHITE, 2.0);
        }
        p.text(pos2(bx.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, regular(13.0), TEXT_DIM.lerp_to_gamma(TEXT, a.hover));
        if a.resp.clicked() {
            *on = !*on;
        }
    }

    fn setup(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect) {
        let x0 = full.left() + 28.0;
        let x1 = full.right() - 28.0;

        // título y versión
        let tg = p.layout_no_wrap("Instalar Capyro".to_owned(), bold(26.0), TEXT);
        let tw = tg.size().x;
        p.galley(pos2(x0, full.top() + 86.0), tg, TEXT);
        let chip_text = match &self.embedded {
            Some(v) => format!("v{v}"),
            None => "última versión".to_owned(),
        };
        let cg = p.layout_no_wrap(chip_text, bold(11.5), ACCENT);
        let chip = Rect::from_min_size(pos2(x0 + tw + 12.0, full.top() + 94.0), vec2(cg.size().x + 18.0, 22.0));
        p.rect_filled(chip, 11.0, ACCENT.gamma_multiply(0.16));
        p.galley(chip.center() - cg.size() / 2.0, cg, ACCENT);
        p.text(
            pos2(x0, full.top() + 132.0),
            Align2::LEFT_CENTER,
            "Reproductor de audio y video: liviano, rápido y con un capibara de mascota.",
            regular(13.0),
            TEXT_DIM,
        );

        // dónde se instala y dónde va la config
        let dir = self.plan.install_dir.display().to_string();
        let cfg = self.plan.config_path.display().to_string();
        self.path_box(ui, p, Rect::from_min_max(pos2(x0, full.top() + 180.0), pos2(x1, full.top() + 222.0)), "DÓNDE SE INSTALA", &dir, false, Pick::InstallDir);
        self.path_box(ui, p, Rect::from_min_max(pos2(x0, full.top() + 254.0), pos2(x1, full.top() + 296.0)), "DÓNDE VA LA CONFIGURACIÓN", &cfg, true, Pick::ConfigDir);

        // opciones
        Self::toggle(ui, p, pos2(x0, full.top() + 314.0), "Acceso directo en el escritorio", &mut self.plan.desktop_shortcut);
        Self::toggle(ui, p, pos2(x0, full.top() + 342.0), "Acceso en el menú Inicio", &mut self.plan.start_menu);

        // Instalar: gris; con el mouse encima crece y se pone amarillo
        let c = pos2(full.center().x, full.top() + 412.0);
        let base = vec2(232.0, 52.0);
        let hit = Rect::from_center_size(c, base * 1.1);
        let a = button(ui, Id::unique("inst-install"), hit, self.picking.is_none());
        let scale = 1.0 + 0.09 * ease_out(a.hover) - 0.05 * a.press;
        let r = Rect::from_center_size(c, base * scale);
        if a.hover > 0.001 {
            p.rect_filled(r.expand(6.0 * a.hover), 32.0, YELLOW.gamma_multiply(0.16 * a.hover));
        }
        p.rect_filled(r.translate(vec2(0.0, 3.0)), 26.0 * scale, Color32::from_black_alpha(60));
        let fill = BUTTON_GRAY.lerp_to_gamma(YELLOW, a.hover);
        p.rect_filled(r, 26.0 * scale, fill);
        let fg = Color32::WHITE.lerp_to_gamma(BG_WINDOW, a.hover);
        let lg = p.layout_no_wrap("Instalar".to_owned(), bold(18.0 * scale), fg);
        let total_w = 22.0 + 10.0 + lg.size().x;
        let lx = r.center().x - total_w / 2.0;
        icon_download(p, pos2(lx + 11.0, r.center().y), 22.0 * scale, fg, a.hover);
        p.galley(pos2(lx + 32.0, r.center().y - lg.size().y / 2.0), lg, fg);
        if a.resp.clicked() {
            let ctx = ui.ctx().clone();
            self.start_install(&ctx);
        }
        let note = if self.embedded.is_some() {
            "Se instala solo para tu usuario: no pide permisos de administrador."
        } else {
            "Se baja la última versión de GitHub. No pide permisos de administrador."
        };
        p.text(pos2(full.center().x, full.top() + 462.0), Align2::CENTER_CENTER, note, regular(11.5), TEXT_FAINT);
        if ui.ctx().input(|i| i.key_pressed(Key::Enter)) && self.picking.is_none() {
            sound::click();
            let ctx = ui.ctx().clone();
            self.start_install(&ctx);
        }
    }

    fn installing(&mut self, _ui: &mut Ui, p: &egui::Painter, full: Rect, now: f64) {
        let x0 = full.left() + 28.0;
        let x1 = full.right() - 28.0;
        let Screen::Installing { started } = self.screen else { return };
        let k = ease_out(((now - started) / 0.35) as f32);
        let mut p = p.clone();
        p.multiply_opacity(k);
        p.text(pos2(x0, full.top() + 104.0), Align2::LEFT_CENTER, "Instalando Capyro…", bold(26.0), TEXT);
        let ver = match &self.embedded {
            Some(v) => format!("Versión {v}"),
            None => "La última versión para Windows".to_owned(),
        };
        p.text(pos2(x0, full.top() + 134.0), Align2::LEFT_CENTER, ver, regular(13.0), TEXT_DIM);

        // la barra verde
        let bar = Rect::from_min_max(pos2(x0, full.top() + 196.0), pos2(x1, full.top() + 218.0));
        p.rect_filled(bar, 11.0, TRACK);
        let w = (bar.width() * self.shown).max(bar.height());
        let fill = Rect::from_min_size(bar.min, vec2(w, bar.height()));
        p.rect_filled(fill, 11.0, GREEN);
        // brillo que recorre la barra
        let sweep = ((now * 0.7).fract() as f32) * (fill.width() + 120.0) - 60.0;
        let sh = Rect::from_center_size(pos2(fill.left() + sweep, fill.center().y), vec2(60.0, fill.height()));
        p.with_clip_rect(fill.shrink(1.0)).rect_filled(sh, 8.0, Color32::from_white_alpha(40));
        p.rect_filled(Rect::from_min_size(fill.min + vec2(6.0, 3.0), vec2((w - 12.0).max(0.0), 4.0)), 2.0, Color32::from_white_alpha(36));
        p.text(pos2(x1, bar.top() - 14.0), Align2::RIGHT_CENTER, format!("{:.0} %", self.shown * 100.0), bold(13.0), GREEN);
        let step = fit_path(&p, &self.step, regular(12.5), bar.width() - 60.0);
        p.text(pos2(x0, bar.top() - 14.0), Align2::LEFT_CENTER, step, regular(12.5), TEXT_DIM);

        // a dónde va
        let rows = [
            ("Capyro", self.plan.install_dir.display().to_string()),
            ("Configuración", self.plan.config_path.display().to_string()),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = full.top() + 262.0 + 24.0 * i as f32;
            p.text(pos2(x0, y), Align2::LEFT_CENTER, *k, regular(12.0), TEXT_FAINT);
            let v = fit_path(&p, v, regular(12.0), x1 - x0 - 110.0);
            p.text(pos2(x0 + 110.0, y), Align2::LEFT_CENTER, v, regular(12.0), TEXT_DIM);
        }
        if install::SIMULATION {
            p.text(pos2(x0, full.top() + 330.0), Align2::LEFT_CENTER, "(simulación: esto no es Windows, se instala en una carpeta de prueba)", regular(11.0), ACCENT.gamma_multiply(0.7));
        }
    }

    fn done(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect, now: f64) {
        let Screen::Done { version, exe, at } = &self.screen else { return };
        let (version, exe, at) = (version.clone(), exe.clone(), *at);
        let t = (now - at) as f32;
        if t < 0.0 {
            // la barra termina de llenarse antes de cambiar de pantalla
            self.installing(ui, p, full, now);
            ui.ctx().request_repaint();
            return;
        }
        let cx = full.center().x;
        // tilde verde que salta
        let pop = ease_out(t / 0.4);
        let bounce = (-t * 4.0).exp() * (t * 11.0).sin().abs() * 8.0;
        let c = pos2(cx, full.top() + 132.0 - bounce);
        let ring = (t / 0.8).clamp(0.0, 1.0);
        if ring < 1.0 {
            p.circle_stroke(c, 34.0 + 26.0 * ring, Stroke::new(3.0, GREEN.gamma_multiply(1.0 - ring)));
        }
        p.circle_filled(c, 34.0 * pop, GREEN);
        icon_check(p, c, 40.0 * pop, Color32::WHITE, 4.0);
        if t < 1.0 {
            ui.ctx().request_repaint();
        }

        let mut pt = p.clone();
        pt.multiply_opacity(ease_out((t - 0.15) / 0.35));
        pt.text(pos2(cx, full.top() + 196.0), Align2::CENTER_CENTER, "¡Listo!", bold(28.0), TEXT);
        // "Capyro 0.3.0-alpha quedó instalado."
        let a = pt.layout_no_wrap("Capyro ".to_owned(), regular(14.5), TEXT_DIM);
        let b = pt.layout_no_wrap(version, bold(14.5), ACCENT);
        let d = pt.layout_no_wrap(" quedó instalado.".to_owned(), regular(14.5), TEXT_DIM);
        let w = a.size().x + b.size().x + d.size().x;
        let mut x = cx - w / 2.0;
        let y = full.top() + 228.0 - a.size().y / 2.0;
        let (aw, bw) = (a.size().x, b.size().x);
        pt.galley(pos2(x, y), a, TEXT_DIM);
        x += aw;
        pt.galley(pos2(x, y), b, ACCENT);
        x += bw;
        pt.galley(pos2(x, y), d, TEXT_DIM);

        // podés borrar el instalador
        let x0 = full.left() + 28.0;
        let x1 = full.right() - 28.0;
        let card = Rect::from_min_max(pos2(x0, full.top() + 256.0), pos2(x1, full.top() + 320.0));
        let mut pc = p.clone();
        pc.multiply_opacity(ease_out((t - 0.3) / 0.35));
        pc.rect_filled(card, 12.0, BG_PANEL);
        pc.rect_stroke(card, 12.0, Stroke::new(1.0, Color32::from_white_alpha(12)), StrokeKind::Inside);
        let ic = pos2(card.left() + 26.0, card.center().y);
        pc.circle_filled(ic, 14.0, ACCENT.gamma_multiply(0.16));
        pc.text(ic, Align2::CENTER_CENTER, "i", bold(15.0), ACCENT);
        pc.text(pos2(card.left() + 52.0, card.top() + 21.0), Align2::LEFT_CENTER, "Ya podés borrar el instalador", bold(13.5), TEXT);
        let me = std::env::current_exe()
            .ok()
            .and_then(|e| e.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "CapyroInstaller.exe".into());
        let sub = format!("Capyro no necesita {me} para andar. Se desinstala desde Configuración → Aplicaciones.");
        let g = pc.layout(sub, regular(11.5), TEXT_DIM, card.width() - 66.0);
        pc.galley(pos2(card.left() + 52.0, card.top() + 32.0), g, TEXT_DIM);

        // ¿Lanzar Capyro? Sí / No (los dos cierran el instalador)
        let mut pq = p.clone();
        let kq = ease_out((t - 0.45) / 0.35);
        pq.multiply_opacity(kq);
        pq.text(pos2(cx, full.top() + 362.0), Align2::CENTER_CENTER, "¿Lanzar Capyro?", bold(19.0), TEXT);
        let by = full.top() + 412.0;
        let mut choice = None;
        for (yes, label, x) in [(true, "Sí", cx - 78.0), (false, "No", cx + 78.0)] {
            let r = Rect::from_center_size(pos2(x, by), vec2(136.0, 46.0));
            let a = button(ui, Id::unique(("inst-launch", yes)), r, kq > 0.8);
            let rr = Rect::from_center_size(r.center(), r.size() * (1.0 + 0.05 * a.hover - 0.07 * a.press));
            if yes {
                if a.hover > 0.001 {
                    pq.rect_filled(rr.expand(5.0 * a.hover), 28.0, ACCENT.gamma_multiply(0.2 * a.hover));
                }
                pq.rect_filled(rr, 23.0, ACCENT.lerp_to_gamma(Color32::WHITE, 0.12 * a.hover));
                pq.text(rr.center(), Align2::CENTER_CENTER, label, bold(16.0), Color32::WHITE);
            } else {
                pq.rect_filled(rr, 23.0, Color32::from_white_alpha((10.0 + 14.0 * a.hover) as u8));
                pq.rect_stroke(rr, 23.0, Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Inside);
                pq.text(rr.center(), Align2::CENTER_CENTER, label, bold(16.0), TEXT);
            }
            if a.resp.clicked() {
                choice = Some(yes);
            }
        }
        if choice == Some(true) {
            if let Err(e) = install::launch(&exe) {
                self.screen = Screen::Failed { message: format!("No pude abrir Capyro: {e}"), at: now };
                return;
            }
        }
        if choice.is_some() {
            self.close = true;
        }
    }

    fn failed(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect, now: f64) {
        let Screen::Failed { message, at } = &self.screen else { return };
        let (message, t) = (message.clone(), (now - *at) as f32);
        let cx = full.center().x;
        let c = pos2(cx, full.top() + 140.0);
        p.circle_filled(c, 32.0 * ease_out(t / 0.35), ERROR);
        icon_cross(p, c, 30.0, Color32::WHITE, 4.0);
        p.text(pos2(cx, full.top() + 202.0), Align2::CENTER_CENTER, "No se pudo instalar", bold(24.0), TEXT);
        let g = p.layout(message, regular(13.0), TEXT_DIM, full.width() - 80.0);
        p.galley(pos2(cx - g.size().x / 2.0, full.top() + 228.0), g, TEXT_DIM);
        let by = full.top() + 330.0;
        for (retry, label, x) in [(true, "Reintentar", cx - 84.0), (false, "Cerrar", cx + 84.0)] {
            let r = Rect::from_center_size(pos2(x, by), vec2(148.0, 44.0));
            let a = button(ui, Id::unique(("inst-failed", retry)), r, true);
            let rr = Rect::from_center_size(r.center(), r.size() * (1.0 + 0.04 * a.hover - 0.06 * a.press));
            let fill = if retry { ACCENT.lerp_to_gamma(Color32::WHITE, 0.12 * a.hover) } else { Color32::from_white_alpha((10.0 + 14.0 * a.hover) as u8) };
            p.rect_filled(rr, 22.0, fill);
            p.text(rr.center(), Align2::CENTER_CENTER, label, bold(14.5), if retry { Color32::WHITE } else { TEXT });
            if a.resp.clicked() {
                if retry {
                    self.screen = Screen::Setup;
                    self.target = 0.0;
                    self.shown = 0.0;
                } else {
                    self.close = true;
                }
            }
        }
    }

    /// Abajo: el autor y con qué se hizo (cada uno con su link).
    fn credits(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect, now: f64) {
        let x0 = full.left() + 28.0;
        let x1 = full.right() - 28.0;
        let top = full.bottom() - 124.0;
        let gap = 14.0;
        let w = (x1 - x0 - gap) / 2.0;
        let cards = [
            (Rect::from_min_size(pos2(x0, top), vec2(w, 62.0)), "HECHO POR", true),
            (Rect::from_min_size(pos2(x0 + w + gap, top), vec2(w, 62.0)), "POTENCIADO POR", false),
        ];
        for (card, label, author) in cards {
            p.text(card.left_top() - vec2(0.0, 11.0), Align2::LEFT_CENTER, label, bold(10.0), TEXT_FAINT);
            let a = button(ui, Id::unique(("inst-credit", author)), card, true);
            p.rect_filled(card, 14.0, Color32::from_white_alpha((8.0 + 9.0 * a.hover) as u8));
            p.rect_stroke(card, 14.0, Stroke::new(1.0, Color32::from_white_alpha(14).lerp_to_gamma(ACCENT, a.hover)), StrokeKind::Inside);
            let ic = pos2(card.left() + 32.0, card.center().y);
            let tx = card.left() + 62.0;
            let (name, sub) = if author {
                if let Some(tex) = &self.avatar {
                    let av = Rect::from_center_size(ic, Vec2::splat(40.0));
                    p.add(RectShape::filled(av, 20.0, Color32::WHITE).with_texture(tex.id(), uv()));
                    p.circle_stroke(ic, 20.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.8)));
                }
                (AUTHOR.to_owned(), format!("github.com/{GITHUB_USER}"))
            } else {
                // el destello: gira con el mouse encima, vuelta rápida al clic
                let dt = ui.ctx().input(|i| i.stable_dt).min(0.05);
                let target = if a.hover > 0.5 { 7.0 } else { 0.0 };
                self.spark_speed += (target - self.spark_speed) * (dt * 5.0).min(1.0);
                if a.resp.is_pointer_button_down_on() && self.spark_speed < 12.0 {
                    self.spark_speed = 24.0;
                }
                self.spark_angle += self.spark_speed * dt;
                if a.hover < 0.5 && self.spark_speed.abs() < 1.0 {
                    let q = std::f32::consts::FRAC_PI_2;
                    let rest = (self.spark_angle / q).round() * q;
                    self.spark_angle += (rest - self.spark_angle) * (dt * 8.0).min(1.0);
                }
                self.spark_angle %= std::f32::consts::TAU;
                p.circle_filled(ic, 20.0, BG_DEEP);
                p.circle_stroke(ic, 20.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.35 + 0.45 * a.hover)));
                let pulse = 1.0 + 0.1 * a.hover * ((now * 6.0).sin() as f32 * 0.5 + 0.5);
                let col = TEXT.lerp_to_gamma(ACCENT, a.hover);
                sparkle(p, ic, 11.0 * pulse, self.spark_angle, col);
                sparkle(p, ic + vec2(9.0, -8.5), 4.0 * pulse, -self.spark_angle * 1.4, col.gamma_multiply(0.8));
                ("Claude".to_owned(), "Anthropic".to_owned())
            };
            p.text(pos2(tx, card.center().y - 9.0), Align2::LEFT_CENTER, name, bold(14.0), TEXT);
            let sg = p.layout_no_wrap(sub, regular(11.5), if author { ACCENT } else { TEXT_DIM });
            let sw = sg.size().x;
            let sy = card.center().y + 9.0;
            p.galley(pos2(tx, sy - sg.size().y / 2.0), sg, ACCENT);
            arrow_out(p, pos2(tx + sw + 9.0 + 1.5 * a.hover, sy - 1.5 * a.hover), 3.2, if author { ACCENT } else { TEXT_DIM.lerp_to_gamma(ACCENT, a.hover) });
            if a.resp.clicked() {
                open_url(if author { GITHUB_URL } else { CLAUDE_URL });
            }
        }
        // pie
        let fy = full.bottom() - 20.0;
        p.text(pos2(x0, fy), Align2::LEFT_CENTER, format!("Capyro - Installer {INSTALLER_VERSION}  ·  GPL-3.0"), regular(10.5), TEXT_FAINT);
        if install::SIMULATION {
            p.text(pos2(x1, fy), Align2::RIGHT_CENTER, "simulación (no es Windows)", regular(10.5), ACCENT.gamma_multiply(0.7));
        }
    }

    fn uninstall_confirm(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect) {
        let Screen::UninstallConfirm { dir, config } = &self.screen else { return };
        let (dir, config) = (dir.clone(), config.clone());
        let x0 = full.left() + 28.0;
        let x1 = full.right() - 28.0;
        let cx = full.center().x;
        p.text(pos2(x0, full.top() + 104.0), Align2::LEFT_CENTER, "Desinstalar Capyro", bold(26.0), TEXT);
        let Some(dir) = dir else {
            p.text(pos2(x0, full.top() + 140.0), Align2::LEFT_CENTER, "No encontré Capyro instalado para tu usuario.", regular(13.5), TEXT_DIM);
            let r = Rect::from_center_size(pos2(cx, full.top() + 230.0), vec2(148.0, 44.0));
            let a = button(ui, Id::unique("inst-un-close"), r, true);
            p.rect_filled(r, 22.0, Color32::from_white_alpha((10.0 + 14.0 * a.hover) as u8));
            p.text(r.center(), Align2::CENTER_CENTER, "Cerrar", bold(14.5), TEXT);
            if a.resp.clicked() {
                self.close = true;
            }
            return;
        };
        let g = p.layout(
            "Se borran el programa, los accesos directos y la entrada de Aplicaciones. Tu configuración, tus listas y tus capturas quedan.".to_owned(),
            regular(13.0),
            TEXT_DIM,
            x1 - x0,
        );
        p.galley(pos2(x0, full.top() + 130.0), g, TEXT_DIM);
        let rows = [("Se borra", dir.display().to_string()), ("Queda", config.map(|c| c.display().to_string()).unwrap_or_default())];
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = full.top() + 200.0 + 26.0 * i as f32;
            p.text(pos2(x0, y), Align2::LEFT_CENTER, *k, regular(12.0), TEXT_FAINT);
            let v = fit_path(p, v, regular(12.0), x1 - x0 - 90.0);
            p.text(pos2(x0 + 90.0, y), Align2::LEFT_CENTER, v, regular(12.0), TEXT);
        }
        let by = full.top() + 300.0;
        let mut go = None;
        for (yes, label, x) in [(true, "Desinstalar", cx - 84.0), (false, "Cancelar", cx + 84.0)] {
            let r = Rect::from_center_size(pos2(x, by), vec2(152.0, 46.0));
            let a = button(ui, Id::unique(("inst-un", yes)), r, true);
            let rr = Rect::from_center_size(r.center(), r.size() * (1.0 + 0.05 * a.hover - 0.06 * a.press));
            if yes {
                p.rect_filled(rr, 23.0, BUTTON_GRAY.lerp_to_gamma(ERROR, a.hover));
                p.text(rr.center(), Align2::CENTER_CENTER, label, bold(15.0), Color32::WHITE);
            } else {
                p.rect_filled(rr, 23.0, Color32::from_white_alpha((10.0 + 14.0 * a.hover) as u8));
                p.text(rr.center(), Align2::CENTER_CENTER, label, bold(15.0), TEXT);
            }
            if a.resp.clicked() {
                go = Some(yes);
            }
        }
        let now = ui.ctx().input(|i| i.time);
        match go {
            Some(true) => match install::uninstall(&dir) {
                Ok(()) => {
                    self.delete_on_exit = Some(dir.clone());
                    let config = install::installed().and_then(|i| i.1).or_else(|| match &self.screen {
                        Screen::UninstallConfirm { config, .. } => config.clone(),
                        _ => None,
                    });
                    self.screen = Screen::Uninstalled { config, at: now };
                }
                Err(message) => self.screen = Screen::Failed { message, at: now },
            },
            Some(false) => self.close = true,
            None => {}
        }
    }

    fn uninstalled(&mut self, ui: &mut Ui, p: &egui::Painter, full: Rect, now: f64) {
        let Screen::Uninstalled { config, at } = &self.screen else { return };
        let t = (now - *at) as f32;
        let cx = full.center().x;
        let c = pos2(cx, full.top() + 140.0);
        p.circle_filled(c, 32.0 * ease_out(t / 0.35), GREEN);
        icon_check(p, c, 38.0, Color32::WHITE, 4.0);
        p.text(pos2(cx, full.top() + 204.0), Align2::CENTER_CENTER, "Capyro se desinstaló", bold(24.0), TEXT);
        if let Some(cfg) = config {
            let s = format!("Tu configuración quedó en {}", cfg.display());
            let s = fit_path(p, &s, regular(12.5), full.width() - 60.0);
            p.text(pos2(cx, full.top() + 236.0), Align2::CENTER_CENTER, s, regular(12.5), TEXT_DIM);
        }
        p.text(pos2(cx, full.top() + 262.0), Align2::CENTER_CENTER, "¡Gracias por haber sido parte de la manada!", regular(12.5), TEXT_FAINT);
        let r = Rect::from_center_size(pos2(cx, full.top() + 330.0), vec2(148.0, 44.0));
        let a = button(ui, Id::unique("inst-un-done"), r, true);
        let rr = Rect::from_center_size(r.center(), r.size() * (1.0 + 0.05 * a.hover - 0.06 * a.press));
        p.rect_filled(rr, 22.0, ACCENT.lerp_to_gamma(Color32::WHITE, 0.12 * a.hover));
        p.text(rr.center(), Align2::CENTER_CENTER, "Cerrar", bold(14.5), Color32::WHITE);
        if a.resp.clicked() {
            self.close = true;
        }
        if t < 0.5 {
            ui.ctx().request_repaint();
        }
    }
}
