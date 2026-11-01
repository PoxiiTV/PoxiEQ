// Sin consola en release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use poxieq_lib::engine;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Lo que lanza la tarea programada (ya elevada): activar los dispositivos nuevos apuntados en el archivo
    if let [flag, file] = args.as_slice() {
        if flag == "--auto-enable" {
            engine::auto_enable(file);
            return;
        }
    }
    // Proceso aparte para escanear plugins o abrir su editor (un plugin que revienta no tumba la app)
    if args.first().is_some_and(|a| a == "--plugin-host") {
        std::process::exit(poxieq_lib::plughost::main(&args[1..]));
    }
    // Copia elevada de la app (UAC): hace lo que necesita admin, deja el resultado en un archivo y sale
    if let [flag, out, rest @ ..] = args.as_slice() {
        if flag == "--elevated" {
            let report = if rest.first().is_some_and(|c| c == "uninstall") {
                engine::SetupReport { error: engine::uninstall().err(), checks: vec![] }
            } else if rest.first().is_some_and(|c| c == "task-create") {
                engine::SetupReport { error: rest.get(1).map_or(Err("falta el archivo".to_string()), |f| engine::task_create(f)).err(), checks: vec![] }
            } else if rest.first().is_some_and(|c| c == "task-delete") {
                engine::task_delete();
                engine::SetupReport::default()
            } else if rest.first().is_some_and(|c| c == "check") {
                // Solo GUID de dispositivo: es lo único que se acepta en la copia elevada
                let ids: Vec<String> = rest[1..].iter().filter(|id| engine::is_device_id(id)).cloned().collect();
                match engine::check(&ids) {
                    Ok(checks) => engine::SetupReport { error: None, checks },
                    Err(e) => engine::SetupReport { error: Some(e), checks: vec![] },
                }
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
