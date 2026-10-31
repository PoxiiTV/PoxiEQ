use std::path::PathBuf;
use std::process::Command;

fn main() {
    // PoxiDSP.dll (el plugin de dinámica que carga el motor) se compila aparte: siempre en release, con
    // su propio perfil (panic = "unwind") y su propia carpeta target. El exe lo lleva dentro y lo
    // instala en el paso elevado, así que también llega con el portable.
    let dsp = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..").join("dsp");
    println!("cargo:rerun-if-changed=../dsp/src");
    println!("cargo:rerun-if-changed=../dsp/Cargo.toml");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--lib", "--manifest-path"])
        .arg(dsp.join("Cargo.toml"))
        // Nada de lo que la compilación de fuera pone para sí misma (clippy, flags, carpeta target)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .status()
        .expect("no se puede lanzar cargo para compilar PoxiDSP");
    assert!(status.success(), "no se ha podido compilar PoxiDSP (dsp/)");
    let dll = dsp.join("target").join("release").join("poxidsp.dll");
    assert!(dll.exists(), "falta {}", dll.display());
    println!("cargo:rustc-env=POXIDSP_DLL={}", dll.display());

    tauri_build::build()
}
