// Sin consola en release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use poxieq_lib::engine;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Copia elevada de la app (UAC): hace lo que necesita admin, deja el resultado en un archivo y sale
    if let [flag, out, rest @ ..] = args.as_slice() {
        if flag == "--elevated" {
            let report = if rest.first().is_some_and(|c| c == "uninstall") {
                engine::SetupReport { error: engine::uninstall().err(), checks: vec![] }
            } else {
                setup(rest)
            };
            let _ = std::fs::write(out, serde_json::to_string(&report).unwrap_or_default());
            return;
        }
    }
    poxieq_lib::run()
}

/// `setup [--installer RUTA] [--dsp] ID... [--disable ID...]`
fn setup(args: &[String]) -> engine::SetupReport {
    let mut installer = None;
    let mut dsp = false;
    let (mut enable, mut disable) = (vec![], vec![]);
    let mut it = args.iter().skip(1);
    let mut disabling = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--installer" => installer = it.next().cloned(),
            "--disable" => disabling = true,
            "--dsp" => dsp = true,
            id if disabling => disable.push(id.to_string()),
            id => enable.push(id.to_string()),
        }
    }
    engine::setup(installer.as_deref(), &enable, &disable, dsp)
}
