mod analyzer;
mod autoeq;
pub mod engine;
mod eq;

use eq::{Curve, Preset};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

// Instalador oficial de Equalizer APO, fijado por hash: si cambia, no se ejecuta
const APO_URL: &str = "https://downloads.sourceforge.net/project/equalizerapo/1.4.2/EqualizerAPO-x64-1.4.2.exe";
const APO_SHA256: &str = "7403be7427bbe1936a40dded082829b6e217fc4f5990fee5cba501f0ae055afa";
const BYPASS_HOTKEY: &str = "Ctrl+Alt+E";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// `None`: el idioma de Windows
    pub lang: Option<String>,
    /// `dark`, `light` o `system`
    pub theme: String,
    pub accent: String,
    pub close_to_tray: bool,
    pub bypass: bool,
    pub bypass_hotkey: bool,
    pub analyzer: bool,
    pub seen_version: String,
    /// Dispositivo que se está editando
    pub device: Option<String>,
    /// Curva de cada dispositivo (por GUID)
    pub curves: HashMap<String, Curve>,
    pub presets: Vec<Preset>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lang: None,
            theme: "dark".into(),
            accent: "cyan".into(),
            close_to_tray: true,
            bypass: false,
            bypass_hotkey: true,
            analyzer: true,
            seen_version: String::new(),
            device: None,
            curves: HashMap::new(),
            presets: vec![],
        }
    }
}

impl Config {
    fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn sanitize(&mut self) {
        if !matches!(self.theme.as_str(), "dark" | "light" | "system") {
            self.theme = "dark".into();
        }
        if !matches!(self.accent.as_str(), "cyan" | "blue" | "emerald" | "violet" | "pink" | "amber" | "red") {
            self.accent = "cyan".into();
        }
        if !matches!(self.lang.as_deref(), None | Some("es") | Some("en")) {
            self.lang = None;
        }
        self.curves.retain(|id, _| id.len() <= 64);
        self.curves.values_mut().for_each(Curve::sanitize);
        eq::sanitize_presets(&mut self.presets);
    }
}

pub struct AppState {
    config: Mutex<Config>,
    config_path: PathBuf,
    start_hidden: bool,
    tray_labels: Mutex<TrayLabels>,
}

impl AppState {
    fn cfg(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    code: String,
    detail: Option<String>,
}

type Res<T> = Result<T, Problem>;

fn problem(code: &str, detail: impl Into<Option<String>>) -> Problem {
    Problem { code: code.into(), detail: detail.into() }
}

// ------------------------------------------------------------------ comandos

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    portable: bool,
    start_hidden: bool,
}

#[tauri::command]
fn app_info(app: AppHandle, st: State<'_, AppState>) -> AppInfo {
    // El instalador NSIS deja `uninstall.exe` junto al exe; el portable no
    let portable = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| !d.join("uninstall.exe").exists())).unwrap_or(true);
    AppInfo { version: app.package_info().version.to_string(), portable: portable && !cfg!(debug_assertions), start_hidden: st.start_hidden }
}

#[tauri::command]
fn config_get(st: State<'_, AppState>) -> Config {
    st.cfg().clone()
}

/// Guarda la configuración y aplica la curva al momento (Equalizer APO relee el archivo solo).
#[tauri::command]
fn config_set(app: AppHandle, st: State<'_, AppState>, mut cfg: Config) -> Res<Config> {
    cfg.sanitize();
    let changed_bypass = st.cfg().bypass != cfg.bypass;
    let changed_hotkey = st.cfg().bypass_hotkey != cfg.bypass_hotkey;
    *st.cfg() = cfg.clone();
    if let Ok(json) = serde_json::to_string_pretty(&cfg) {
        if let Some(dir) = st.config_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&st.config_path, json);
    }
    if changed_bypass {
        refresh_tray(&app);
    }
    if changed_hotkey {
        register_hotkey(&app, cfg.bypass_hotkey);
    }
    apply(&cfg)?;
    Ok(cfg)
}

fn apply(cfg: &Config) -> Res<()> {
    if !engine::status().installed {
        return Ok(());
    }
    engine::write_config(&eq::render(&cfg.curves, cfg.bypass)).map_err(|e| problem("engine.write", e))
}

#[tauri::command]
async fn engine_status() -> engine::Status {
    tauri::async_runtime::spawn_blocking(engine::status).await.unwrap_or(engine::Status { installed: false, config_dir: None, devices: vec![] })
}

/// Instala el motor si falta (descarga + hash), activa/quita dispositivos, reinicia el audio y
/// comprueba. Un solo aviso de UAC.
#[tauri::command]
async fn engine_setup(app: AppHandle, enable: Vec<String>, disable: Vec<String>) -> Res<engine::SetupReport> {
    let valid = |id: &String| id.len() == 38 && id.starts_with('{') && id.ends_with('}') && id[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !enable.iter().chain(&disable).all(valid) {
        return Err(problem("engine.badDevice", None));
    }
    let mut args = vec!["setup".to_string()];
    if !engine::status().installed {
        let _ = app.emit("engine-progress", "download");
        let path = download_apo().await?;
        args.extend(["--installer".into(), path.to_string_lossy().into_owned()]);
    }
    let _ = app.emit("engine-progress", "install");
    let mut cmd_args = args.clone();
    cmd_args.extend(enable.iter().cloned());
    if !disable.is_empty() {
        cmd_args.push("--disable".into());
        cmd_args.extend(disable);
    }
    let out = tauri::async_runtime::spawn_blocking(move || engine::run_elevated(&cmd_args))
        .await
        .map_err(|_| problem("engine.setup", None))?
        .map_err(|e| problem("engine.elevation", e))?;
    let report: engine::SetupReport = serde_json::from_str(&out).map_err(|_| problem("engine.setup", None))?;
    // Recién instalado: escribir ya la curva que hubiera
    let cfg = app.state::<AppState>().cfg().clone();
    let _ = apply(&cfg);
    Ok(report)
}

async fn download_apo() -> Res<PathBuf> {
    let fail = |e: String| problem("engine.download", e);
    let client = reqwest::Client::builder().user_agent("PoxiEQ").build().map_err(|e| fail(e.to_string()))?;
    let bytes = client
        .get(APO_URL)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| fail(e.to_string()))?
        .bytes()
        .await
        .map_err(|e| fail(e.to_string()))?;
    let hash = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
    if hash != APO_SHA256 {
        return Err(problem("engine.hash", None));
    }
    let path = std::env::temp_dir().join("PoxiEQ-EqualizerAPO-1.4.2.exe");
    std::fs::write(&path, &bytes).map_err(|e| fail(e.to_string()))?;
    Ok(path)
}

#[tauri::command]
fn autostart_get(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn autostart_set(app: AppHandle, on: bool) -> Res<()> {
    use tauri_plugin_autostart::ManagerExt;
    let l = app.autolaunch();
    if on { l.enable() } else { l.disable() }.map_err(|e| problem("io.failed", e.to_string()))
}

#[derive(Deserialize, Clone)]
pub struct TrayLabels {
    open: String,
    bypass: String,
    quit: String,
}

#[tauri::command]
fn tray_labels(app: AppHandle, st: State<'_, AppState>, labels: TrayLabels) {
    *st.tray_labels.lock().unwrap_or_else(|e| e.into_inner()) = labels;
    refresh_tray(&app);
}

/// Quita el motor por completo (efectos originales restaurados). Un aviso de UAC.
#[tauri::command]
async fn engine_uninstall() -> Res<()> {
    let out = tauri::async_runtime::spawn_blocking(|| engine::run_elevated(&["uninstall".to_string()]))
        .await
        .map_err(|_| problem("engine.setup", None))?
        .map_err(|e| problem("engine.elevation", e))?;
    let report: engine::SetupReport = serde_json::from_str(&out).map_err(|_| problem("engine.setup", None))?;
    match report.error {
        Some(e) => Err(problem("engine.uninstall", e)),
        None => Ok(()),
    }
}

#[tauri::command]
async fn autoeq_index(app: AppHandle, force: bool) -> Res<Vec<autoeq::Entry>> {
    let cache = app.path().app_data_dir().map_err(|e| problem("io.failed", e.to_string()))?.join("autoeq-index.md");
    autoeq::index(&cache, force).await.map_err(|e| problem("autoeq.offline", e))
}

#[tauri::command]
async fn autoeq_fetch(path: String) -> Res<String> {
    autoeq::fetch(&path).await.map_err(|e| problem("autoeq.offline", e))
}

/// Analizador: solo con la ventana a la vista (la interfaz lo para al esconderse)
#[tauri::command]
fn analyzer_start(app: AppHandle, device: String) {
    analyzer::start(app, device);
}

#[tauri::command]
fn analyzer_stop() {
    analyzer::stop();
}

#[tauri::command]
fn app_quit(app: AppHandle) {
    app.exit(0);
}

// ------------------------------------------------------------ bandeja y atajo

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn refresh_tray(app: &AppHandle) {
    let st = app.state::<AppState>();
    let l = st.tray_labels.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let bypass = st.cfg().bypass;
    let build = || -> tauri::Result<Menu<tauri::Wry>> {
        Menu::with_items(
            app,
            &[
                &MenuItem::with_id(app, "open", &l.open, true, None::<&str>)?,
                &CheckMenuItem::with_id(app, "bypass", &l.bypass, true, bypass, None::<&str>)?,
                &PredefinedMenuItem::separator(app)?,
                &MenuItem::with_id(app, "quit", &l.quit, true, None::<&str>)?,
            ],
        )
    };
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build()) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Bypass desde la bandeja o el atajo: se guarda y se avisa a la interfaz para que lo pinte.
fn toggle_bypass(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut cfg = st.cfg().clone();
    cfg.bypass = !cfg.bypass;
    if config_set(app.clone(), st, cfg.clone()).is_ok() {
        let _ = app.emit("bypass", cfg.bypass);
    }
}

fn register_hotkey(app: &AppHandle, on: bool) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if on {
        let _ = gs.on_shortcut(BYPASS_HOTKEY, |app, _, ev| {
            if ev.state == ShortcutState::Pressed {
                toggle_bypass(app);
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_hidden = std::env::args().any(|a| a == "--hidden");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_window(app)))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let config_path = app.path().app_data_dir()?.join("config.json");
            let mut cfg = Config::load(&config_path);
            cfg.sanitize();
            app.manage(AppState {
                config: Mutex::new(cfg.clone()),
                config_path,
                start_hidden,
                tray_labels: Mutex::new(TrayLabels { open: "Abrir PoxiEQ".into(), bypass: "Bypass".into(), quit: "Salir".into() }),
            });
            // Las ventanas se crean con el estado ya listo: si la interfaz cargara antes, sus primeras
            // llamadas fallarían y la ventana se quedaría en negro
            for w in &app.config().app.windows {
                tauri::WebviewWindowBuilder::from_config(app.handle(), w)?.build()?;
            }
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().ok_or("sin icono")?)
                .tooltip("PoxiEQ")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| match ev.id().as_ref() {
                    "open" => show_window(app),
                    "bypass" => toggle_bypass(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;
            refresh_tray(app.handle());
            register_hotkey(app.handle(), cfg.bypass_hotkey);
            // Al arrancar se reescribe la curva: por si se tocó config.txt a mano o se reinstaló el motor
            let _ = apply(&cfg);
            Ok(())
        })
        .on_window_event(|w, ev| {
            if let WindowEvent::CloseRequested { api, .. } = ev {
                if w.app_handle().state::<AppState>().cfg().close_to_tray {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            config_get,
            config_set,
            engine_status,
            engine_setup,
            engine_uninstall,
            autoeq_index,
            autoeq_fetch,
            autostart_get,
            autostart_set,
            tray_labels,
            analyzer_start,
            analyzer_stop,
            app_quit,
        ])
        .run(tauri::generate_context!())
        .expect("no se ha podido arrancar PoxiEQ");
}
