//! El mismo "clic" de Capyro al tocar un botón.

/// click.mp3 de Capyro pasado a WAV (Windows lo reproduce desde memoria).
static CLICK: &[u8] = include_bytes!("../assets/click.wav");

#[cfg(windows)]
pub fn click() {
    use std::ffi::c_void;
    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> i32;
    }
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;
    // SAFETY: CLICK es estático (vive mientras suena) y es un WAV válido.
    unsafe {
        PlaySoundW(CLICK.as_ptr().cast(), std::ptr::null_mut(), SND_ASYNC | SND_MEMORY | SND_NODEFAULT);
    }
}

/// Fuera de Windows (solo para probar la interfaz): paplay o aplay si están.
#[cfg(not(windows))]
pub fn click() {
    std::thread::spawn(|| {
        let path = std::env::temp_dir().join("capyro-installer-click.wav");
        if !path.exists() && std::fs::write(&path, CLICK).is_err() {
            return;
        }
        for player in ["paplay", "aplay"] {
            let ok = std::process::Command::new(player)
                .arg(&path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn es_un_wav() {
        assert_eq!(&super::CLICK[..4], b"RIFF");
        assert_eq!(&super::CLICK[8..12], b"WAVE");
    }
}
