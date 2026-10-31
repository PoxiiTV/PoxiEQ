mod analyzer;
mod apps;
mod autoeq;
pub mod engine;
mod eq;
mod hotkeys;
mod osd;
mod room;

use eq::{Curve, DeviceSettings, Live, Loudness, Mic, Preset, SectionPresets};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

// Instalador oficial de Equalizer APO, fijado por hash: si cambia, no se ejecuta
const APO_URL: &str = "https://downloads.sourceforge.net/project/equalizerapo/1.4.2/EqualizerAPO-x64-1.4.2.exe";
const APO_SHA256: &str = "7403be7427bbe1936a40dded082829b6e217fc4f5990fee5cba501f0ae055afa";

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
    pub analyzer: bool,
    /// `spectrum` (espectro con retención de picos) o `spectrogram`
    pub analyzer_mode: String,
    pub seen_version: String,
    /// Salida que se está editando
    pub device: Option<String>,
    /// Micrófono que se edita en su página (`None`: el predeterminado)
    pub mic_device: Option<String>,
    /// Al cambiar Windows la salida predeterminada (cascos enchufados, Bluetooth…), editar esa
    pub follow_default: bool,
    /// Curva de cada dispositivo (por GUID)
    pub curves: HashMap<String, Curve>,
    pub presets: Vec<Preset>,
    pub loudness: Loudness,
    pub meter_collapsed: bool,
    /// Formato anterior: una sola dinámica (Sonido + Cascos) para todas las salidas. Solo se lee, para
    /// repartirla entre los dispositivos (`migrate_per_device`).
    #[serde(skip_serializing)]
    pub dsp: Option<poxidsp::Settings>,
    /// Lo que recuerda cada dispositivo (por GUID) aparte de su curva: su PoxiDSP y el último preset de cada
    /// sección. Sin entrada, lo de serie (Natural, sin crossfeed).
    pub per_device: HashMap<String, DeviceSettings>,
    /// Presets propios de Sonido, Cascos, Estudio y Micrófono
    pub section_presets: SectionPresets,
    /// Preset automático por aplicación en primer plano
    pub app_rules: Vec<apps::AppRule>,
    /// Atajos globales: bypass y un preset por hueco, cada uno con su combinación
    pub hotkeys: hotkeys::Hotkeys,
    /// Formato anterior de los atajos (Ctrl+Alt+E y Ctrl+Alt+1…9): solo se lee, para migrarlo
    #[serde(skip_serializing)]
    pub bypass_hotkey: Option<bool>,
    #[serde(skip_serializing)]
    pub preset_keys: Option<Vec<Option<String>>>,
    /// Micrófono: su propio bypass (su EQ va en `curves` y su puerta/compresor en `per_device`)
    pub mic: Mic,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lang: None,
            theme: "dark".into(),
            accent: "cyan".into(),
            close_to_tray: true,
            bypass: false,
            analyzer: true,
            analyzer_mode: "spectrum".into(),
            seen_version: String::new(),
            device: None,
            mic_device: None,
            follow_default: true,
            curves: HashMap::new(),
            presets: vec![],
            loudness: Loudness::default(),
            meter_collapsed: false,
            dsp: None,
            per_device: HashMap::new(),
            section_presets: SectionPresets::default(),
            app_rules: vec![],
            hotkeys: hotkeys::Hotkeys::default(),
            bypass_hotkey: None,
            preset_keys: None,
            mic: Mic::default(),
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
        self.loudness.sanitize();
        self.section_presets.sanitize();
        eq::sanitize_devices(&mut self.per_device, &self.section_presets);
        if !matches!(self.analyzer_mode.as_str(), "spectrum" | "spectrogram") {
            self.analyzer_mode = "spectrum".into();
        }
        if self.device.as_deref().is_some_and(|d| !engine::is_device_id(d)) {
            self.device = None;
        }
        if self.mic_device.as_deref().is_some_and(|d| !engine::is_device_id(d)) {
            self.mic_device = None;
        }
        apps::sanitize_rules(&mut self.app_rules);
        if self.bypass_hotkey.is_some() || self.preset_keys.is_some() {
            self.hotkeys = hotkeys::Hotkeys::migrate(self.bypass_hotkey.take(), self.preset_keys.take());
        }
        self.hotkeys.sanitize(apps::valid_preset_id);
    }

    /// Antes la dinámica de las salidas (Sonido y Cascos) era una para todas y la puerta/compresor del
    /// micrófono, otra para todos: cada una pasa tal cual a los dispositivos de su tipo que ya se usaban
    /// (`used`: id y si es una entrada) y que aún no tienen la suya. Los demás empiezan con lo de serie.
    fn migrate_per_device(&mut self, used: &[(String, bool)]) {
        let (outputs, inputs) = (self.dsp.take(), self.mic.dsp.take());
        for (id, input) in used {
            if let Some(mut dsp) = if *input { inputs } else { outputs } {
                dsp.sanitize();
                self.per_device.entry(id.clone()).or_insert(DeviceSettings { dsp, ..DeviceSettings::default() });
            }
        }
    }

    /// Lo que pinta el menú de la bandeja: si cambia, se rehace
    fn tray_key(&self) -> (bool, Option<String>, BTreeMap<String, Option<String>>) {
        (self.bypass, self.device.clone(), self.curves.iter().map(|(k, c)| (k.clone(), c.preset.clone())).collect())
    }
}

/// Preset listo para aplicarse desde Rust (atajos, bandeja, reglas por app). Lo manda la interfaz con el
/// nombre ya traducido y, en `auto_gain`, el preamp automático de su curva (la matemática vive en `eq.ts`).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetEntry {
    id: String,
    name: String,
    curve: Curve,
    auto_gain: f64,
}

pub struct AppState {
    config: Mutex<Config>,
    config_path: PathBuf,
    start_hidden: bool,
    tray_labels: Mutex<TrayLabels>,
    /// Lo momentáneo (A/B, herramientas de estudio, test de audición): no se guarda
    live: Mutex<Live>,
    presets: Mutex<Vec<PresetEntry>>,
    /// Hilo que vigila la app en primer plano (se despierta al cambiar las reglas)
    watcher: OnceLock<std::thread::Thread>,
    /// Atajos que Windows no ha dejado registrar (los tiene otro programa)
    hotkeys_failed: Mutex<Vec<String>>,
    /// Corrección de sala: respuesta de cada posición medida (potencia en la rejilla de `room`)
    room: Mutex<Vec<Vec<f64>>>,
    /// Última salida predeterminada de Windows vista (para notar cuándo cambia)
    default_output: Mutex<Option<String>>,
}

impl AppState {
    fn cfg(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn presets(&self) -> MutexGuard<'_, Vec<PresetEntry>> {
        self.presets.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn labels(&self) -> TrayLabels {
        self.tray_labels.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn live(&self) -> MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn room(&self) -> MutexGuard<'_, Vec<Vec<f64>>> {
        self.room.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn failed(&self) -> MutexGuard<'_, Vec<String>> {
        self.hotkeys_failed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn default_output(&self) -> MutexGuard<'_, Option<String>> {
        self.default_output.lock().unwrap_or_else(|e| e.into_inner())
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
fn config_set(app: AppHandle, st: State<'_, AppState>, cfg: Config) -> Res<Config> {
    let cfg = store(&app, cfg);
    apply(&cfg, &st.live())?;
    Ok(cfg)
}

/// Valida, guarda en disco y pone al día lo que depende de la configuración (bandeja, atajos, reglas)
fn store(app: &AppHandle, mut cfg: Config) -> Config {
    let st = app.state::<AppState>();
    cfg.sanitize();
    let (tray, keys, rules) = {
        let old = st.cfg();
        (old.tray_key(), old.hotkeys.clone(), old.app_rules.clone())
    };
    *st.cfg() = cfg.clone();
    save(&st.config_path, &cfg);
    if tray != cfg.tray_key() {
        refresh_tray(app);
    }
    if keys != cfg.hotkeys {
        register_hotkeys(app, &cfg);
    }
    if rules != cfg.app_rules {
        if let Some(t) = st.watcher.get() {
            t.unpark();
        }
    }
    cfg
}

fn save(path: &std::path::Path, cfg: &Config) {
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, json);
    }
}

/// Configuración y estado momentáneo con los que arranca la app. Lo del formato anterior se reparte entre los
/// dispositivos que ya se usaban (`used`) y, siguiendo a Windows, se empieza en su salida predeterminada. Cada
/// dispositivo suena con lo suyo (lo escribe `apply`). Lo momentáneo empieza siempre limpio: Estudio en
/// estéreo normal y sin simulación, aunque haya presets suyos guardados.
fn startup(mut cfg: Config, used: &[(String, bool)], default_output: Option<String>, is_input: impl Fn(&str) -> bool) -> (Config, Live) {
    cfg.sanitize();
    cfg.migrate_per_device(used);
    // Antes el micrófono se elegía en el mismo selector que las salidas
    if cfg.device.as_deref().is_some_and(&is_input) {
        cfg.mic_device = cfg.device.take();
    }
    if cfg.follow_default && default_output.is_some() {
        cfg.device = default_output;
    }
    (cfg, Live::default())
}

/// Dispositivos que ya se usaban (activados en el motor o con curva propia), solo si hay algo que migrar
fn used_devices(cfg: &Config) -> Vec<(String, bool)> {
    if cfg.dsp.is_none() && cfg.mic.dsp.is_none() {
        return vec![];
    }
    let mut used: Vec<(String, bool)> = engine::status().devices.into_iter().filter(|d| d.enabled).map(|d| (d.id, d.input)).collect();
    for id in cfg.curves.keys().filter(|id| engine::is_device_id(id)) {
        if !used.iter().any(|(u, _)| u == id) {
            used.push((id.clone(), engine::is_input(id)));
        }
    }
    used
}

/// Cambio hecho desde Rust (atajo, bandeja, regla por app): se guarda, suena y se avisa a la interfaz
fn commit(app: &AppHandle, cfg: Config) {
    let cfg = store(app, cfg);
    let _ = app.emit("config", &cfg);
    let live = *app.state::<AppState>().live();
    let _ = apply(&cfg, &live);
}

/// Curva actual de la salida que se edita (sin curva guardada, la plana de serie, como en la interfaz)
fn current_curve(st: &AppState) -> Option<(String, Curve)> {
    let pref = st.cfg().device.clone();
    let dev = engine::active_device(pref.as_deref())?;
    let saved = st.cfg().curves.get(&dev).cloned();
    let curve = saved.or_else(|| st.presets().iter().find(|p| p.id == "b:flat").map(|p| Curve { preset: Some(p.id.clone()), ..p.curve.clone() }));
    Some((dev, curve.unwrap_or_default()))
}

/// Pone un preset en la salida que se edita, igual que `applyPreset` en la interfaz. Devuelve su nombre.
fn apply_preset(app: &AppHandle, id: &str) -> Option<String> {
    let st = app.state::<AppState>();
    let entry = st.presets().iter().find(|p| p.id == id).cloned()?;
    let (dev, cur) = current_curve(&st)?;
    let auto = cur.auto_preamp || entry.curve.auto_preamp;
    let curve = Curve {
        preamp: if auto { entry.auto_gain } else { entry.curve.preamp },
        auto_preamp: auto,
        preset: Some(entry.id),
        ..entry.curve
    };
    let mut cfg = st.cfg().clone();
    cfg.curves.insert(dev, curve);
    commit(app, cfg);
    Some(entry.name)
}

/// La interfaz manda los presets (de serie ya traducidos y propios) para usarlos sin ella: bandeja y atajos
#[tauri::command]
fn presets_sync(app: AppHandle, st: State<'_, AppState>, mut presets: Vec<PresetEntry>) {
    presets.truncate(300);
    presets.retain_mut(|p| {
        p.name = eq::clean_name(&p.name);
        p.curve.sanitize();
        p.auto_gain = eq::clamp(p.auto_gain, -30.0, 12.0, 0.0);
        apps::valid_preset_id(&p.id) && !p.name.is_empty()
    });
    *st.presets() = presets;
    refresh_tray(&app);
}

/// Apps abiertas con ventana, para crear una regla sin escribir rutas
#[tauri::command]
async fn apps_list() -> Vec<apps::OpenApp> {
    tauri::async_runtime::spawn_blocking(apps::list).await.unwrap_or_default()
}

/// Guarda un preset compartido como `.poxieq` en Descargas y lo enseña en el Explorador
#[tauri::command]
fn preset_export(app: AppHandle, name: String, code: String) -> Res<String> {
    if !share_code_ok(&code) {
        return Err(problem("share.bad", None));
    }
    let dir = app.path().download_dir().or_else(|_| app.path().document_dir()).map_err(|e| problem("io.failed", e.to_string()))?;
    let stem = file_stem(&name);
    let mut path = dir.join(format!("{stem}.poxieq"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).poxieq"));
        n += 1;
    }
    std::fs::write(&path, &code).map_err(|e| problem("io.failed", e.to_string()))?;
    let _ = tauri_plugin_opener::reveal_item_in_dir(&path);
    Ok(path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Código de preset compartido (`PXEQ2.` + base64url, o `PXEQ1.` de los primeros, solo del ecualizador): solo
/// eso y de tamaño razonable. Su contenido lo valida la interfaz al importarlo (`share.ts`).
fn share_code_ok(code: &str) -> bool {
    let body = code.strip_prefix("PXEQ2.").or_else(|| code.strip_prefix("PXEQ1."));
    code.len() <= 8192 && body.is_some_and(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
}

/// Nombre de archivo válido en Windows a partir del nombre del preset
fn file_stem(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c)).take(48).collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']);
    let upper = clean.to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && upper.as_bytes()[3].is_ascii_digit());
    if clean.is_empty() || reserved {
        "preset".into()
    } else {
        clean.into()
    }
}

fn apply(cfg: &Config, live: &Live) -> Res<()> {
    if !engine::status().installed {
        return Ok(());
    }
    let text = eq::render(&cfg.curves, &cfg.per_device, cfg.bypass, &cfg.loudness, cfg.mic.bypass, live, engine::is_input);
    engine::write_config(&text).map_err(|e| problem("engine.write", e))
}

/// Cambia lo momentáneo y lo hace sonar
fn set_live(st: &AppState, change: impl FnOnce(&mut Live)) -> Res<()> {
    let live = {
        let mut live = st.live();
        change(&mut live);
        *live
    };
    let cfg = st.cfg().clone();
    apply(&cfg, &live)
}

/// A/B igualado: mientras se mantiene pulsado suena el original con `gain` dB para compararlo al mismo volumen
#[tauri::command]
fn compare_set(st: State<'_, AppState>, gain: Option<f64>) -> Res<()> {
    set_live(&st, |l| l.compare = gain.filter(|g| g.is_finite()))
}

/// Herramientas de estudio (monitorización y simulación de otros sistemas). Lo que no es válido ni llega:
/// serde rechaza cualquier valor fuera de los enumerados.
#[tauri::command]
fn studio_set(st: State<'_, AppState>, studio: eq::Studio) -> Res<()> {
    set_live(&st, |l| l.studio = studio)
}

/// Test de audición: mientras dura, todo en plano (sin EQ, dinámica ni herramientas de estudio)
#[tauri::command]
fn hearing_set(st: State<'_, AppState>, on: bool) -> Res<()> {
    set_live(&st, |l| {
        l.hearing = on;
        l.studio = eq::Studio::default();
    })
}

/// Al esconder la ventana o salir no se queda nada momentáneo puesto (mono, simulación, test, medición…)
/// sin que se vea
fn reset_live(app: &AppHandle) {
    ROOM_CANCEL.store(true, Ordering::SeqCst);
    let st = app.state::<AppState>();
    if *st.live() != Live::default() {
        let _ = set_live(&st, |l| *l = Live::default());
        let _ = app.emit("live-reset", ());
    }
}

#[tauri::command]
fn meter_reset() {
    analyzer::reset();
}

#[tauri::command]
async fn engine_status() -> engine::Status {
    let status = || engine::Status { dsp: engine::dsp_state(), ..engine::status() };
    tauri::async_runtime::spawn_blocking(status).await.unwrap_or(engine::Status {
        installed: false,
        config_dir: None,
        devices: vec![],
        dsp: engine::DspState::Missing,
    })
}

/// Instala el motor si falta (descarga + hash) y PoxiDSP si falta o es de otra versión, activa/quita
/// dispositivos, reinicia el audio y comprueba. Un solo aviso de UAC.
#[tauri::command]
async fn engine_setup(app: AppHandle, enable: Vec<String>, disable: Vec<String>) -> Res<engine::SetupReport> {
    if !enable.iter().chain(&disable).all(|id| engine::is_device_id(id)) {
        return Err(problem("engine.badDevice", None));
    }
    let dsp = engine::dsp_state() != engine::DspState::Current;
    if enable.is_empty() && disable.is_empty() && !dsp {
        return Ok(engine::SetupReport::default());
    }
    let mut args = vec!["setup".to_string()];
    if dsp {
        args.push("--dsp".into());
    }
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
    let st = app.state::<AppState>();
    let cfg = st.cfg().clone();
    let live = *st.live();
    let _ = apply(&cfg, &live);
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

/// Textos que pinta Rust (bandeja y aviso en pantalla), en el idioma de la interfaz
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrayLabels {
    open: String,
    bypass: String,
    quit: String,
    presets: String,
    eq_on: String,
    eq_off: String,
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

/// Analizador: solo con la ventana a la vista (la interfaz lo para al esconderse). De una salida mide lo que
/// suena; de un micrófono, lo que llega de él ya procesado.
#[tauri::command]
fn analyzer_start(app: AppHandle, device: String) {
    if engine::is_device_id(&device) {
        let input = engine::is_input(&device);
        analyzer::start(app, device, input);
    }
}

// ---------------------------------------------------------- corrección de sala

/// Una medición cada vez, y la petición de cortarla (cancelar, esconder la ventana, salir)
static ROOM_BUSY: AtomicBool = AtomicBool::new(false);
static ROOM_CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomLevels {
    /// dBFS en el micrófono: ruido de fondo antes del barrido y pico durante
    noise: f64,
    peak: f64,
    /// Posiciones medidas
    takes: usize,
}

/// Salida de verdad y micrófono de verdad
fn room_devices(output: &str, input: &str) -> Res<()> {
    if engine::is_device_id(output) && !engine::is_input(output) && engine::is_input(input) {
        Ok(())
    } else {
        Err(problem("engine.badDevice", None))
    }
}

/// Barrido por la salida mientras graba el micro. Mientras suena todo va en plano (EQ, PoxiDSP, estudio y
/// micrófono, como el A/B pero entero) y al terminar, falle o se cancele, vuelve lo de siempre.
fn room_take(app: &AppHandle, output: &str, input: &str, level: f64, secs: f64) -> Res<room::Take> {
    if ROOM_BUSY.swap(true, Ordering::SeqCst) {
        return Err(problem("room.busy", None));
    }
    ROOM_CANCEL.store(false, Ordering::SeqCst);
    let st = app.state::<AppState>();
    let result = set_live(&st, |l| l.measuring = true).and_then(|_| {
        // El motor relee el archivo al momento; margen para que no suene nada procesado
        std::thread::sleep(Duration::from_millis(400));
        let level = if level.is_finite() { level } else { room::MIN_LEVEL };
        room::record(output, input, level, secs, &ROOM_CANCEL).map_err(|e| match e {
            room::Error::Cancelled => problem("room.cancelled", None),
            room::Error::Format => problem("room.format", None),
            room::Error::Device(d) => problem("room.device", d),
        })
    });
    let _ = set_live(&st, |l| l.measuring = false);
    ROOM_BUSY.store(false, Ordering::SeqCst);
    result
}

/// Barrido corto para ver el nivel que llega al micrófono
#[tauri::command]
async fn room_check(app: AppHandle, output: String, input: String, level: f64) -> Res<RoomLevels> {
    room_devices(&output, &input)?;
    let handle = app.clone();
    let take = tauri::async_runtime::spawn_blocking(move || room_take(&handle, &output, &input, level, room::CHECK_SECS))
        .await
        .map_err(|_| problem("room.device", None))??;
    let takes = app.state::<AppState>().room().len();
    Ok(RoomLevels { noise: take.noise, peak: take.peak, takes })
}

/// Mide una posición y la añade a la media (como mucho `MAX_TAKES`: la más antigua sale)
#[tauri::command]
async fn room_measure(app: AppHandle, output: String, input: String, level: f64) -> Res<RoomLevels> {
    room_devices(&output, &input)?;
    let handle = app.clone();
    let (take, grid) = tauri::async_runtime::spawn_blocking(move || {
        let take = room_take(&handle, &output, &input, level, room::MEASURE_SECS)?;
        let grid = take.analyze();
        Ok::<_, Problem>((take, grid))
    })
    .await
    .map_err(|_| problem("room.device", None))??;
    let st = app.state::<AppState>();
    let mut takes = st.room();
    if takes.len() >= room::MAX_TAKES {
        takes.remove(0);
    }
    takes.push(grid);
    Ok(RoomLevels { noise: take.noise, peak: take.peak, takes: takes.len() })
}

#[tauri::command]
fn room_reset(st: State<'_, AppState>) {
    st.room().clear();
}

#[tauri::command]
fn room_cancel() {
    ROOM_CANCEL.store(true, Ordering::SeqCst);
}

/// Curva y filtros con los ajustes elegidos (no vuelve a medir)
#[tauri::command]
async fn room_fit(app: AppHandle, mut params: room::FitParams) -> Res<room::Fit> {
    params.sanitize();
    let takes = app.state::<AppState>().room().clone();
    if takes.is_empty() {
        return Err(problem("room.empty", None));
    }
    tauri::async_runtime::spawn_blocking(move || room::fit(&room::average(&takes), &params)).await.map_err(|_| problem("room.empty", None))
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
    let l = st.labels();
    let bypass = st.cfg().bypass;
    let active = current_curve(&st).and_then(|(_, c)| c.preset);
    let presets: Vec<(String, String)> = st.presets().iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    let build = || -> tauri::Result<Menu<tauri::Wry>> {
        // Submenú de presets: los de serie, una raya y los propios; con check el que suena
        let items = presets
            .iter()
            .map(|(id, name)| CheckMenuItem::with_id(app, format!("preset:{id}"), name, true, active.as_deref() == Some(id.as_str()), None::<&str>))
            .collect::<tauri::Result<Vec<_>>>()?;
        let mine = PredefinedMenuItem::separator(app)?;
        let mut refs: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![];
        for (i, item) in items.iter().enumerate() {
            if i > 0 && presets[i - 1].0.starts_with("b:") && !presets[i].0.starts_with("b:") {
                refs.push(&mine);
            }
            refs.push(item);
        }
        Menu::with_items(
            app,
            &[
                &MenuItem::with_id(app, "open", &l.open, true, None::<&str>)?,
                &CheckMenuItem::with_id(app, "bypass", &l.bypass, true, bypass, None::<&str>)?,
                &Submenu::with_items(app, &l.presets, !refs.is_empty(), &refs)?,
                &PredefinedMenuItem::separator(app)?,
                &MenuItem::with_id(app, "quit", &l.quit, true, None::<&str>)?,
            ],
        )
    };
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build()) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Bypass desde la bandeja o el atajo; con el atajo, además, aviso en pantalla
fn toggle_bypass(app: &AppHandle, notify: bool) {
    let st = app.state::<AppState>();
    let mut cfg = st.cfg().clone();
    cfg.bypass = !cfg.bypass;
    let off = cfg.bypass;
    let keys = cfg.hotkeys.bypass.as_deref().map(hotkeys::label).unwrap_or_default();
    commit(app, cfg);
    if notify {
        let l = st.labels();
        osd::show(app, if off { &l.eq_off } else { &l.eq_on }, &keys, if off { "off" } else { "on" });
    }
}

/// Atajo de un hueco de preset: el preset asignado, con aviso en pantalla
fn preset_hotkey(app: &AppHandle, slot: usize) {
    let key = app.state::<AppState>().cfg().hotkeys.presets.get(slot).cloned().unwrap_or_default();
    if let Some(name) = key.preset.and_then(|id| apply_preset(app, &id)) {
        osd::show(app, &name, &key.keys.as_deref().map(hotkeys::label).unwrap_or_default(), "preset");
    }
}

/// Atajos globales: el de bypass y los huecos de preset con combinación y preset. Un atajo registrado deja de
/// llegar a las demás apps; si otro programa ya lo tiene, Windows no deja registrarlo: se apunta y se avisa.
fn register_hotkeys(app: &AppHandle, cfg: &Config) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut failed = vec![];
    if let Some(k) = &cfg.hotkeys.bypass {
        let ok = gs.on_shortcut(k.as_str(), |app, _, ev| {
            if ev.state == ShortcutState::Pressed {
                toggle_bypass(app, true);
            }
        });
        if ok.is_err() {
            failed.push(k.clone());
        }
    }
    for (slot, p) in cfg.hotkeys.presets.iter().enumerate() {
        let (Some(k), Some(_)) = (&p.keys, &p.preset) else { continue };
        let ok = gs.on_shortcut(k.as_str(), move |app, _, ev| {
            if ev.state == ShortcutState::Pressed {
                preset_hotkey(app, slot);
            }
        });
        if ok.is_err() {
            failed.push(k.clone());
        }
    }
    *app.state::<AppState>().failed() = failed.clone();
    let _ = app.emit("hotkeys-failed", failed);
}

/// Comprueba una combinación grabada en Ajustes: su forma canónica o el motivo del rechazo
#[tauri::command]
fn hotkey_check(keys: String) -> Res<String> {
    hotkeys::check(&keys).map_err(|code| problem(code, None))
}

/// Mientras se graba un atajo no hay ninguno registrado: si no, Windows se lo quedaría antes de llegar a la
/// ventana (y saltaría el bypass en vez de grabarse)
#[tauri::command]
fn hotkeys_pause(app: AppHandle, st: State<'_, AppState>, paused: bool) {
    if paused {
        let _ = app.global_shortcut().unregister_all();
    } else {
        let cfg = st.cfg().clone();
        register_hotkeys(&app, &cfg);
    }
}

/// Atajos que no se han podido registrar (para avisar en Ajustes)
#[tauri::command]
fn hotkeys_failed(st: State<'_, AppState>) -> Vec<String> {
    st.failed().clone()
}

/// Preset automático por app: sondeo ligero (cada 500 ms) de la ventana en primer plano.
/// Sin reglas activas el hilo se queda dormido hasta que cambien.
fn watch_foreground(app: AppHandle) -> std::thread::Thread {
    std::thread::spawn(move || {
        let own = std::env::current_exe().ok().and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned())).unwrap_or_default();
        let mut auto = apps::Auto::new(&own);
        let mut seen: Vec<apps::AppRule> = vec![];
        loop {
            let st = app.state::<AppState>();
            let rules: Vec<apps::AppRule> = st.cfg().app_rules.iter().filter(|r| r.on).cloned().collect();
            if rules != seen {
                auto.rearm();
                seen = rules.clone();
            }
            if rules.is_empty() {
                if auto.active() {
                    let cur = current_curve(&st);
                    run_step(&app, auto.release(cur.as_ref().map(|(d, c)| (d.as_str(), c))));
                }
                std::thread::park();
                continue;
            }
            if let Some(exe) = apps::foreground_exe().filter(|e| !auto.seen(e)) {
                let cur = current_curve(&st);
                run_step(&app, auto.step(&exe, &rules, cur.as_ref().map(|(d, c)| (d.as_str(), c))));
            }
            std::thread::park_timeout(Duration::from_millis(500));
        }
    })
    .thread()
    .clone()
}

fn run_step(app: &AppHandle, step: apps::Step) {
    match step {
        apps::Step::Keep => {}
        apps::Step::Apply(id) => {
            apply_preset(app, &id);
        }
        apps::Step::Restore(dev, curve) => {
            let mut cfg = app.state::<AppState>().cfg().clone();
            cfg.curves.insert(dev, curve);
            commit(app, cfg);
        }
    }
}

// ------------------------------------------------- salida predeterminada

/// Salida a la que hay que pasar porque Windows ha cambiado la predeterminada de `before` a `now`: solo si se
/// sigue y no es ya la que se edita. Elegir otra a mano no apaga la opción: el siguiente cambio vuelve a mandar.
fn follow_target(cfg: &Config, before: Option<&str>, now: Option<&str>) -> Option<String> {
    let now = now.filter(|n| cfg.follow_default && before != Some(*n))?;
    (cfg.device.as_deref() != Some(now)).then(|| now.to_string())
}

/// Windows ha cambiado de salida predeterminada o se ha conectado, desconectado o activado algo. Se hace aquí y
/// no en la interfaz para que valga también con la ventana escondida (atajos, bandeja y reglas por app usan la
/// salida que se edita). La interfaz recibe `devices` con la salida a la que se ha pasado (o nada) y refresca.
fn devices_changed(app: &AppHandle) {
    let st = app.state::<AppState>();
    let now = engine::default_device(false);
    let before = std::mem::replace(&mut *st.default_output(), now.clone());
    let cfg = st.cfg().clone();
    let target = follow_target(&cfg, before.as_deref(), now.as_deref());
    if let Some(id) = &target {
        commit(app, Config { device: Some(id.clone()), ..cfg });
    }
    let _ = app.emit("devices", target);
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
            let loaded = Config::load(&config_path);
            let legacy = loaded.dsp.is_some() || loaded.mic.dsp.is_some();
            let used = used_devices(&loaded);
            let default_output = engine::default_device(false);
            let (cfg, live) = startup(loaded, &used, default_output.clone(), engine::is_input);
            if legacy {
                // Repartido una vez: el formato anterior ya no se vuelve a leer
                save(&config_path, &cfg);
            }
            app.manage(AppState {
                config: Mutex::new(cfg.clone()),
                config_path,
                start_hidden,
                tray_labels: Mutex::new(TrayLabels {
                    open: "Abrir PoxiEQ".into(),
                    bypass: "Bypass".into(),
                    quit: "Salir".into(),
                    presets: "Presets".into(),
                    eq_on: "EQ encendido".into(),
                    eq_off: "EQ apagado".into(),
                }),
                live: Mutex::new(live),
                presets: Mutex::new(vec![]),
                watcher: OnceLock::new(),
                hotkeys_failed: Mutex::new(vec![]),
                room: Mutex::new(vec![]),
                default_output: Mutex::new(default_output),
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
                    "bypass" => toggle_bypass(app, false),
                    "quit" => app.exit(0),
                    id => {
                        if let Some(id) = id.strip_prefix("preset:") {
                            apply_preset(app, id);
                        }
                    }
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;
            refresh_tray(app.handle());
            register_hotkeys(app.handle(), &cfg);
            osd::init(app.handle());
            let _ = app.state::<AppState>().watcher.set(watch_foreground(app.handle().clone()));
            let handle = app.handle().clone();
            engine::watch_devices(move || devices_changed(&handle));
            // Al arrancar se reescribe la curva: por si se tocó config.txt a mano o se reinstaló el motor
            let _ = apply(&cfg, &live);
            Ok(())
        })
        .on_window_event(|w, ev| {
            if let WindowEvent::CloseRequested { api, .. } = ev {
                if w.app_handle().state::<AppState>().cfg().close_to_tray {
                    api.prevent_close();
                    let _ = w.hide();
                    reset_live(w.app_handle());
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
            presets_sync,
            apps_list,
            preset_export,
            analyzer_start,
            compare_set,
            studio_set,
            hearing_set,
            meter_reset,
            analyzer_stop,
            app_quit,
            hotkey_check,
            hotkeys_pause,
            hotkeys_failed,
            room_check,
            room_measure,
            room_reset,
            room_cancel,
            room_fit,
        ])
        .build(tauri::generate_context!())
        .expect("no se ha podido arrancar PoxiEQ")
        .run(|app, ev| {
            if let RunEvent::Exit = ev {
                reset_live(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_new_fields() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"analyzerMode":"3d","device":"../../x","bypassHotkey":true,"presetKeys":["b:bass",null,"z:mal","u:abc",null,null,null,null,null,"b:flat","b:music"],
                "appRules":[{"exe":"Spotify.exe","name":"Spotify","preset":"b:music","on":true}]}"#,
        )
        .unwrap();
        cfg.sanitize();
        assert_eq!(cfg.analyzer_mode, "spectrum");
        assert_eq!(cfg.device, None);
        assert_eq!(cfg.app_rules[0].exe, "spotify.exe");
        // Atajos del formato anterior: mismos presets en cada hueco, teclas nuevas (teclado numérico)
        let h = &cfg.hotkeys;
        assert_eq!(h.bypass.as_deref(), Some("control+alt+Numpad0"));
        assert_eq!(h.presets.len(), hotkeys::SLOTS);
        assert_eq!(h.presets[0].preset.as_deref(), Some("b:bass"));
        assert_eq!(h.presets[0].keys.as_deref(), Some("control+alt+Numpad1"));
        assert_eq!(h.presets[2].preset, None);
        assert_eq!(h.presets[3].preset.as_deref(), Some("u:abc"));
        // Al guardarse ya no lleva el formato anterior, y al volver a cargarse no se migra otra vez
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("bypassHotkey") && !json.contains("presetKeys"));
        let mut again: Config = serde_json::from_str(&json).unwrap();
        again.sanitize();
        assert_eq!(again.hotkeys, cfg.hotkeys);
        // Bypass sin atajo en el formato anterior: sigue sin atajo
        let mut off: Config = serde_json::from_str(r#"{"bypassHotkey":false,"presetKeys":[null]}"#).unwrap();
        off.sanitize();
        assert_eq!(off.hotkeys.bypass, None);
        // Una configuración de antes (sin los campos nuevos) carga con los valores por defecto
        let mut old: Config = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        old.sanitize();
        assert_eq!(old.hotkeys, hotkeys::Hotkeys::default());
        assert!(old.app_rules.is_empty());
        assert_eq!(old.analyzer_mode, "spectrum");
        assert_eq!(old.mic, Mic::default());
        assert!(old.follow_default && old.mic_device.is_none());
        let mut bad: Config = serde_json::from_str(r#"{"micDevice":"../x","followDefault":false}"#).unwrap();
        bad.sanitize();
        assert!(!bad.follow_default && bad.mic_device.is_none());
        // Ajustes de un dispositivo fuera de rango
        let id = "{00000000-0000-0000-0000-00000000000a}";
        let mut m: Config = serde_json::from_str(&format!(r#"{{"mic":{{"bypass":true}},"perDevice":{{"{id}":{{"dsp":{{"gate":{{"on":true,"threshold":20,"range":500}}}}}}}}}}"#)).unwrap();
        m.sanitize();
        let gate = m.per_device[id].dsp.gate;
        assert!(m.mic.bypass && gate.on);
        assert_eq!((gate.threshold, gate.range), (0.0, 90.0));
        // «Graves»: sin el campo (configuración de antes) queda por defecto; fuera de rango se recorta
        let mut g: Config = serde_json::from_str(&format!(r#"{{"perDevice":{{"{id}":{{"dsp":{{"bass":{{"on":true}},"bassBoost":{{"on":true,"amount":7}}}}}}}}}}"#)).unwrap();
        g.sanitize();
        let d = g.per_device[id].dsp;
        assert!(d.bass.on && d.bass_boost.on && d.bass_boost.amount == 1.0);
        assert!(old.per_device.is_empty() && old.section_presets == SectionPresets::default());
    }

    /// Configuración del formato anterior: una dinámica para todas las salidas y otra para todos los micros
    const OLD: &str = r#"{"device":"{00000000-0000-0000-0000-00000000000a}",
        "curves":{"{00000000-0000-0000-0000-00000000000a}":{"preamp":-3,"bands":[]}},
        "dsp":{"leveler":{"on":true,"target":-23},"crossfeed":{"on":true,"feed":6},"bassBoost":{"on":true,"amount":0.4}},
        "mic":{"bypass":false,"dsp":{"gate":{"on":true,"threshold":-45},"comp":{"on":true,"ratio":4}}}}"#;

    #[test]
    fn migracion_global_a_por_dispositivo() {
        let (a, b, mic, new) = (
            "{00000000-0000-0000-0000-00000000000a}",
            "{00000000-0000-0000-0000-00000000000b}",
            "{00000000-0000-0000-0000-0000000000c0}",
            "{00000000-0000-0000-0000-00000000000f}",
        );
        let used = [(a.to_string(), false), (b.to_string(), false), (mic.to_string(), true)];
        let (cfg, _) = startup(serde_json::from_str(OLD).unwrap(), &used, None, |id| id == mic);
        // Cada salida usada se queda con lo de antes, sin perder nada
        for id in [a, b] {
            let d = &cfg.per_device[id].dsp;
            assert!(d.leveler.on && d.leveler.target == -23.0 && d.crossfeed.on && d.crossfeed.feed == 6.0 && d.bass_boost.on && d.bass_boost.amount == 0.4);
            assert!(!d.gate.on, "la puerta del micrófono no pasa a las salidas");
        }
        // El micrófono, con su puerta y su compresor
        let m = &cfg.per_device[mic].dsp;
        assert!(m.gate.on && m.gate.threshold == -45.0 && m.comp.on && m.comp.ratio == 4.0 && !m.leveler.on);
        // Uno nuevo empieza con lo de serie (Natural, sin crossfeed)
        assert!(!cfg.per_device.contains_key(new));
        // Se guarda sin el formato anterior y al volver a cargarse no se migra otra vez ni se pierde nada
        let json = serde_json::to_string(&cfg).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(back.get("dsp").is_none() && back["mic"].get("dsp").is_none());
        let again: Config = serde_json::from_str(&json).unwrap();
        assert!(again.dsp.is_none() && again.mic.dsp.is_none());
        let (again, _) = startup(again, &[(new.to_string(), false)], None, |_| false);
        assert_eq!(again.per_device, cfg.per_device);
        // Lo que ya tenía su propia configuración no se pisa
        let mut keep: Config = serde_json::from_str(OLD).unwrap();
        keep.per_device.insert(a.into(), DeviceSettings::default());
        keep.migrate_per_device(&used);
        assert_eq!(keep.per_device[a], DeviceSettings::default());
        assert!(keep.per_device[b].dsp.leveler.on);
    }

    #[test]
    fn al_arrancar_cada_dispositivo_lo_suyo_y_estudio_limpio() {
        let (a, b) = ("{00000000-0000-0000-0000-00000000000a}", "{00000000-0000-0000-0000-00000000000b}");
        let mut cfg: Config = serde_json::from_str(OLD).unwrap();
        // Hay presets de Estudio guardados y B tiene otra configuración (solo crossfeed)
        cfg.section_presets.studio.push(eq::SectionPreset {
            id: "u:mono".into(),
            name: "Mono".into(),
            dsp: None,
            curve: None,
            studio: Some(eq::Studio { listen: eq::Listen::Mono, sim: Some(eq::Sim::Phone), ..eq::Studio::default() }),
        });
        let mut phones = poxidsp::Settings::default();
        phones.crossfeed.on = true;
        cfg.per_device.insert(b.into(), DeviceSettings { dsp: phones, ..DeviceSettings::default() });
        // Windows tiene puesta B: se arranca en ella, cada una con lo suyo y Estudio sin nada
        let (cfg, live) = startup(cfg, &[(a.to_string(), false), (b.to_string(), false)], Some(b.into()), |_| false);
        assert_eq!(cfg.device.as_deref(), Some(b));
        assert_eq!(live, Live::default());
        assert_eq!(cfg.section_presets.studio.len(), 1, "los presets de Estudio se conservan");
        let text = eq::render(&cfg.curves, &cfg.per_device, cfg.bypass, &cfg.loudness, cfg.mic.bypass, &live, |_| false);
        assert!(!text.contains("Copy:") && !text.contains("estudio"), "{text}");
        let chunk = |id: &str| poxidsp::base64(&cfg.per_device[id].dsp.chunk(poxidsp::device_key(id)));
        assert!(text.contains(&chunk(a)) && text.contains(&chunk(b)));
        assert_ne!(cfg.per_device[a].dsp, cfg.per_device[b].dsp);
    }

    #[test]
    fn follow_default_output() {
        let a = "{00000000-0000-0000-0000-00000000000a}";
        let b = "{00000000-0000-0000-0000-00000000000b}";
        let cfg = Config { device: Some(a.into()), ..Config::default() };
        // Windows pasa de A a B: se edita B
        assert_eq!(follow_target(&cfg, Some(a), Some(b)).as_deref(), Some(b));
        // Sin cambio de predeterminada (otro aviso, p. ej. un micro enchufado): nada, aunque se edite otra a mano
        let manual = Config { device: Some(b.into()), ..Config::default() };
        assert_eq!(follow_target(&manual, Some(a), Some(a)), None);
        // Elegida otra a mano y Windows cambia: vuelve a mandar Windows
        assert_eq!(follow_target(&manual, Some(b), Some(a)).as_deref(), Some(a));
        // Ya se edita la nueva, sin salida predeterminada u opción apagada: nada
        assert_eq!(follow_target(&manual, Some(a), Some(b)), None);
        assert_eq!(follow_target(&cfg, Some(a), None), None);
        let off = Config { follow_default: false, ..cfg.clone() };
        assert_eq!(follow_target(&off, Some(a), Some(b)), None);
    }

    #[test]
    fn share_and_file_names() {
        assert!(share_code_ok("PXEQ1.eyJuIjoiYSJ9"));
        assert!(!share_code_ok("PXEQ1."));
        assert!(!share_code_ok("PXEQ1.abc def"));
        assert!(share_code_ok("PXEQ2.eyJzIjoic291bmQifQ"));
        assert!(!share_code_ok("PXEQ2."));
        assert!(!share_code_ok("PXEQ3.abc"));
        assert!(!share_code_ok(&format!("PXEQ1.{}", "a".repeat(9000))));
        assert_eq!(file_stem("Mis DT 770: noche/peli?"), "Mis DT 770 nochepeli");
        assert_eq!(file_stem(r"..\..\"), "preset");
        assert_eq!(file_stem("con"), "preset");
        assert_eq!(file_stem("COM1"), "preset");
        assert_eq!(file_stem("Comedia"), "Comedia");
        assert_eq!(file_stem("  "), "preset");
    }
}
