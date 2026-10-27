//! Si algo sale mal antes de que haya ventana (o se cae), que se vea: en
//! Windows un cartel y un log en %TEMP%\\CapyroInstaller.log.

use std::io::Write;

pub fn log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("CapyroInstaller.log")
}

pub fn log(msg: &str) {
    eprintln!("CapyroInstaller: {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "{msg}");
    }
}

/// Cartel de error (Windows) además del log.
pub fn fatal(msg: &str) {
    log(msg);
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let text = wide(&format!("{msg}\n\nDetalle en {}", log_path().display()));
        let caption = wide("Capyro - Installer");
        // SAFETY: cadenas UTF-16 terminadas en 0 que viven durante la llamada.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10); // MB_ICONERROR
        }
    }
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        fatal(&format!("El instalador se cerró por un error:\n{info}"));
    }));
}
