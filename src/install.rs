//! La instalación en sí: corre en otro hilo y le cuenta a la interfaz por
//! dónde va con [`Msg`].
//!
//! 1. El paquete (`capyro-windows-x64.zip`): el que viene adentro del
//!    instalador o, si no trae, el de la última versión para Windows
//!    publicada en GitHub (Releases del repo de Capyro).
//! 2. Se descomprime en la carpeta elegida. Lo que ya existía y está en uso
//!    (un Capyro abierto) se renombra a `.old`; Capyro lo borra solo al abrir.
//! 3. La config: si no existe, la de ejemplo con `system = "Win"`. Si ya
//!    existe no se pisa (solo se corrige `system`). Su ruta queda anotada en
//!    `capyro-config.txt`, al lado de capyro.exe.
//! 4. Windows: accesos directos, "Aplicaciones instaladas" y uninstall.exe.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Repo de Capyro (de dónde se baja si el instalador no trae el paquete).
pub const REPO: &str = "CapyIsInsano/Capyro";
/// La release que publica el CI del repo con la versión para Windows.
pub const WINDOWS_TAG: &str = "windows";
pub const ASSET: &str = "capyro-windows-x64.zip";
/// Clave de "Aplicaciones instaladas" (por usuario: no pide administrador).
#[cfg_attr(not(windows), allow(dead_code))]
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Capyro";
/// Formatos que abre Capyro ("Abrir con…").
#[cfg_attr(not(windows), allow(dead_code))]
pub const EXTENSIONS: [&str; 7] = [".mp4", ".mov", ".avi", ".mp3", ".wav", ".ogg", ".aac"];
/// Tipo de archivo de Capyro en el registro (ProgID).
#[cfg_attr(not(windows), allow(dead_code))]
pub const PROG_ID: &str = "Capyro.Media";
/// Dónde se anotan las capacidades (Aplicaciones predeterminadas).
#[cfg_attr(not(windows), allow(dead_code))]
pub const CAPABILITIES: &str = r"Software\Capyro\Capabilities";

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

/// true = esto no es Windows: se instala en una carpeta de prueba y no se
/// tocan accesos directos ni el registro (para probar la interfaz).
pub const SIMULATION: bool = !cfg!(windows);

/// Raíz de la "simulación" fuera de Windows.
#[cfg(not(windows))]
fn sim_root() -> PathBuf {
    std::env::temp_dir().join("capyro-installer-prueba")
}

/// `%LOCALAPPDATA%\Programs\Capyro` (donde instalan las apps por usuario).
pub fn default_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Capyro"))
            .join("Programs")
            .join("Capyro")
    }
    #[cfg(not(windows))]
    {
        sim_root().join("AppData/Local/Programs/Capyro")
    }
}

/// `Documentos\Capyro\Config\Capyro.lua`
pub fn default_config_path() -> PathBuf {
    #[cfg(windows)]
    let docs = dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from(r"C:\Capyro"));
    #[cfg(not(windows))]
    let docs = sim_root().join("Documents");
    docs.join("Capyro").join("Config").join("Capyro.lua")
}

/// La versión que trae el paquete de adentro (version.txt), si trae.
pub fn embedded_version() -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD?)).ok()?;
    let root = root_prefix(&mut zip);
    let mut f = zip.by_name(&format!("{root}version.txt")).ok()?;
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

fn install(plan: &Plan, send: &dyn Fn(Msg)) -> Result<(String, PathBuf), String> {
    send(Msg::Step("Preparando…".into()));
    send(Msg::Progress(0.0));

    // 1. el paquete
    let (span_start, mut release_version) = (if PAYLOAD.is_some() { 0.0 } else { 0.55 }, None);
    let downloaded;
    let bytes: &[u8] = match PAYLOAD {
        Some(p) => p,
        None => {
            let (data, version) = download_latest(send)?;
            downloaded = data;
            release_version = version;
            &downloaded
        }
    };
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("El paquete está dañado: {e}"))?;
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

/// `options.system` en "Win" (la línea `system = "..."` de capyro.lua).
pub fn set_system_win(lua: &str) -> String {
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
            out.push_str(&format!("{indent}system{pad}= \"Win\"{rest}"));
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
        let fixed = set_system_win(&old);
        if fixed != old {
            std::fs::write(path, fixed)?;
        }
        return Ok(());
    }
    let base = default.unwrap_or("-- Configuración de Capyro\nreturn { options = { system = \"Win\" } }\n");
    let text = format!("-- Instalado por CapyroInstaller.\n{}", set_system_win(base));
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
    let url = format!("https://api.github.com/repos/{}/releases/tags/{WINDOWS_TAG}", repo());
    let out = command(&curl())
        .args(["-sS", "-L", "--fail", "--connect-timeout", "15", "--max-time", "60"])
        .args(["-H", "Accept: application/vnd.github+json", "-A", "CapyroInstaller"])
        .arg(&url)
        .output()
        .map_err(|e| format!("No pude usar curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.contains("404") {
            "Todavía no hay una versión de Capyro para Windows publicada".into()
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
        .ok_or("La versión publicada no trae el paquete para Windows")?;
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
        }
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{PROG_ID}"));
        let _ = hkcu.delete_subkey_all(r"Software\Classes\Applications\capyro.exe");
        let _ = hkcu.delete_subkey_all(r"Software\Capyro");
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

/// Fuera de Windows: nada de registro ni accesos directos (simulación).
#[cfg(not(windows))]
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

/// Saca Capyro: accesos directos, registro y lo que hay en la carpeta (la
/// carpeta en sí, con uninstall.exe adentro, se borra al cerrar: ver
/// [`delete_dir_later`]). La config del usuario (en Documentos) queda.
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
