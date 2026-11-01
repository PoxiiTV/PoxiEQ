//! Volumen por aplicación en la salida predeterminada: atenuación cuando habla una app de voz y mezclador
//! juego ↔ chat. Más la salida de cada app (Spotify → cascos).
//!
//! Solo se toca el volumen de cada sesión (el del mezclador de Windows) y nunca por encima del que tenía. El
//! original se guarda en memoria y en disco: se devuelve al apagar la función, al salir y, si PoxiEQ murió a
//! mitad, en cuanto vuelve a ver esa app.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{Interface, GUID, HSTRING, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Threading::*;

/// Apps de voz de serie (nombre del ejecutable en minúsculas)
pub const VOICE_APPS: [&str; 10] =
    ["discord.exe", "teamspeak.exe", "ts3client_win64.exe", "ms-teams.exe", "teams.exe", "zoom.exe", "skype.exe", "mumble.exe", "whatsapp.exe", "whatsapp.root.exe"];

/// Juego y voz (se guarda en la configuración)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Voice {
    /// Atenuación: mientras habla una app de voz, las demás bajan `duck_db`
    pub duck: bool,
    pub duck_db: f64,
    /// Apps de voz
    pub apps: Vec<String>,
    /// Mezclador: −1 = todo juego (las de voz bajan), 0 = igual, 1 = todo chat (las demás bajan)
    pub mixer: f64,
}

impl Default for Voice {
    fn default() -> Self {
        Self { duck: false, duck_db: 8.0, apps: VOICE_APPS.iter().map(|s| s.to_string()).collect(), mixer: 0.0 }
    }
}

impl Voice {
    pub fn sanitize(&mut self) {
        self.duck_db = if self.duck_db.is_finite() { self.duck_db.clamp(3.0, 30.0).round() } else { 8.0 };
        self.mixer = if self.mixer.is_finite() { (self.mixer.clamp(-1.0, 1.0) * 20.0).round() / 20.0 } else { 0.0 };
        for a in &mut self.apps {
            *a = a.trim().to_lowercase();
        }
        let mut seen = std::collections::HashSet::new();
        self.apps.retain(|a| crate::apps::valid_exe(a) && seen.insert(a.clone()));
        self.apps.truncate(50);
    }

    fn active(&self) -> bool {
        self.duck || self.mixer != 0.0
    }
}

/// dB que baja el mezclador en un extremo
const MIX_RANGE_DB: f64 = 30.0;
/// Pico de la app de voz a partir del cual «está hablando» (−30 dBFS)
const TALKING: f32 = 0.032;
const ATTACK: f64 = 0.05;
const HOLD: Duration = Duration::from_millis(300);
const RELEASE: f64 = 0.6;
const TICK: Duration = Duration::from_millis(33);

/// Atenuación en dB de cada sesión según el mezclador y la atenuación (`duck`, ya con su envolvente)
fn attenuation(voice: bool, mixer: f64, duck: f64) -> f64 {
    if voice {
        (-mixer).max(0.0) * MIX_RANGE_DB
    } else {
        mixer.max(0.0) * MIX_RANGE_DB + duck
    }
}

/// Envolvente de la atenuación: baja en 50 ms, aguanta 300 ms tras la última voz y vuelve en 600 ms
fn envelope(cur: f64, target: f64, talking_recently: bool, dt: f64) -> f64 {
    if talking_recently {
        (cur + target * dt / ATTACK).min(target)
    } else {
        (cur - target.max(1.0) * dt / RELEASE).max(0.0)
    }
}

// ------------------------------------------------------------ el hilo

struct Shared {
    voice: Mutex<Voice>,
    file: PathBuf,
}

static SHARED: OnceLock<(Arc<Shared>, std::thread::Thread)> = OnceLock::new();
static EXIT: AtomicBool = AtomicBool::new(false);
static RESTORED: AtomicBool = AtomicBool::new(false);

/// Arranca el hilo (dormido mientras no haya nada que hacer). `file`: donde se guardan los volúmenes originales.
pub fn start(voice: Voice, file: PathBuf) {
    let shared = Arc::new(Shared { voice: Mutex::new(voice), file });
    let s = shared.clone();
    let t = std::thread::spawn(move || run(&s));
    let _ = SHARED.set((shared, t.thread().clone()));
}

/// La configuración ha cambiado
pub fn update(voice: &Voice) {
    if let Some((s, t)) = SHARED.get() {
        *s.voice.lock().unwrap_or_else(|e| e.into_inner()) = voice.clone();
        t.unpark();
    }
}

/// Al salir: devuelve todos los volúmenes (espera como mucho medio segundo)
pub fn shutdown() {
    let Some((_, t)) = SHARED.get() else { return };
    EXIT.store(true, Ordering::SeqCst);
    t.unpark();
    let until = Instant::now() + Duration::from_millis(500);
    while !RESTORED.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Session {
    exe: String,
    volume: ISimpleAudioVolume,
    meter: Option<IAudioMeterInformation>,
    /// Volumen que tenía antes de tocarlo
    original: Option<f32>,
    /// Lo último que se le puso (para notar si la persona lo cambia en el mezclador de Windows)
    last: Option<f32>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    /// exe → volumen original
    apps: HashMap<String, f32>,
}

/// Para que Windows sepa que el cambio de volumen es nuestro
const CONTEXT: GUID = GUID::from_u128(0x7a1c9e52_4f0b_4d8e_9a77_50c1e0e7b0d3);

fn run(shared: &Shared) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut sessions: HashMap<String, Session> = HashMap::new();
    // Lo que quedó a medias si PoxiEQ murió: se devuelve en cuanto aparece cada app
    let mut pending: HashMap<String, f32> = std::fs::read_to_string(&shared.file).ok().and_then(|t| serde_json::from_str::<Saved>(&t).ok()).map(|s| s.apps).unwrap_or_default();
    let mut duck = 0.0f64;
    let mut last_voice = Instant::now() - HOLD * 10;
    let mut refreshed = Instant::now() - Duration::from_secs(10);
    let mut last_tick = Instant::now();
    let mut saved_keys: Vec<String> = vec![];
    loop {
        let exit = EXIT.load(Ordering::SeqCst);
        let voice = shared.voice.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let active = voice.active() && !exit;
        if refreshed.elapsed() >= Duration::from_secs(1) || (!active && !pending.is_empty()) {
            refresh(&mut sessions, &mut pending);
            refreshed = Instant::now();
        }
        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64().min(0.2);
        last_tick = now;

        if active {
            let talking = voice.duck && sessions.values().any(|s| voice.apps.contains(&s.exe) && s.meter.as_ref().and_then(|m| unsafe { m.GetPeakValue() }.ok()).is_some_and(|p| p > TALKING));
            if talking {
                last_voice = now;
            }
            duck = if voice.duck { envelope(duck, voice.duck_db, now.duration_since(last_voice) < HOLD, dt) } else { 0.0 };
        } else {
            duck = 0.0;
        }
        for s in sessions.values_mut() {
            let att = if active { attenuation(voice.apps.contains(&s.exe), voice.mixer, duck) } else { 0.0 };
            apply(s, att);
        }
        save(shared, &sessions, &pending, &mut saved_keys);

        if exit {
            RESTORED.store(true, Ordering::SeqCst);
            return;
        }
        if active {
            std::thread::park_timeout(TICK);
        } else {
            // Todo devuelto: se olvidan las sesiones y se duerme hasta que cambie la configuración
            sessions.clear();
            std::thread::park();
        }
    }
}

/// Pone la sesión a `att` dB por debajo de su original (0 = la deja como estaba y la suelta)
fn apply(s: &mut Session, att: f64) {
    let Ok(current) = (unsafe { s.volume.GetMasterVolume() }) else { return };
    if let (Some(last), Some(orig)) = (s.last, s.original) {
        if (current - last).abs() > 0.004 {
            // La persona lo ha movido en Windows: eso pasa a ser su volumen
            let factor = 10f64.powf(-attenuation_of(last, orig) / 20.0) as f32;
            s.original = Some((current / factor.max(1e-4)).min(1.0));
        }
    }
    if att <= 0.01 {
        if let Some(orig) = s.original.take() {
            unsafe {
                let _ = s.volume.SetMasterVolume(orig, &CONTEXT);
            }
        }
        s.last = None;
        return;
    }
    let orig = *s.original.get_or_insert(current);
    let want = (orig as f64 * 10f64.powf(-att / 20.0)) as f32;
    if s.last.is_none_or(|l| (l - want).abs() > 0.0005) {
        unsafe {
            let _ = s.volume.SetMasterVolume(want.min(orig), &CONTEXT);
        }
        s.last = Some(want.min(orig));
    }
}

/// dB que separan lo puesto (`set`) del original
fn attenuation_of(set: f32, orig: f32) -> f64 {
    if orig <= 0.0 || set <= 0.0 {
        0.0
    } else {
        -20.0 * (set as f64 / orig as f64).log10()
    }
}

/// Guarda en disco los originales de lo que está tocado (solo si cambia qué está tocado)
fn save(shared: &Shared, sessions: &HashMap<String, Session>, pending: &HashMap<String, f32>, saved: &mut Vec<String>) {
    let mut apps: HashMap<String, f32> = pending.clone();
    for s in sessions.values() {
        if let Some(o) = s.original {
            apps.insert(s.exe.clone(), o);
        }
    }
    let mut keys: Vec<String> = apps.keys().cloned().collect();
    keys.sort();
    if keys == *saved {
        return;
    }
    *saved = keys;
    if apps.is_empty() {
        let _ = std::fs::remove_file(&shared.file);
    } else if let Ok(json) = serde_json::to_string(&Saved { apps }) {
        let _ = std::fs::write(&shared.file, json);
    }
}

/// Sesiones de la salida predeterminada ahora mismo. Las que desaparecen se devuelven a su volumen antes de soltarlas.
fn refresh(sessions: &mut HashMap<String, Session>, pending: &mut HashMap<String, f32>) {
    let found = unsafe { list_sessions() }.unwrap_or_default();
    let ids: Vec<String> = found.iter().map(|(id, _)| id.clone()).collect();
    for (id, s) in sessions.iter_mut() {
        if !ids.contains(id) {
            apply(s, 0.0);
        }
    }
    sessions.retain(|id, _| ids.contains(id));
    for (id, mut s) in found {
        if sessions.contains_key(&id) {
            continue;
        }
        // Una app que se quedó bajada porque PoxiEQ murió a mitad
        if let Some(orig) = pending.remove(&s.exe) {
            if unsafe { s.volume.GetMasterVolume() }.is_ok_and(|v| v < orig - 0.004) {
                unsafe {
                    let _ = s.volume.SetMasterVolume(orig, &CONTEXT);
                }
            }
            s.original = None;
        }
        sessions.insert(id, s);
    }
}

unsafe fn list_sessions() -> windows::core::Result<Vec<(String, Session)>> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None)?;
    let list = mgr.GetSessionEnumerator()?;
    let mut out = vec![];
    for i in 0..list.GetCount()? {
        let Ok(ctl) = list.GetSession(i) else { continue };
        let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else { continue };
        if c2.IsSystemSoundsSession() == S_OK {
            continue;
        }
        let Ok(pid) = c2.GetProcessId() else { continue };
        let Some(exe) = exe_of(pid) else { continue };
        let Ok(id) = c2.GetSessionInstanceIdentifier() else { continue };
        let key = id.to_string().unwrap_or_default();
        CoTaskMemFree(Some(id.0 as _));
        let Ok(volume) = ctl.cast::<ISimpleAudioVolume>() else { continue };
        out.push((key, Session { exe, volume, meter: ctl.cast::<IAudioMeterInformation>().ok(), original: None, last: None }));
    }
    Ok(out)
}

/// Nombre del ejecutable de un proceso, en minúsculas
pub fn exe_of(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(p);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_lowercase())
    }
}

// ------------------------------------------------------------ salida por app

/// Procesos abiertos de cada ejecutable (en minúsculas)
pub fn pids_of(exes: &[String]) -> Vec<(u32, String)> {
    let mut out = vec![];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
            if exes.contains(&exe) {
                out.push((e.th32ProcessID, exe));
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Salida de una app (`device` = GUID del endpoint; `None` = la predeterminada de Windows). Usa la interfaz sin
/// documentar que usa Windows en «Volumen de la aplicación y preferencias de dispositivo» (como EarTrumpet).
pub fn set_app_output(pid: u32, device: Option<&str>) -> bool {
    // Cambió de IID en Windows 10 21H2: se prueban las dos
    const IIDS: [GUID; 2] = [GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324), GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258)];
    type SetPersisted = unsafe extern "system" fn(*mut std::ffi::c_void, u32, EDataFlow, ERole, *mut std::ffi::c_void) -> windows::core::HRESULT;
    unsafe {
        let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        let Ok(factory) = windows::Win32::System::WinRT::RoGetActivationFactory::<windows::core::IUnknown>(&class) else { return false };
        let id = device.map(|g| HSTRING::from(format!(r"\\?\SWD#MMDEVAPI#{{0.0.0.00000000}}.{g}#{{e6327cad-dcec-4949-ae8a-991e976a79d2}}")));
        let raw_id: *mut std::ffi::c_void = id.as_ref().map_or(std::ptr::null_mut(), |h| std::mem::transmute_copy(h));
        for iid in IIDS {
            let mut ptr = std::ptr::null_mut();
            if factory.query(&iid, &mut ptr).is_err() || ptr.is_null() {
                continue;
            }
            // IUnknown (3) + IInspectable (3) + 19 métodos que no se usan: SetPersistedDefaultAudioEndpoint es el 25
            let vtbl = *(ptr as *const *const usize);
            let set: SetPersisted = std::mem::transmute(*vtbl.add(25));
            let ok = [eConsole, eMultimedia].iter().all(|&role| set(ptr, pid, eRender, role, raw_id).is_ok());
            windows::core::IUnknown::from_raw(ptr);
            return ok;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mezclador_y_atenuacion() {
        // Centro: nada; con la voz hablando, solo bajan las demás
        assert_eq!(attenuation(true, 0.0, 8.0), 0.0);
        assert_eq!(attenuation(false, 0.0, 8.0), 8.0);
        // Hacia «juego» baja la voz; hacia «chat», lo demás
        assert_eq!(attenuation(true, -0.5, 0.0), 15.0);
        assert_eq!(attenuation(false, -0.5, 0.0), 0.0);
        assert_eq!(attenuation(false, 1.0, 0.0), 30.0);
        // Envolvente: baja en 50 ms, vuelve en 600 ms
        let mut d = 0.0;
        for _ in 0..3 {
            d = envelope(d, 8.0, true, 0.02);
        }
        assert_eq!(d, 8.0);
        for _ in 0..10 {
            d = envelope(d, 8.0, false, 0.033);
        }
        assert!(d > 0.0 && d < 8.0);
        for _ in 0..20 {
            d = envelope(d, 8.0, false, 0.033);
        }
        assert_eq!(d, 0.0);
        assert!((attenuation_of(0.5, 1.0) - 6.0206).abs() < 1e-3);
    }

    #[test]
    fn ajustes_de_voz_validados() {
        let mut v = Voice { duck_db: 99.0, mixer: 0.333, apps: vec![" Discord.EXE ".into(), "discord.exe".into(), "../x.exe".into()], ..Voice::default() };
        v.sanitize();
        assert_eq!((v.duck_db, v.mixer), (30.0, 0.35));
        assert_eq!(v.apps, ["discord.exe"]);
    }
}

