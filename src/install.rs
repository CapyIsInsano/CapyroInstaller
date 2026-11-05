//! La instalación en sí: corre en otro hilo y le cuenta a la interfaz por
//! dónde va con [`Msg`].
//!
//! **Windows**
//! 1. El paquete (`capyro-windows-x64.zip`): el que viene adentro del
//!    instalador o, si no trae, el de la última versión para Windows
//!    publicada en GitHub (Releases del repo de Capyro).
//! 2. Se descomprime en la carpeta elegida. Lo que ya existía y está en uso
//!    (un Capyro abierto) se renombra a `.old`; Capyro lo borra solo al abrir.
//! 3. La config: si no existe, la de ejemplo con `system = "Win"`. Si ya
//!    existe no se pisa (solo se corrige `system`). Su ruta queda anotada en
//!    `capyro-config.txt`, al lado de capyro.exe.
//! 4. Accesos directos, "Aplicaciones instaladas" y uninstall.exe.
//!
//! **Mac**
//! 1. El paquete (`capyro-macos-arm64.zip` o `capyro-macos-x64.zip`, según
//!    la Mac): el de adentro o el de la release `macos` de GitHub.
//! 2. El Capyro.app se descomprime al lado del lugar elegido (con los
//!    permisos y la firma intactos) y después toma su lugar: si había uno,
//!    nunca queda a medias.
//! 3. La config, igual que en Windows pero con `system = "Mac"`; su ruta va
//!    en `~/Library/Application Support/Capyro/capyro-config.txt` (el .app
//!    no se toca: está firmado).
//! 4. Se anota en el Finder ("Abrir con…") y, si se pidió, en el Dock.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Repo de Capyro (de dónde se baja si el instalador no trae el paquete).
pub const REPO: &str = "CapyIsInsano/Capyro";
/// La release que publica el CI del repo con la versión para este sistema
/// (en Linux, la simulación usa la de Windows).
pub const RELEASE_TAG: &str = if cfg!(target_os = "macos") { "macos" } else { "windows" };
pub const ASSET: &str = if cfg!(target_os = "macos") {
    if cfg!(target_arch = "aarch64") {
        "capyro-macos-arm64.zip"
    } else {
        "capyro-macos-x64.zip"
    }
} else {
    "capyro-windows-x64.zip"
};
/// `options.system` de la config que se instala.
pub const SYSTEM_CODE: &str = if cfg!(target_os = "macos") { "Mac" } else { "Win" };
/// Para los textos: "Windows" o "la Mac".
pub const SYSTEM_NAME: &str = if cfg!(target_os = "macos") { "la Mac" } else { "Windows" };
/// Clave de "Aplicaciones instaladas" (por usuario: no pide administrador).
#[cfg_attr(not(windows), allow(dead_code))]
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Capyro";
/// Formatos que abre Capyro ("Abrir con…").
#[cfg_attr(not(windows), allow(dead_code))]
pub const EXTENSIONS: [&str; 15] = [
    ".mp4", ".mov", ".avi", ".mkv", ".webm", // video
    ".mp3", ".wav", ".ogg", ".aac", ".flac", ".opus", ".m4a", ".aiff", ".aif", ".aifc", // audio
];
/// Tipo de archivo de Capyro en el registro (ProgID).
#[cfg_attr(not(windows), allow(dead_code))]
pub const PROG_ID: &str = "Capyro.Media";
/// Dónde se anotan las capacidades (Aplicaciones predeterminadas).
#[cfg_attr(not(windows), allow(dead_code))]
pub const CAPABILITIES: &str = r"Software\Capyro\Capabilities";
/// Dónde Windows busca capyro.exe por su nombre (App Paths, por usuario).
#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\capyro.exe";
/// Lista "Abrir con…" que el Explorador lee por cada extensión (por usuario).
#[cfg_attr(not(windows), allow(dead_code))]
pub const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// Archivo junto a capyro.exe con la ruta de la config (lo lee Capyro).
pub const CONFIG_POINTER: &str = "capyro-config.txt";

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

/// Lo que eligió el usuario.
#[derive(Debug, Clone)]
pub struct Plan {
    pub install_dir: PathBuf,
    pub config_path: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu: bool,
}

/// Lo que se le cuenta a la interfaz.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Qué se está haciendo ("Copiando avcodec-63.dll").
    Step(String),
    /// 0.0 ..= 1.0
    Progress(f32),
    Done { version: String, exe: PathBuf },
    Failed(String),
}

/// true = esto no es Windows ni la Mac: se instala en una carpeta de prueba y
/// no se tocan accesos directos ni el registro (para probar la interfaz).
pub const SIMULATION: bool = !cfg!(any(windows, target_os = "macos"));

/// Raíz de la "simulación" (Linux).
#[cfg(not(any(windows, target_os = "macos")))]
fn sim_root() -> PathBuf {
    std::env::temp_dir().join("capyro-installer-prueba")
}

/// La carpeta personal (Mac).
#[cfg(target_os = "macos")]
fn home() -> PathBuf {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Mac: `~/Library/Application Support/Capyro` (config, listas, sesión).
#[cfg(target_os = "macos")]
pub fn app_support() -> PathBuf {
    home().join("Library/Application Support/Capyro")
}

/// `%LOCALAPPDATA%\Programs\Capyro` (donde instalan las apps por usuario).
/// Mac: `/Applications/Capyro.app` (o `~/Applications` si no se puede
/// escribir en la de todos: un usuario que no es administrador).
pub fn default_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Capyro"))
            .join("Programs")
            .join("Capyro")
    }
    #[cfg(target_os = "macos")]
    {
        let all = Path::new("/Applications");
        let probe = all.join(format!(".capyro-installer-{}", std::process::id()));
        let writable = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        if writable {
            all.join("Capyro.app")
        } else {
            home().join("Applications").join("Capyro.app")
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        sim_root().join("AppData/Local/Programs/Capyro")
    }
}

/// `Documentos\Capyro\Config\Capyro.lua`; en la Mac,
/// `~/Library/Application Support/Capyro/capyro.lua` (como las demás apps, y
/// sin el cartel de permiso que macOS pide para Documentos).
pub fn default_config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        app_support().join("capyro.lua")
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(windows)]
        let docs = dirs::document_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
            .unwrap_or_else(|| PathBuf::from(r"C:\Capyro"));
        #[cfg(not(windows))]
        let docs = sim_root().join("Documents");
        docs.join("Capyro").join("Config").join("Capyro.lua")
    }
}

/// La versión que trae el paquete de adentro (version.txt), si trae.
pub fn embedded_version() -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD?)).ok()?;
    let name = if cfg!(target_os = "macos") {
        format!("{APP_ROOT}Contents/Resources/version.txt")
    } else {
        format!("{}version.txt", root_prefix(&mut zip))
    };
    let mut f = zip.by_name(&name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    let v = s.lines().next()?.trim().to_owned();
    (!v.is_empty()).then_some(v)
}

/// Corre la instalación entera (en el hilo que la llama).
pub fn run(plan: &Plan, tx: &Sender<Msg>, wake: &dyn Fn()) {
    let send = |m: Msg| {
        let _ = tx.send(m);
        wake();
    };
    match install(plan, &send) {
        Ok((version, exe)) => send(Msg::Done { version, exe }),
        Err(e) => send(Msg::Failed(e)),
    }
}

/// El paquete (sus bytes), la versión de la release si se bajó, y dónde
/// empieza la barra de progreso (bajar ocupa la primera parte).
type Package = (std::borrow::Cow<'static, [u8]>, Option<String>, f32);

/// El paquete: el de adentro, o el de GitHub.
fn package(send: &dyn Fn(Msg)) -> Result<Package, String> {
    match PAYLOAD {
        Some(p) => Ok((std::borrow::Cow::Borrowed(p), None, 0.0)),
        None => {
            let (data, version) = download_latest(send)?;
            Ok((std::borrow::Cow::Owned(data), version, 0.55))
        }
    }
}

#[cfg(target_os = "macos")]
fn install(plan: &Plan, send: &dyn Fn(Msg)) -> Result<(String, PathBuf), String> {
    send(Msg::Step("Preparando…".into()));
    send(Msg::Progress(0.0));
    let (bytes, release_version, span_start) = package(send)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&*bytes)).map_err(|e| format!("El paquete está dañado: {e}"))?;
    if zip.by_name(&format!("{APP_ROOT}Contents/MacOS/Capyro")).is_err() {
        return Err("El paquete no trae Capyro.app".into());
    }

    // 1. el Capyro.app nuevo, al lado de donde va
    let app = &plan.install_dir;
    let parent = app.parent().ok_or("Elegí una carpeta para Capyro.app")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("No pude crear {}: {e}", parent.display()))?;
    let staged = parent.join(".Capyro-instalando.app");
    let _ = std::fs::remove_dir_all(&staged);
    extract_app(&mut zip, &staged, send, (span_start, 0.88)).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staged);
        if e.contains("ermission") || e.contains("ermiso") {
            format!("No tengo permiso para escribir en {}. Probá en tu carpeta de Aplicaciones (~/Applications)", parent.display())
        } else {
            e
        }
    })?;
    let resources = staged.join("Contents/Resources");
    let version = std::fs::read_to_string(resources.join("version.txt"))
        .ok()
        .and_then(|t| t.lines().next().map(|l| l.trim().to_owned()))
        .filter(|v| !v.is_empty())
        .or(release_version)
        .unwrap_or_else(|| "(sin versión)".into());
    let default_config = std::fs::read_to_string(resources.join("config").join("capyro.lua")).ok();

    // 2. toma el lugar del que había (Capyro abierto no molesta: sigue andando)
    send(Msg::Step("Copiando Capyro.app…".into()));
    swap_app(&staged, app).map_err(|e| format!("No pude poner Capyro.app en {}: {e}", parent.display()))?;
    send(Msg::Progress(0.92));

    // 3. la config
    send(Msg::Step("Guardando la configuración…".into()));
    write_config(&plan.config_path, default_config.as_deref())
        .map_err(|e| format!("No pude guardar la configuración en {}: {e}", plan.config_path.display()))?;
    let pointer = format!(
        "# Dónde está la configuración de Capyro (la eligió el instalador).\n# Para usar otra, cambiá la ruta de abajo.\n{}\n",
        plan.config_path.display()
    );
    let support = app_support();
    std::fs::create_dir_all(&support)
        .and_then(|_| std::fs::write(support.join(CONFIG_POINTER), pointer))
        .map_err(|e| format!("No pude escribir {CONFIG_POINTER}: {e}"))?;
    send(Msg::Progress(0.95));

    // 4. el Finder ("Abrir con…") y el Dock
    send(Msg::Step(if plan.desktop_shortcut { "Agregando Capyro al Dock…" } else { "Avisándole al Finder…" }.into()));
    system::integrate(plan, app, &version)?;
    send(Msg::Progress(1.0));
    send(Msg::Step("Listo".into()));
    Ok((version, app.clone()))
}

#[cfg(not(target_os = "macos"))]
fn install(plan: &Plan, send: &dyn Fn(Msg)) -> Result<(String, PathBuf), String> {
    send(Msg::Step("Preparando…".into()));
    send(Msg::Progress(0.0));

    // 1. el paquete
    let (bytes, release_version, span_start) = package(send)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&*bytes)).map_err(|e| format!("El paquete está dañado: {e}"))?;
    let root = root_prefix(&mut zip);
    if zip.by_name(&format!("{root}capyro.exe")).is_err() {
        return Err("El paquete no trae capyro.exe".into());
    }

    // 2. los archivos
    let dir = &plan.install_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("No pude crear {}: {e}", dir.display()))?;
    let probe = dir.join(".capyro-installer-test");
    std::fs::write(&probe, b"ok").map_err(|e| format!("No tengo permiso para escribir en {}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);

    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.size())).sum::<u64>().max(1);
    let mut done: u64 = 0;
    let mut version_txt = None;
    let mut default_config = None;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("El paquete está dañado: {e}"))?;
        let Some(name) = f.enclosed_name() else { continue };
        let Ok(rel) = name.strip_prefix(root.trim_end_matches('/')) else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dir.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("No pude crear {}: {e}", target.display()))?;
            continue;
        }
        let shown = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        send(Msg::Step(format!("Copiando {shown}")));
        let mut data = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut data).map_err(|e| format!("No pude leer {shown} del paquete: {e}"))?;
        if rel == Path::new("version.txt") {
            version_txt = String::from_utf8_lossy(&data).lines().next().map(|l| l.trim().to_owned());
        }
        if rel == Path::new("config").join("capyro.lua") {
            default_config = Some(String::from_utf8_lossy(&data).into_owned());
        }
        write_replacing(&target, &data).map_err(|e| format!("No pude copiar {shown}: {e}"))?;
        done += f.size();
        let k = done as f32 / total as f32;
        send(Msg::Progress(span_start + (0.92 - span_start) * k));
    }
    let exe = dir.join("capyro.exe");
    let version = version_txt
        .filter(|v| !v.is_empty())
        .or(release_version)
        .unwrap_or_else(|| "(sin versión)".into());

    // 3. la config
    send(Msg::Step("Guardando la configuración…".into()));
    write_config(&plan.config_path, default_config.as_deref()).map_err(|e| {
        format!("No pude guardar la configuración en {}: {e}", plan.config_path.display())
    })?;
    let pointer = format!(
        "# Dónde está la configuración de Capyro (la eligió el instalador).\n# Para usar otra, cambiá la ruta de abajo.\n{}\n",
        plan.config_path.display()
    );
    std::fs::write(dir.join(CONFIG_POINTER), pointer).map_err(|e| format!("No pude escribir {CONFIG_POINTER}: {e}"))?;
    send(Msg::Progress(0.95));

    // 4. Windows: accesos directos, desinstalador y "Aplicaciones instaladas"
    send(Msg::Step("Creando accesos directos…".into()));
    system::integrate(plan, &exe, &version)?;
    send(Msg::Progress(1.0));
    send(Msg::Step("Listo".into()));
    Ok((version, exe))
}

/// La carpeta de adentro del zip si todo viene en una ("Capyro/..."), o "".
fn root_prefix<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> String {
    if zip.by_name("capyro.exe").is_ok() {
        return String::new();
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    names
        .iter()
        .find_map(|n| n.strip_suffix("capyro.exe").filter(|p| p.ends_with('/') && p.matches('/').count() == 1))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// Escribe `data` en `target`. Si ya existe se renombra a `.old` antes
/// (un .exe o una .dll en uso no se pueden pisar, pero sí renombrar).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn write_replacing(target: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        let mut old = target.as_os_str().to_owned();
        old.push(".old");
        let old = PathBuf::from(old);
        let _ = std::fs::remove_file(&old);
        if std::fs::rename(target, &old).is_err() {
            // no se pudo renombrar: se intenta pisar directamente
            return std::fs::write(target, data);
        }
    }
    let mut f = std::fs::File::create(target)?;
    f.write_all(data)?;
    f.sync_all()
}

/// Adentro del paquete de la Mac, todo va en esta carpeta.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub const APP_ROOT: &str = "Capyro.app/";

/// Saca `Capyro.app/…` del zip a `dest` (que queda como el .app), con los
/// permisos de cada archivo (el ejecutable y las librerías tienen que poder
/// ejecutarse) y los enlaces simbólicos. Los metadatos del Finder
/// (`__MACOSX/`, `._algo`) no.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn extract_app<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    dest: &Path,
    send: &dyn Fn(Msg),
    span: (f32, f32),
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.size())).sum::<u64>().max(1);
    let mut done = 0u64;
    std::fs::create_dir_all(dest).map_err(|e| format!("No pude crear {}: {e}", dest.display()))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("El paquete está dañado: {e}"))?;
        let Some(name) = f.enclosed_name() else { continue };
        let Ok(rel) = name.strip_prefix(APP_ROOT.trim_end_matches('/')) else { continue };
        if rel.as_os_str().is_empty() || rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with("._")) {
            continue;
        }
        let target = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("No pude crear {}: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("No pude crear {}: {e}", parent.display()))?;
        }
        let shown = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        send(Msg::Step(format!("Copiando {shown}")));
        let mut data = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut data).map_err(|e| format!("No pude leer {shown} del paquete: {e}"))?;
        if f.is_symlink() {
            let to = String::from_utf8_lossy(&data).into_owned();
            std::os::unix::fs::symlink(&to, &target).map_err(|e| format!("No pude crear el enlace {shown}: {e}"))?;
        } else {
            std::fs::write(&target, &data).map_err(|e| format!("No pude copiar {shown}: {e}"))?;
            let mode = f.unix_mode().map(|m| m & 0o7777).filter(|m| *m != 0).unwrap_or(0o644);
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("No pude darle permisos a {shown}: {e}"))?;
        }
        done += f.size();
        send(Msg::Progress(span.0 + (span.1 - span.0) * (done as f32 / total as f32)));
    }
    Ok(())
}

/// Pone el .app recién sacado (`staged`) en el lugar de `app`: el que había
/// pasa a `.Capyro-viejo.app` (y se borra), así nunca queda uno a medias.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn swap_app(staged: &Path, app: &Path) -> std::io::Result<()> {
    let parent = app.parent().ok_or_else(|| std::io::Error::other("sin carpeta"))?;
    let old = parent.join(".Capyro-viejo.app");
    let _ = std::fs::remove_dir_all(&old);
    let had = app.exists();
    if had {
        std::fs::rename(app, &old)?;
    }
    if let Err(e) = std::fs::rename(staged, app) {
        if had {
            let _ = std::fs::rename(&old, app);
        }
        return Err(e);
    }
    if had {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

/// `options.system` en "Win" (la línea `system = "..."` de capyro.lua).
#[cfg(test)]
pub fn set_system_win(lua: &str) -> String {
    set_system(lua, "Win")
}

/// `options.system` en `code` ("Win" o "Mac"; la línea `system = "..."` de
/// capyro.lua).
pub fn set_system(lua: &str, code: &str) -> String {
    let mut out = String::with_capacity(lua.len() + 32);
    let mut found = false;
    for line in lua.split_inclusive('\n') {
        let t = line.trim_start();
        let is_opt = t.starts_with("system") && t["system".len()..].trim_start().starts_with('=') && !found;
        if is_opt {
            let indent = &line[..line.len() - t.len()];
            let after_eq = &t[t.find('=').unwrap() + 1..];
            // lo que viene después del valor (coma y comentario)
            let rest = after_eq
                .trim_start()
                .strip_prefix('"')
                .and_then(|v| v.find('"').map(|i| &v[i + 1..]))
                .unwrap_or(",\n");
            let pad = &t["system".len()..t.find('=').unwrap()];
            out.push_str(&format!("{indent}system{pad}= \"{code}\"{rest}"));
            if !rest.ends_with('\n') && line.ends_with('\n') {
                out.push('\n');
            }
            found = true;
        } else {
            out.push_str(line);
        }
    }
    out
}

fn write_config(path: &Path, default: Option<&str>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        // ya había una: no se pisa (son los atajos y opciones del usuario)
        let old = std::fs::read_to_string(path)?;
        let fixed = set_system(&old, SYSTEM_CODE);
        if fixed != old {
            std::fs::write(path, fixed)?;
        }
        return Ok(());
    }
    let fallback = format!("-- Configuración de Capyro\nreturn {{ options = {{ system = \"{SYSTEM_CODE}\" }} }}\n");
    let base = default.unwrap_or(&fallback);
    let text = format!("-- Instalado por CapyroInstaller.\n{}", set_system(base, SYSTEM_CODE));
    std::fs::write(path, text)
}

/* ------------------------------------------------------------------------ */
/* bajar la última versión (instalador online)                              */
/* ------------------------------------------------------------------------ */

fn repo() -> String {
    std::env::var("CAPYRO_REPO").ok().filter(|r| r.contains('/')).unwrap_or_else(|| REPO.to_owned())
}

fn curl() -> PathBuf {
    #[cfg(windows)]
    {
        let sys = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("curl.exe"));
        match sys {
            Some(p) if p.is_file() => p,
            _ => PathBuf::from("curl.exe"),
        }
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("curl")
    }
}

fn command(program: &Path) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn download_latest(send: &dyn Fn(Msg)) -> Result<(Vec<u8>, Option<String>), String> {
    send(Msg::Step("Buscando la última versión…".into()));
    let url = format!("https://api.github.com/repos/{}/releases/tags/{RELEASE_TAG}", repo());
    let out = command(&curl())
        .args(["-sS", "-L", "--fail", "--connect-timeout", "15", "--max-time", "60"])
        .args(["-H", "Accept: application/vnd.github+json", "-A", "CapyroInstaller"])
        .arg(&url)
        .output()
        .map_err(|e| format!("No pude usar curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.contains("404") {
            format!("Todavía no hay una versión de Capyro para {SYSTEM_NAME} publicada")
        } else if matches!(out.status.code(), Some(5 | 6 | 7 | 28 | 35)) {
            "Sin conexión: no pude llegar a GitHub".into()
        } else {
            format!("GitHub falló: {}", err.trim())
        });
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("Respuesta rara de GitHub: {e}"))?;
    let asset = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == ASSET))
        .ok_or_else(|| format!("La versión publicada no trae el paquete para {SYSTEM_NAME} ({ASSET})"))?;
    let dl = asset["browser_download_url"].as_str().ok_or("El paquete no tiene URL")?.to_owned();
    let size = asset["size"].as_u64().unwrap_or(0);
    let version = v["body"].as_str().and_then(|b| {
        b.lines().find_map(|l| l.trim().strip_prefix("version:").map(|v| v.trim().trim_matches('`').to_owned()))
    });

    let tmp = std::env::temp_dir().join(format!("capyro-installer-{}.zip", std::process::id()));
    let mut child = command(&curl())
        .args(["-sS", "-L", "--fail", "--retry", "2", "--connect-timeout", "15", "-A", "CapyroInstaller", "-o"])
        .arg(&tmp)
        .arg(&dl)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("No pude usar curl: {e}"))?;
    let mb = |b: u64| format!("{:.1}", b as f64 / 1_048_576.0).replace('.', ",");
    let status = loop {
        let got = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        if size > 0 {
            send(Msg::Step(format!("Bajando Capyro ({} de {} MB)", mb(got), mb(size))));
            send(Msg::Progress(0.55 * (got as f32 / size as f32).min(1.0)));
        } else {
            send(Msg::Step(format!("Bajando Capyro ({} MB)", mb(got))));
        }
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    };
    let data = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if !status.success() {
        return Err("Se cortó la descarga de Capyro".into());
    }
    let data = data.map_err(|e| e.to_string())?;
    if size > 0 && data.len() as u64 != size {
        return Err("La descarga quedó incompleta".into());
    }
    Ok((data, version))
}

/* ------------------------------------------------------------------------ */
/* lo que es de Windows                                                     */
/* ------------------------------------------------------------------------ */

#[cfg(windows)]
mod system {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    use winreg::RegKey;

    fn shortcut(exe: &Path, lnk: &Path) -> Result<(), String> {
        if let Some(d) = lnk.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let mut sl = mslnk::ShellLink::new(exe).map_err(|e| e.to_string())?;
        if let Some(dir) = exe.parent() {
            sl.set_working_dir(Some(dir.to_string_lossy().into_owned()));
        }
        sl.set_icon_location(Some(exe.to_string_lossy().into_owned()));
        sl.set_name(Some("Capyro — Just press play.".to_owned()));
        sl.create_lnk(lnk).map_err(|e| e.to_string())
    }

    pub fn start_menu_lnk() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs\Capyro.lnk"))
    }

    pub fn desktop_lnk() -> Option<PathBuf> {
        dirs::desktop_dir().map(|d| d.join("Capyro.lnk"))
    }

    fn dir_size_kb(dir: &Path) -> u32 {
        fn walk(d: &Path) -> u64 {
            std::fs::read_dir(d)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| match e.file_type() {
                            Ok(t) if t.is_dir() => walk(&e.path()),
                            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
        (walk(dir) / 1024).min(u32::MAX as u64) as u32
    }

    pub fn integrate(plan: &Plan, exe: &Path, version: &str) -> Result<(), String> {
        let dir = &plan.install_dir;
        if plan.start_menu {
            if let Some(l) = start_menu_lnk() {
                shortcut(exe, &l).map_err(|e| format!("No pude crear el acceso del menú Inicio: {e}"))?;
            }
        }
        if plan.desktop_shortcut {
            if let Some(l) = desktop_lnk() {
                shortcut(exe, &l).map_err(|e| format!("No pude crear el acceso directo: {e}"))?;
            }
        }
        // el desinstalador: este mismo programa con --uninstall
        let uninstaller = dir.join("uninstall.exe");
        if let Ok(me) = std::env::current_exe() {
            if me != uninstaller {
                let _ = super::write_replacing(&uninstaller, &std::fs::read(&me).map_err(|e| e.to_string())?);
            }
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(UNINSTALL_KEY)
            .map_err(|e| format!("No pude registrar Capyro en Aplicaciones: {e}"))?;
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let set = |k: &str, v: &str| key.set_value(k, &v).map_err(|e| e.to_string());
        set("DisplayName", "Capyro")?;
        set("DisplayVersion", version)?;
        set("Publisher", "Capy y los Capibaras")?;
        set("DisplayIcon", &s(exe))?;
        set("InstallLocation", &s(dir))?;
        set("UninstallString", &format!("\"{}\" --uninstall", s(&uninstaller)))?;
        set("QuietUninstallString", &format!("\"{}\" --uninstall --quiet", s(&uninstaller)))?;
        set("URLInfoAbout", "https://github.com/CapyIsInsano/Capyro")?;
        set("CapyroConfig", &s(&plan.config_path))?;
        set("CapyroShortcutDesktop", if plan.desktop_shortcut { "1" } else { "0" })?;
        key.set_value("NoModify", &1u32).map_err(|e| e.to_string())?;
        key.set_value("NoRepair", &1u32).map_err(|e| e.to_string())?;
        key.set_value("EstimatedSize", &dir_size_kb(dir)).map_err(|e| e.to_string())?;
        register_formats(exe).map_err(|e| format!("No pude registrar los formatos para \"Abrir con\": {e}"))?;
        Ok(())
    }

    /// "Abrir con…": Capyro aparece (y como recomendada) para sus formatos.
    /// No se adueña de ninguno: el usuario elige si lo quiere por defecto
    /// (y desde Configuración → Aplicaciones predeterminadas → Capyro).
    fn register_formats(exe: &Path) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let exe_s = exe.to_string_lossy().into_owned();
        let open_cmd = format!("\"{exe_s}\" \"%1\"");
        let icon = format!("\"{exe_s}\",0");

        // el tipo "archivo de Capyro" (ProgID) y cómo se abre
        let (prog, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}"))?;
        prog.set_value("", &"Archivo multimedia (Capyro)")?;
        prog.set_value("FriendlyTypeName", &"Archivo multimedia")?;
        hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}\DefaultIcon"))?.0.set_value("", &icon)?;
        hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}\shell\open\command"))?.0.set_value("", &open_cmd)?;

        // la aplicación en sí (lista de "Abrir con")
        let app = r"Software\Classes\Applications\capyro.exe";
        let (a, _) = hkcu.create_subkey(app)?;
        a.set_value("FriendlyAppName", &"Capyro")?;
        hkcu.create_subkey(format!(r"{app}\DefaultIcon"))?.0.set_value("", &icon)?;
        hkcu.create_subkey(format!(r"{app}\shell\open\command"))?.0.set_value("", &open_cmd)?;
        let (types, _) = hkcu.create_subkey(format!(r"{app}\SupportedTypes"))?;

        // App Paths: así Windows encuentra capyro.exe por su nombre. Sin esto,
        // "Abrir con…" no puede lanzar Capyro y no lo lista.
        let (ap, _) = hkcu.create_subkey(APP_PATHS)?;
        ap.set_value("", &exe_s)?;
        if let Some(dir) = exe.parent() {
            ap.set_value("Path", &dir.to_string_lossy().into_owned())?;
        }

        // capacidades: aparece en Aplicaciones predeterminadas
        let (cap, _) = hkcu.create_subkey(CAPABILITIES)?;
        cap.set_value("ApplicationName", &"Capyro")?;
        cap.set_value("ApplicationDescription", &"Just press play. Reproductor de audio y video.")?;
        cap.set_value("ApplicationIcon", &icon)?;
        let (assoc, _) = hkcu.create_subkey(format!(r"{CAPABILITIES}\FileAssociations"))?;

        for ext in EXTENSIONS {
            types.set_value(ext, &"")?;
            assoc.set_value(ext, &PROG_ID)?;
            let (k, _) = hkcu.create_subkey(format!(r"Software\Classes\{ext}\OpenWithProgids"))?;
            k.set_value(PROG_ID, &"")?;
            // y la del Explorador: sin esta, "Abrir con…" no muestra Capyro
            let (fe, _) = hkcu.create_subkey(format!(r"{FILE_EXTS}\{ext}\OpenWithProgids"))?;
            fe.set_value(PROG_ID, &"")?;
        }
        hkcu.create_subkey(r"Software\RegisteredApplications")?.0.set_value("Capyro", &CAPABILITIES)?;
        notify_assoc_changed();
        Ok(())
    }

    fn unregister_formats() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for ext in EXTENSIONS {
            if let Ok(k) = hkcu.open_subkey_with_flags(format!(r"Software\Classes\{ext}\OpenWithProgids"), KEY_ALL_ACCESS) {
                let _ = k.delete_value(PROG_ID);
            }
            if let Ok(k) = hkcu.open_subkey_with_flags(format!(r"{FILE_EXTS}\{ext}\OpenWithProgids"), KEY_ALL_ACCESS) {
                let _ = k.delete_value(PROG_ID);
            }
        }
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{PROG_ID}"));
        let _ = hkcu.delete_subkey_all(r"Software\Classes\Applications\capyro.exe");
        let _ = hkcu.delete_subkey_all(r"Software\Capyro");
        let _ = hkcu.delete_subkey_all(APP_PATHS);
        if let Ok(k) = hkcu.open_subkey_with_flags(r"Software\RegisteredApplications", KEY_ALL_ACCESS) {
            let _ = k.delete_value("Capyro");
        }
        notify_assoc_changed();
    }

    /// Que el Explorador se entere sin reiniciar.
    fn notify_assoc_changed() {
        #[link(name = "shell32")]
        extern "system" {
            fn SHChangeNotify(event: i32, flags: u32, a: *const std::ffi::c_void, b: *const std::ffi::c_void);
        }
        const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
        // SAFETY: llamada sin punteros (SHCNF_IDLIST con NULL).
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
    }

    /// Lo que se anotó al instalar (para desinstalar).
    pub fn installed() -> Option<(PathBuf, Option<PathBuf>)> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?;
        let dir: String = key.get_value("InstallLocation").ok()?;
        let cfg: Option<String> = key.get_value("CapyroConfig").ok();
        Some((PathBuf::from(dir), cfg.map(PathBuf::from)))
    }

    pub fn remove_registration() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
        unregister_formats();
        for l in [start_menu_lnk(), desktop_lnk()].into_iter().flatten() {
            let _ = std::fs::remove_file(l);
        }
    }

    /// Borra la carpeta cuando este proceso (que está adentro, es el
    /// uninstall.exe) ya cerró: un .cmd en %TEMP% reintenta cada segundo
    /// (hasta 30 veces) y después se borra a sí mismo.
    pub fn delete_dir_later(dir: &Path) {
        use std::os::windows::process::CommandExt;
        let script = std::env::temp_dir().join(format!("capyro-borrar-{}.cmd", std::process::id()));
        let d = dir.display();
        let text = format!(
            "@echo off\r\nchcp 65001 >nul\r\nset n=0\r\n:otra\r\nping -n 2 127.0.0.1 >nul\r\n\
             rmdir /s /q \"{d}\" 2>nul\r\nif not exist \"{d}\" goto fin\r\nset /a n+=1\r\n\
             if %n% lss 30 goto otra\r\n:fin\r\ndel \"%~f0\"\r\n"
        );
        if std::fs::write(&script, text).is_err() {
            return;
        }
        let _ = std::process::Command::new("cmd.exe")
            .arg("/C")
            .arg(&script)
            .current_dir(std::env::temp_dir())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }

    pub fn launch(exe: &Path) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(exe)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .creation_flags(0x0000_0008 | 0x0000_0200) // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
            .spawn()
            .map(|_| ())
    }
}

/* ------------------------------------------------------------------------ */
/* lo que es de la Mac                                                      */
/* ------------------------------------------------------------------------ */

#[cfg(target_os = "macos")]
mod system {
    use super::*;

    const LSREGISTER: &str =
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

    pub fn integrate(plan: &Plan, app: &Path, _version: &str) -> Result<(), String> {
        // sin la marca de "bajado de internet" (lo escribió un instalador que sí la tenía)
        let _ = command(Path::new("/usr/bin/xattr")).args(["-dr", "com.apple.quarantine"]).arg(app).status();
        // que el Finder lo anote ya ("Abrir con…") sin tener que abrirlo antes
        let _ = command(Path::new(LSREGISTER)).arg("-f").arg(app).status();
        if plan.desktop_shortcut {
            add_to_dock(app);
        }
        Ok(())
    }

    /// Lo agrega a las apps fijas del Dock (si no está ya) y reinicia el Dock.
    fn add_to_dock(app: &Path) {
        let path = app.to_string_lossy().trim_end_matches('/').to_owned();
        let current = command(Path::new("/usr/bin/defaults"))
            .args(["read", "com.apple.dock", "persistent-apps"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        if current.contains(&format!("{}/", crate::dock::url_path(&path))) || current.contains(&path) {
            return;
        }
        let tile = format!(
            "<dict><key>tile-data</key><dict><key>file-data</key><dict>\
             <key>_CFURLString</key><string>{}</string><key>_CFURLStringType</key><integer>0</integer>\
             </dict></dict></dict>",
            xml_escape(&path)
        );
        let ok = command(Path::new("/usr/bin/defaults"))
            .args(["write", "com.apple.dock", "persistent-apps", "-array-add", &tile])
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            let _ = command(Path::new("/usr/bin/killall")).arg("Dock").status();
        }
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    /// Lo saca del Dock (si lo había puesto) y reinicia el Dock.
    pub fn remove_from_dock(app: &Path) {
        let Ok(out) = command(Path::new("/usr/bin/defaults")).args(["export", "com.apple.dock", "-"]).output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let xml = String::from_utf8_lossy(&out.stdout);
        let Some(fixed) = crate::dock::dock_without(&xml, &app.to_string_lossy()) else { return };
        let Ok(mut child) = command(Path::new("/usr/bin/defaults"))
            .args(["import", "com.apple.dock", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(fixed.as_bytes());
        }
        if child.wait().is_ok_and(|s| s.success()) {
            let _ = command(Path::new("/usr/bin/killall")).arg("Dock").status();
        }
    }

    /// El Capyro.app instalado (en Aplicaciones, la de todos o la tuya) y su config.
    pub fn installed() -> Option<(PathBuf, Option<PathBuf>)> {
        let app = [PathBuf::from("/Applications/Capyro.app"), home().join("Applications/Capyro.app")]
            .into_iter()
            .find(|a| a.join("Contents/MacOS/Capyro").is_file())?;
        let config = std::fs::read_to_string(app_support().join(CONFIG_POINTER))
            .ok()
            .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(PathBuf::from))
            .or_else(|| Some(default_config_path()));
        Some((app, config))
    }

    /// En la Mac no hace falta (el .app ya se fue a la Papelera).
    pub fn delete_dir_later(_dir: &Path) {}

    /// Lo abre como si se tocara en el Finder.
    pub fn launch(app: &Path) -> std::io::Result<()> {
        command(Path::new("/usr/bin/open")).arg(app).spawn().map(|_| ())
    }

    /// A la Papelera, como arrastrarlo con el Finder.
    pub fn move_to_trash(path: &Path) -> Result<(), String> {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| e.localizedDescription().to_string())
    }
}

/// Linux: nada de registro ni accesos directos (simulación).
#[cfg(not(any(windows, target_os = "macos")))]
mod system {
    use super::*;

    pub fn integrate(_plan: &Plan, _exe: &Path, _version: &str) -> Result<(), String> {
        std::thread::sleep(std::time::Duration::from_millis(250));
        Ok(())
    }

    pub fn installed() -> Option<(PathBuf, Option<PathBuf>)> {
        let dir = default_install_dir();
        dir.join("capyro.exe").exists().then(|| (dir, Some(default_config_path())))
    }

    pub fn remove_registration() {}

    pub fn delete_dir_later(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    pub fn launch(exe: &Path) -> std::io::Result<()> {
        eprintln!("(simulación) acá se abriría {}", exe.display());
        Ok(())
    }
}

pub use system::launch;

/* ------------------------------------------------------------------------ */
/* desinstalar                                                              */
/* ------------------------------------------------------------------------ */

/// Dónde está instalado (según lo que se anotó al instalar) y su config.
pub fn installed() -> Option<(PathBuf, Option<PathBuf>)> {
    system::installed()
}

/// Programa el borrado de la carpeta para cuando cierre el desinstalador.
pub fn delete_dir_later(dir: &Path) {
    system::delete_dir_later(dir);
}

/// Mac: Capyro.app a la Papelera y fuera del Dock. La config del usuario
/// (en Application Support) queda.
#[cfg(target_os = "macos")]
pub fn uninstall(app: &Path) -> Result<(), String> {
    if !app.join("Contents/MacOS/Capyro").exists() {
        return Err(format!("No encontré Capyro en {}", app.display()));
    }
    system::move_to_trash(app).map_err(|e| format!("No pude moverlo a la Papelera: {e}"))?;
    system::remove_from_dock(app);
    Ok(())
}

/// Saca Capyro: accesos directos, registro y lo que hay en la carpeta (la
/// carpeta en sí, con uninstall.exe adentro, se borra al cerrar: ver
/// [`delete_dir_later`]). La config del usuario (en Documentos) queda.
#[cfg(not(target_os = "macos"))]
pub fn uninstall(dir: &Path) -> Result<(), String> {
    if !dir.join("capyro.exe").exists() {
        return Err(format!("No encontré Capyro en {}", dir.display()));
    }
    // ¿está abierto? (un .exe en uso no se puede borrar)
    let exe = dir.join("capyro.exe");
    let moved = dir.join("capyro.exe.borrando");
    if std::fs::rename(&exe, &moved).is_err() {
        return Err("Cerrá Capyro y probá de nuevo".into());
    }
    let _ = std::fs::remove_file(&moved);
    system::remove_registration();
    // lo que se pueda ahora; la carpeta entera cuando este proceso cierre
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_queda_en_win() {
        let lua = "local options = {\n    splash = true,\n    system        = \"auto\",  -- comentario\n    x = 1,\n}\n";
        let out = set_system_win(lua);
        assert!(out.contains("    system        = \"Win\",  -- comentario\n"), "{out}");
        assert!(out.contains("splash = true") && out.contains("x = 1"));
        assert_eq!(set_system_win(&out), out);
        // sin la opción no cambia nada
        assert_eq!(set_system_win("return {}\n"), "return {}\n");
        // la de Capyro de verdad
        let real = "    system        = \"Lin\",\n";
        assert_eq!(set_system_win(real), "    system        = \"Win\",\n");
    }

    #[test]
    fn system_queda_en_mac() {
        let real = "    system        = \"Lin\",\n";
        assert_eq!(set_system(real, "Mac"), "    system        = \"Mac\",\n");
        assert_eq!(set_system(&set_system(real, "Mac"), "Win"), "    system        = \"Win\",\n");
    }

    /// El paquete de la Mac: el Capyro.app con su ejecutable (que tiene que
    /// quedar ejecutable), un enlace, y lo del Finder que no va.
    #[cfg(unix)]
    #[test]
    fn instalar_el_app_de_la_mac() {
        use std::os::unix::fs::PermissionsExt;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let exe = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            let plain = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            w.add_directory("Capyro.app/", plain).unwrap();
            w.start_file("Capyro.app/Contents/MacOS/Capyro", exe).unwrap();
            w.write_all(b"binario").unwrap();
            w.start_file("Capyro.app/Contents/Frameworks/libavcodec.62.dylib", exe).unwrap();
            w.write_all(b"lib").unwrap();
            w.start_file("Capyro.app/Contents/Resources/version.txt", plain).unwrap();
            w.write_all(b"0.4.0-alpha\n").unwrap();
            w.add_symlink("Capyro.app/Contents/Resources/enlace", "version.txt", plain).unwrap();
            w.start_file("__MACOSX/Capyro.app/._Contents", plain).unwrap();
            w.write_all(b"basura").unwrap();
            w.start_file("Capyro.app/Contents/._Info.plist", plain).unwrap();
            w.write_all(b"basura").unwrap();
            w.finish().unwrap();
        }
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(buf.get_ref().as_slice())).unwrap();
        let base = std::env::temp_dir().join(format!("capyro-inst-mac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let staged = base.join(".Capyro-instalando.app");
        extract_app(&mut z, &staged, &|_| {}, (0.0, 1.0)).unwrap();
        let exe = staged.join("Contents/MacOS/Capyro");
        assert_eq!(std::fs::read(&exe).unwrap(), b"binario");
        assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read_link(staged.join("Contents/Resources/enlace")).unwrap(), Path::new("version.txt"));
        assert!(!staged.join("Contents/._Info.plist").exists());
        assert!(!base.join("__MACOSX").exists());
        // toma el lugar del que había
        let app = base.join("Capyro.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/viejo"), "v").unwrap();
        swap_app(&staged, &app).unwrap();
        assert!(app.join("Contents/MacOS/Capyro").is_file() && !app.join("Contents/viejo").exists());
        assert!(!staged.exists() && !base.join(".Capyro-viejo.app").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn instalar_un_paquete() {
        // un zip chiquito con la forma del de Capyro, adentro de "Capyro/"
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (n, d) in [
                ("Capyro/capyro.exe", "exe"),
                ("Capyro/avcodec-63.dll", "dll"),
                ("Capyro/version.txt", "0.3.0-alpha\n"),
                ("Capyro/config/capyro.lua", "local options = {\n    system        = \"auto\",\n}\n"),
            ] {
                w.start_file(n, o).unwrap();
                w.write_all(d.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(buf.get_ref().as_slice())).unwrap();
        assert_eq!(root_prefix(&mut z), "Capyro/");

        let base = std::env::temp_dir().join(format!("capyro-inst-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cfg = base.join("Docs/Capyro/Config/Capyro.lua");
        write_config(&cfg, Some("local options = {\n    system        = \"auto\",\n}\n")).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("system        = \"Win\""), "{text}");
        // si ya existe no se pisa
        std::fs::write(&cfg, "-- mía\nlocal options = {\n    system = \"Lin\",\n}\n").unwrap();
        write_config(&cfg, Some("otra")).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.starts_with("-- mía") && text.contains("system = \"Win\""), "{text}");

        let f = base.join("app/capyro.exe");
        write_replacing(&f, b"uno").unwrap();
        write_replacing(&f, b"dos").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"dos");
        assert_eq!(std::fs::read(base.join("app/capyro.exe.old")).unwrap(), b"uno");
        let _ = std::fs::remove_dir_all(&base);
    }
}
