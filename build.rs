//! 1. Si existe `payload/capyro-windows-x64.zip`, va adentro del instalador
//!    (instalador "offline"). Si no, el instalador lo baja de GitHub al
//!    instalar (instalador "online").
//! 2. En Windows: ícono, versión y manifiesto (.rc compilado con windres).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());

    /* --- el paquete --- */
    let payload = manifest_dir.join("payload").join("capyro-windows-x64.zip");
    println!("cargo:rerun-if-changed={}", payload.display());
    println!("cargo:rerun-if-env-changed=CAPYRO_PAYLOAD");
    let payload = std::env::var_os("CAPYRO_PAYLOAD").map(PathBuf::from).unwrap_or(payload);
    let code = if payload.is_file() {
        println!("cargo:warning=instalador offline: {}", payload.display());
        format!(
            "pub const PAYLOAD: Option<&[u8]> = Some(include_bytes!({:?}));\n",
            payload.canonicalize().unwrap()
        )
    } else {
        println!("cargo:warning=sin payload/capyro-windows-x64.zip: instalador online (baja Capyro de GitHub)");
        "pub const PAYLOAD: Option<&[u8]> = None;\n".to_owned()
    };
    std::fs::write(out.join("payload.rs"), code).unwrap();

    /* --- recursos de Windows --- */
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let assets = manifest_dir.join("assets");
    for f in ["installer.rc", "installer.manifest", "capyro.ico"] {
        println!("cargo:rerun-if-changed={}", assets.join(f).display());
    }
    let windres = std::env::var("WINDRES").ok().unwrap_or_else(|| {
        if Command::new("x86_64-w64-mingw32-windres").arg("--version").output().is_ok() {
            "x86_64-w64-mingw32-windres".into()
        } else {
            "windres".into()
        }
    });
    let res = out.join("installer_res.o");
    let status = Command::new(&windres)
        .current_dir(&assets)
        .arg("-O")
        .arg("coff")
        .arg("-I")
        .arg(&assets)
        .arg(assets.join("installer.rc"))
        .arg(&res)
        .status();
    match status {
        Ok(s) if s.success() => link(&res),
        other => println!("cargo:warning=no pude compilar los recursos con {windres} ({other:?}): el .exe queda sin ícono"),
    }
}

fn link(obj: &Path) {
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}
