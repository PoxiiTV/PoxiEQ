//! Motor de audio: Equalizer APO.
//!
//! Hace lo mismo que su `DeviceSelector` (registrar el APO en el `FxProperties` del
//! dispositivo y guardar los APO originales en `Child APOs` para poder restaurarlos),
//! pero sin ventana y reiniciando solo el servicio de audio en lugar del PC.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use winreg::enums::*;
use winreg::RegKey;

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Pipes::*;
use windows::Win32::System::Registry::KEY_ALL_ACCESS;
use windows::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const RENDER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";
const CAPTURE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
const APP: &str = r"SOFTWARE\EqualizerAPO";
const CHILD: &str = r"SOFTWARE\EqualizerAPO\Child APOs";

const PRE_MIX: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
const POST_MIX: &str = "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}";
const NO_KEY: &str = "!KEY";
const NO_VALUE: &str = "!VALUE";

// Huecos de efectos del endpoint (PKEY_FX_*): LFX, GFX, SFX, MFX, EFX
const FX: [&str; 5] = [
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},5",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},6",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},7",
];
const L: usize = 0;
const G: usize = 1;
const S: usize = 2;
const M: usize = 3;
const E: usize = 4;
const MULTI_FX: [&str; 3] = [
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},14",
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},15",
];
const SFX_MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},5";
const MFX_MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},6";
const EFX_MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},7";
const DEFAULT_MODE: &str = "{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}";
const DISABLE_ENHANCEMENTS: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
const FX_TITLE: &str = "{b725f130-47ef-101a-a5f1-02608c9eebac},10";
const CONNECTION_NAME: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
const DEVICE_NAME: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";
/// Ruta del nodo de dispositivo: «{1}.USB\VID_…», «{1}.BTHENUM\…»
const DEVICE_PATH: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},2";
const COMBINED_DEVICE: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},41";
/// Aparato físico al que pertenece el endpoint (PKEY_Device_ContainerId)
const CONTAINER_ID: &str = "{8c7ed206-3f8a-4827-b3ab-ae9e1faefc6c},2";

pub type Result<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn hklm() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}

// ------------------------------------------------------------------ estado

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// GUID del endpoint, tal cual aparece en el registro: `{xxxxxxxx-...}`
    pub id: String,
    /// «Altavoces», «Auriculares»...
    pub name: String,
    /// Nombre del hardware: «Audient iD4», «Realtek(R) Audio»...
    pub device: String,
    pub default: bool,
    pub enabled: bool,
    pub unplugged: bool,
    /// Entrada (micrófono): en el motor solo tiene etapa de captura
    pub input: bool,
    /// PoxiEQ lo activó, pero ya no pasa por el motor
    pub broken: Option<Broken>,
    /// Cascos Bluetooth: la otra mitad (del estéreo, su manos libres; del manos libres, su estéreo)
    pub pair: Option<String>,
    /// Manos libres de unos cascos Bluetooth (mono, para llamadas): va con su estéreo, no se ajusta aparte
    pub hands_free: bool,
}

/// Por qué un dispositivo que PoxiEQ activó ya no pasa por el motor. Se arregla con el mismo paso elevado
/// que activa (`setup`).
#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Broken {
    /// Nuestros APO ya no están en `FxProperties` (lo reescribió una actualización de Windows o del driver) o
    /// falta la DLL del motor
    Lost,
    /// «Mejoras de audio» desactivadas en el dispositivo: Windows no carga ningún APO
    Enhancements,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub config_dir: Option<String>,
    pub devices: Vec<Device>,
    /// Lo rellena el comando `engine_status` (leer la DLL no hace falta en cada cambio de la curva)
    pub dsp: DspState,
}

pub fn status() -> Status {
    let app = hklm().open_subkey(APP).ok();
    let installed = install_dir().is_some_and(|d| d.join("EqualizerAPO.dll").exists());
    Status {
        installed,
        config_dir: app.and_then(|k| k.get_value("ConfigPath").ok()),
        devices: with_pairs([devices(false, installed), devices(true, installed)].concat()),
        dsp: DspState::Missing,
    }
}

/// Salidas (`input` = false) o entradas conectadas, la predeterminada primero. `installed`: está la DLL del
/// motor (sin ella nada de lo activado suena).
fn devices(input: bool, installed: bool) -> Vec<Device> {
    let Ok(root) = hklm().open_subkey(if input { CAPTURE } else { RENDER }) else { return vec![] };
    let default = default_device(input);
    let mut list: Vec<Device> = root
        .enum_keys()
        .flatten()
        .filter_map(|id| {
            let key = root.open_subkey(&id).ok()?;
            let state: u32 = key.get_value("DeviceState").ok()?;
            // Solo activos o desenchufados: los «no presentes» y deshabilitados no se tocan
            // Los deshabilitados llevan 0x10000000 en el registro, no DEVICE_STATE_DISABLED
            let unplugged = state & DEVICE_STATE_UNPLUGGED.0 != 0;
            if state & (DEVICE_STATE_DISABLED.0 | 0x1000_0000) != 0 || (state & DEVICE_STATE_ACTIVE.0 == 0 && !unplugged) {
                return None;
            }
            let props = key.open_subkey("Properties").ok()?;
            let enabled = is_enabled(&id);
            Some(Device {
                default: default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(&id)),
                broken: broken(&id, enabled, installed),
                pair: None,
                hands_free: false,
                enabled,
                name: props.get_value(CONNECTION_NAME).unwrap_or_default(),
                device: props.get_value(DEVICE_NAME).unwrap_or_default(),
                unplugged,
                input,
                id,
            })
        })
        .collect();
    list.sort_by(|a, b| b.default.cmp(&a.default).then(a.unplugged.cmp(&b.unplugged)).then(a.name.cmp(&b.name)));
    list
}

/// Junta el estéreo y el manos libres de cada casco Bluetooth. Se activan y se reparan juntos, así que si el
/// manos libres se rompe, se ve en el estéreo (que es el que sale en la interfaz).
fn with_pairs(mut list: Vec<Device>) -> Vec<Device> {
    for (hf, stereo) in bt_pairs() {
        let hf_broken = list.iter().find(|d| d.id == hf).and_then(|d| d.broken);
        for d in list.iter_mut() {
            if d.id == hf {
                d.pair = Some(stereo.clone());
                d.hands_free = true;
            } else if d.id == stereo {
                d.pair = Some(hf.clone());
                d.broken = d.broken.or(hf_broken);
            }
        }
    }
    list
}

/// Cascos Bluetooth: el estéreo (A2DP, nodo en BTHENUM) y el manos libres (BTHHFENUM) son dos salidas del mismo
/// aparato (mismo ContainerId). Devuelve `(manos libres, estéreo)`.
pub fn bt_pairs() -> Vec<(String, String)> {
    let Ok(root) = hklm().open_subkey(RENDER) else { return vec![] };
    let (mut stereo, mut hf) = (vec![], vec![]);
    for id in root.enum_keys().flatten() {
        let Ok(props) = root.open_subkey(format!(r"{id}\Properties")) else { continue };
        let (Ok(path), Ok(container)) = (props.get_value::<String, _>(DEVICE_PATH), props.get_raw_value(CONTAINER_ID)) else { continue };
        let path = path.to_ascii_uppercase();
        if path.contains(".BTHENUM") {
            stereo.push((id, container.bytes));
        } else if path.contains(".BTHHFENUM") {
            hf.push((id, container.bytes));
        }
    }
    pair_up(&hf, &stereo)
}

fn pair_up(hf: &[(String, Vec<u8>)], stereo: &[(String, Vec<u8>)]) -> Vec<(String, String)> {
    hf.iter().filter_map(|(h, c)| stereo.iter().find(|(_, s)| s == c).map(|(s, _)| (h.clone(), s.clone()))).collect()
}

/// Añade a la lista la otra mitad de cada casco Bluetooth (activar o quitar uno es activar o quitar los dos)
pub fn with_bt_halves(ids: &[String]) -> Vec<String> {
    let pairs = bt_pairs();
    let mut out = ids.to_vec();
    for id in ids {
        for (hf, stereo) in &pairs {
            let other = if id.eq_ignore_ascii_case(stereo) { hf } else if id.eq_ignore_ascii_case(hf) { stereo } else { continue };
            if !out.iter().any(|o| o.eq_ignore_ascii_case(other)) {
                out.push(other.clone());
            }
        }
    }
    out
}

/// Clave del dispositivo en el registro: en `Render` si es una salida y, si no, en `Capture`
/// (lo mismo que hace `DeviceAPOInfo::load`)
fn dev_path(id: &str) -> String {
    let render = format!(r"{RENDER}\{id}");
    if hklm().open_subkey(&render).is_ok() {
        render
    } else {
        format!(r"{CAPTURE}\{id}")
    }
}

/// La salida es Bluetooth: su nodo de dispositivo cuelga del bus BTHENUM (A2DP), BTHHFENUM (manos libres)
/// o BTHLE (LE Audio)
pub fn is_bluetooth(id: &str) -> bool {
    hklm()
        .open_subkey(format!(r"{RENDER}\{id}\Properties"))
        .and_then(|p| p.get_value::<String, _>(DEVICE_PATH))
        .is_ok_and(|v| v.to_ascii_uppercase().contains(".BTH"))
}

/// El GUID es de una entrada (micrófono)
pub fn is_input(id: &str) -> bool {
    is_device_id(id) && hklm().open_subkey(format!(r"{RENDER}\{id}")).is_err() && hklm().open_subkey(format!(r"{CAPTURE}\{id}")).is_ok()
}

fn is_enabled(id: &str) -> bool {
    let Ok(fx) = hklm().open_subkey(format!(r"{}\FxProperties", dev_path(id))) else { return false };
    FX.iter().any(|name| {
        fx.get_value::<String, _>(name)
            .is_ok_and(|v| v.eq_ignore_ascii_case(PRE_MIX) || v.eq_ignore_ascii_case(POST_MIX))
    })
}

/// Lo activó PoxiEQ (o el `DeviceSelector` del motor): guarda sus APO originales en `Child APOs`, y eso no lo
/// toca ni Windows ni el driver
fn activated(id: &str) -> bool {
    hklm().open_subkey(format!(r"{CHILD}\{id}")).is_ok()
}

/// «Mejoras de audio» desactivadas (en la configuración de sonido de Windows o por el driver)
fn enhancements_off(id: &str) -> bool {
    hklm()
        .open_subkey(format!(r"{}\FxProperties", dev_path(id)))
        .and_then(|fx| fx.get_value::<u32, _>(DISABLE_ENHANCEMENTS))
        .is_ok_and(|v| v != 0)
}

fn broken(id: &str, enabled: bool, installed: bool) -> Option<Broken> {
    activated(id).then(|| classify(enabled && installed, || enhancements_off(id))).flatten()
}

/// Un dispositivo activado: `hooked` = sigue enganchado al motor (y este está instalado)
fn classify(hooked: bool, enhancements_off: impl FnOnce() -> bool) -> Option<Broken> {
    if !hooked {
        Some(Broken::Lost)
    } else if enhancements_off() {
        Some(Broken::Enhancements)
    } else {
        None
    }
}

/// `{guid}` de un dispositivo, sin nada más (es lo que acaba en rutas del registro y en el archivo del motor)
pub fn is_device_id(id: &str) -> bool {
    id.len() == 38 && id.starts_with('{') && id.ends_with('}') && id[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// La salida que se está editando: la elegida si está conectada y, si no, la predeterminada
/// (lo mismo que `currentDevice` en la interfaz)
pub fn active_device(preferred: Option<&str>) -> Option<String> {
    let plugged = |id: &str| {
        hklm()
            .open_subkey(format!(r"{RENDER}\{id}"))
            .and_then(|k| k.get_value::<u32, _>("DeviceState"))
            .is_ok_and(|s| s & DEVICE_STATE_ACTIVE.0 != 0 && s & (DEVICE_STATE_DISABLED.0 | 0x1000_0000) == 0)
    };
    preferred.filter(|id| is_device_id(id) && plugged(id)).map(String::from).or_else(|| default_device(false))
}

/// Salida (`input` = false) o entrada predeterminada de Windows (rol consola)
pub fn default_device(input: bool) -> Option<String> {
    // En un hilo propio: COM en modo multihilo en el hilo de la ventana impide crearla (la interfaz necesita
    // ese hilo en modo apartamento para arrastrar y soltar), y esto también se llama al arrancar
    std::thread::spawn(move || default_device_mta(input)).join().ok().flatten()
}

fn default_device_mta(input: bool) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(if input { eCapture } else { eRender }, eConsole).ok()?;
        let id = dev.GetId().ok()?;
        let s = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as _));
        // «{0.0.0.00000000}.{guid}» -> «{guid}»
        s.and_then(|s| s.rsplit_once("}.").map(|(_, g)| g.to_string()))
    }
}

/// Llama a `changed` cuando Windows cambia la salida o la entrada predeterminada o se conecta, desconecta o
/// activa un dispositivo. Sin sondeo: Windows avisa por `IMMNotificationClient` y este hilo, dormido el resto
/// del tiempo, agrupa la ráfaga de avisos de un mismo cambio (llegan varios: uno por rol, otro por estado…).
/// En el aviso no se hace nada más que mandarlo al hilo: Windows pide no bloquear ahí.
pub fn watch_devices(changed: impl Fn() + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let (tx, rx) = mpsc::channel();
        let client: IMMNotificationClient = Notify(tx).into();
        if en.RegisterEndpointNotificationCallback(&client).is_err() {
            return;
        }
        // `en` y `client` viven lo que el hilo (toda la vida de la app): el registro no se quita
        while rx.recv().is_ok() {
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            changed();
        }
    });
}

#[windows::core::implement(IMMNotificationClient)]
struct Notify(mpsc::Sender<()>);

impl IMMNotificationClient_Impl for Notify_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, _: EDataFlow, role: ERole, _: &PCWSTR) -> windows::core::Result<()> {
        // Cada cambio llega una vez por rol; con el de consola basta (es el que lee `default_device`)
        if role == eConsole {
            let _ = self.0.send(());
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

// --------------------------------------------------------- activar / quitar

#[derive(PartialEq)]
enum Mode {
    LfxGfx,
    SfxMfx,
    SfxEfx,
}

/// Registra Equalizer APO en el dispositivo. Requiere administrador.
///
/// En una entrada (micrófono) solo hay etapa de captura: el APO de pre-mix va en LFX o SFX, no se toca
/// GFX/MFX/EFX y no hay APO original de post-mix (como `DeviceAPOInfo::install` con `input`).
pub fn enable(id: &str) -> Result<()> {
    let dev_path = dev_path(id);
    if is_enabled(id) {
        // Ya activado: solo puede faltar que vuelvan las «mejoras de audio» (reparar)
        if let Ok(fx) = hklm().open_subkey_with_flags(format!(r"{dev_path}\FxProperties"), KEY_READ | KEY_SET_VALUE) {
            let _ = fx.delete_value(DISABLE_ENHANCEMENTS);
        }
        return Ok(());
    }
    let input = dev_path.starts_with(CAPTURE);
    let fx_path = format!(r"{dev_path}\FxProperties");
    let (child, _) = hklm().create_subkey(format!(r"{CHILD}\{id}")).map_err(err("Child APOs"))?;

    let mut orig: [String; 5] = Default::default();
    // Solo KEY_SET_VALUE: Windows no da más a los administradores sobre estas claves
    let fx = if hklm().open_subkey(&fx_path).is_ok() {
        let fx = hklm().open_subkey_with_flags(&fx_path, KEY_READ | KEY_SET_VALUE).map_err(err("FxProperties"))?;
        for (i, name) in FX.iter().enumerate() {
            orig[i] = fx.get_value(name).unwrap_or_else(|_| NO_VALUE.into());
        }
        fx
    } else {
        let create = || hklm().create_subkey_with_flags(&fx_path, KEY_READ | KEY_SET_VALUE).map(|(k, _)| k);
        let fx = match create() {
            Ok(k) => k,
            Err(_) => {
                // Sin permisos sobre la clave del dispositivo: tomarla como hace DeviceSelector
                take_ownership(&dev_path)?;
                create().map_err(err("FxProperties"))?
            }
        };
        fx.set_value(FX_TITLE, &"PoxiEQ").map_err(err("FxProperties"))?;
        orig.iter_mut().for_each(|o| *o = NO_KEY.into());
        fx
    };
    for (i, name) in FX.iter().enumerate() {
        child.set_value(name, &orig[i]).map_err(err("Child APOs"))?;
    }

    let has = |name: &str| fx.get_raw_value(name).is_ok();
    let mode = if (has(FX[L]) || has(FX[G])) && ![FX[S], FX[M], FX[E]].iter().chain(MULTI_FX.iter()).any(|n| has(n)) {
        Mode::LfxGfx
    } else if hklm()
        .open_subkey(format!(r"{dev_path}\Properties"))
        .is_ok_and(|p| p.get_raw_value(COMBINED_DEVICE).is_ok())
    {
        // Bluetooth combinado de Windows 11: EFX no funciona
        Mode::SfxMfx
    } else {
        Mode::SfxEfx
    };

    let none = |a: usize, b: usize| orig[a] == NO_VALUE && orig[b] == NO_VALUE;
    let (pre, post) = match mode {
        Mode::LfxGfx if none(L, G) => (&orig[S], &orig[M]),
        Mode::LfxGfx => (&orig[L], &orig[G]),
        Mode::SfxMfx if none(S, M) => (&orig[L], &orig[G]),
        Mode::SfxMfx => (&orig[S], &orig[M]),
        Mode::SfxEfx if none(S, E) => (&orig[L], &orig[G]),
        Mode::SfxEfx => (&orig[S], &orig[E]),
    };
    let real = |g: &String| if g == NO_KEY || g == NO_VALUE { String::new() } else { g.clone() };
    child.set_value("PreMixChild", &real(pre)).map_err(err("Child APOs"))?;
    child.set_value("PostMixChild", &if input { String::new() } else { real(post) }).map_err(err("Child APOs"))?;
    child.set_value("AllowSilentBufferModification", &"false").map_err(err("Child APOs"))?;
    let _ = child.delete_value("DisableAutomaticAdjustment");
    child.set_value("Version", &"2").map_err(err("Child APOs"))?;

    let set = |name: &str, v: &str| fx.set_value(name, &v).map_err(err("FxProperties"));
    let mode_list = |name: &str| -> Result<()> {
        if !has(name) {
            fx.set_value(name, &vec![DEFAULT_MODE.to_string()]).map_err(err("FxProperties"))?;
        }
        Ok(())
    };
    match mode {
        Mode::LfxGfx => {
            set(FX[L], PRE_MIX)?;
            if !input {
                set(FX[G], POST_MIX)?;
            }
            for i in [S, M, E] {
                let _ = fx.delete_value(FX[i]);
            }
        }
        Mode::SfxMfx | Mode::SfxEfx => {
            let _ = fx.delete_value(FX[L]);
            let _ = fx.delete_value(FX[G]);
            set(FX[S], PRE_MIX)?;
            mode_list(SFX_MODES)?;
            if input {
                // Entrada: MFX y EFX se quedan como estaban
            } else if mode == Mode::SfxMfx {
                set(FX[M], POST_MIX)?;
                mode_list(MFX_MODES)?;
            } else {
                set(FX[E], POST_MIX)?;
                mode_list(EFX_MODES)?;
            }
        }
    }
    // Las «mejoras de audio» desactivadas en Windows anulan el APO
    let _ = fx.delete_value(DISABLE_ENHANCEMENTS);
    Ok(())
}

/// Quita Equalizer APO del dispositivo y deja los APO originales. Requiere administrador.
pub fn disable(id: &str) -> Result<()> {
    let child_path = format!(r"{CHILD}\{id}");
    let Ok(child) = hklm().open_subkey(&child_path) else { return Ok(()) };
    let orig: Vec<String> = FX.iter().map(|n| child.get_value(n).unwrap_or_default()).collect();
    let fx_path = format!(r"{}\FxProperties", dev_path(id));

    if orig[0] == NO_KEY {
        let _ = hklm().delete_subkey(&fx_path);
    } else if let Ok(fx) = hklm().open_subkey_with_flags(&fx_path, KEY_READ | KEY_SET_VALUE) {
        for (name, value) in FX.iter().zip(&orig) {
            if value == NO_VALUE {
                let _ = fx.delete_value(name);
            } else if !value.is_empty() {
                fx.set_value(name, value).map_err(err("FxProperties"))?;
            }
        }
    }
    drop(child);
    hklm().delete_subkey_all(&child_path).map_err(err("Child APOs"))?;
    if let Ok(children) = hklm().open_subkey(CHILD) {
        if children.enum_keys().next().is_none() && children.enum_values().next().is_none() {
            drop(children);
            let _ = hklm().delete_subkey(CHILD);
        }
    }
    Ok(())
}

fn take_ownership(key_path: &str) -> Result<()> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).map_err(err("token"))?;
        let mut luid = LUID::default();
        LookupPrivilegeValueW(None, SE_TAKE_OWNERSHIP_NAME, &mut luid).map_err(err("privilegio"))?;
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let adjusted = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        let _ = CloseHandle(token);
        adjusted.map_err(err("privilegio"))?;

        let mut sid_buf = [0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut size = sid_buf.len() as u32;
        let admins = PSID(sid_buf.as_mut_ptr() as _);
        CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(admins), &mut size).map_err(err("SID"))?;

        let name = HSTRING::from(format!(r"MACHINE\{key_path}"));
        SetNamedSecurityInfoW(&name, SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION, Some(admins), None, None, None)
            .ok()
            .map_err(err("propietario"))?;

        let mut old_acl: *mut ACL = std::ptr::null_mut();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        GetNamedSecurityInfoW(&name, SE_REGISTRY_KEY, DACL_SECURITY_INFORMATION, None, None, Some(&mut old_acl), None, &mut sd)
            .ok()
            .map_err(err("permisos"))?;
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: KEY_ALL_ACCESS.0,
            grfAccessMode: SET_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_GROUP,
                ptstrName: PWSTR(admins.0 as _),
                ..Default::default()
            },
        };
        let mut new_acl: *mut ACL = std::ptr::null_mut();
        let set = SetEntriesInAclW(Some(&[access]), Some(old_acl), &mut new_acl);
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        set.ok().map_err(err("permisos"))?;
        let applied = SetNamedSecurityInfoW(&name, SE_REGISTRY_KEY, DACL_SECURITY_INFORMATION, None, None, Some(new_acl), None);
        let _ = LocalFree(Some(HLOCAL(new_acl as _)));
        applied.ok().map_err(err("permisos"))
    }
}

// ----------------------------------------------------- instalar el motor

/// Instala Equalizer APO en silencio. Su instalador abre siempre el DeviceSelector al
/// final (incluso con /S); lo cerramos porque los dispositivos los activamos nosotros.
pub fn install(installer: &str) -> Result<()> {
    let mut setup = Command::new(installer).arg("/S").spawn().map_err(err("instalador"))?;
    loop {
        if let Some(code) = setup.try_wait().map_err(err("instalador"))? {
            // 0 = bien; con el DeviceSelector cerrado a la fuerza pide reinicio y no hace falta
            return if status().installed { Ok(()) } else { Err(format!("el instalador terminó con {code}")) };
        }
        hidden("taskkill", &["/F", "/IM", "DeviceSelector.exe"]);
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// El motor trabaja por debajo: fuera sus accesos del menú Inicio y su aviso de actualizaciones
/// (las versiones las decide PoxiEQ). Se repite en cada activación por si se instaló antes.
fn hide_engine() {
    let menus = [std::env::var("ProgramData").map(|d| PathBuf::from(d).join(r"Microsoft\Windows\Start Menu\Programs")), std::env::var("APPDATA").map(|d| PathBuf::from(d).join(r"Microsoft\Windows\Start Menu\Programs"))];
    for dir in menus.into_iter().flatten() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().starts_with("Equalizer APO") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    if let Some(dir) = install_dir() {
        let checker = dir.join("UpdateChecker.exe");
        if checker.exists() {
            let _ = hidden(&checker.to_string_lossy(), &["-u"]);
        }
    }
    // Fuera de «Aplicaciones instaladas»: se desinstala desde PoxiEQ (Ajustes o al desinstalar PoxiEQ)
    if let Ok((k, _)) = hklm().create_subkey(UNINSTALL) {
        let _ = k.set_value("SystemComponent", &1u32);
    }
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\EqualizerAPO";

// ------------------------------------------------------------------ PoxiDSP

/// `PoxiDSP.dll`, compilado por `build.rs` y metido en el exe (así llega también con el portable)
pub const DSP_DLL: &[u8] = include_bytes!(env!("POXIDSP_DLL"));

#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DspState {
    Missing,
    /// Instalado, pero no es el de esta versión de PoxiEQ
    Old,
    Current,
}

/// Dentro de la carpeta del motor (Archivos de programa): solo un administrador puede cambiarlo, y lo
/// carga audiodg.exe
fn dsp_path() -> Option<PathBuf> {
    install_dir().map(|d| d.join("VSTPlugins").join(poxidsp::DLL_NAME))
}

pub fn dsp_state() -> DspState {
    match dsp_path().and_then(|p| std::fs::read(p).ok()) {
        None => DspState::Missing,
        Some(b) if b == DSP_DLL => DspState::Current,
        Some(_) => DspState::Old,
    }
}

/// Borra las copias viejas que dejó una actualización (las que sigan cargadas no se pueden: otra vez será)
fn clean_old_dsp() {
    let Some(dir) = dsp_path().and_then(|p| p.parent().map(PathBuf::from)) else { return };
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().ends_with(".old") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Instala (o actualiza) PoxiDSP.dll. Requiere administrador. Devuelve `true` si la versión anterior
/// estaba cargada y hay que reiniciar el audio para que entre la nueva.
pub fn install_dsp() -> Result<bool> {
    let path = dsp_path().ok_or("el motor no está instalado")?;
    if std::fs::read(&path).is_ok_and(|b| b == DSP_DLL) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err("VSTPlugins"))?;
    }
    clean_old_dsp();
    let mut loaded = false;
    if path.exists() {
        // Una DLL cargada no se puede sobrescribir, pero sí renombrar: sigue sonando hasta reiniciar el audio
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let old = path.with_extension(format!("{stamp}.old"));
        std::fs::rename(&path, &old).map_err(err(poxidsp::DLL_NAME))?;
        loaded = std::fs::remove_file(&old).is_err();
    }
    std::fs::write(&path, DSP_DLL).map_err(err(poxidsp::DLL_NAME))?;
    Ok(loaded)
}

fn install_dir() -> Option<PathBuf> {
    hklm().open_subkey(APP).ok()?.get_value::<String, _>("InstallPath").ok().map(PathBuf::from)
}

/// Quita el motor por completo: restaura los efectos originales de cada salida, reinicia el audio
/// (para soltar la DLL) y lanza su desinstalador en silencio. Requiere administrador.
pub fn uninstall() -> Result<()> {
    let Some(dir) = install_dir() else { return Ok(()) };
    let ids: Vec<String> = hklm().open_subkey(CHILD).map(|k| k.enum_keys().flatten().collect()).unwrap_or_default();
    for id in &ids {
        disable(id)?;
    }
    task_delete();
    restart_audio()?;
    // Su desinstalador no sabe nada de nuestro plugin
    if let Some(p) = dsp_path() {
        let _ = std::fs::remove_file(p);
        clean_old_dsp();
    }
    hidden(&dir.join("Uninstall.exe").to_string_lossy(), &["/S"]);
    // El desinstalador de NSIS se copia a TEMP y vuelve al momento: se espera a que borre su registro
    let deadline = Instant::now() + Duration::from_secs(60);
    while hklm().open_subkey(APP).is_ok_and(|k| k.get_raw_value("InstallPath").is_ok()) {
        if Instant::now() > deadline {
            return Err("el desinstalador del motor no ha terminado".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Ok(())
}

fn hidden(exe: &str, args: &[&str]) -> std::process::Output {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(exe);
    // Los programas del motor (Qt) buscan sus complementos en la carpeta de trabajo: se lanzan desde la suya
    if let Some(dir) = std::path::Path::new(exe).parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
        .unwrap_or_else(|_| std::process::Output { status: Default::default(), stdout: vec![], stderr: vec![] })
}

/// Reinicia el servicio de audio de Windows para que cargue el APO. Corte de ~2 s.
pub fn restart_audio() -> Result<()> {
    let out = hidden(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", "Restart-Service -Name Audiosrv -Force -ErrorAction Stop"],
    );
    // La salida de PowerShell viene en la página de códigos de la consola: no se enseña, solo el hecho
    if out.status.success() {
        Ok(())
    } else {
        Err("no se pudo reiniciar el servicio de audio de Windows".into())
    }
}

// -------------------------------------------------------------- comprobar

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: String,
    pub pre_mix: bool,
    pub post_mix: bool,
}

/// Abre cada dispositivo y espera a que el APO avise por la tubería de prueba que
/// Equalizer APO trae para esto (`DeviceTestPipeName`). Requiere administrador.
pub fn check(ids: &[String]) -> Result<Vec<Check>> {
    let pipe = format!("PoxiEQTest{}", std::process::id());
    let (tx, rx) = mpsc::channel::<String>();
    let name = HSTRING::from(format!(r"\\.\pipe\{pipe}"));
    // Límite: el hilo se queda bloqueado esperando clientes; vale porque este proceso elevado termina enseguida
    std::thread::spawn(move || unsafe { listen(&name, tx) });

    let (app, _) = hklm().create_subkey(APP).map_err(err("EqualizerAPO"))?;
    app.set_value("DeviceTestPipeName", &pipe).map_err(err("EqualizerAPO"))?;

    let mut results: Vec<Check> = ids.iter().map(|id| Check { id: id.clone(), ..Default::default() }).collect();
    // Las entradas solo tienen etapa de captura (el APO avisa como «PreMix»)
    let inputs: Vec<bool> = ids.iter().map(|id| is_input(id)).collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            for (id, &input) in ids.iter().zip(&inputs) {
                let _ = open_stream(&en, id, input);
            }
        }
    }

    let done = |r: &Check, input: bool| r.pre_mix && (r.post_mix || input);
    let deadline = Instant::now() + Duration::from_secs(3);
    while results.iter().zip(&inputs).any(|(r, &input)| !done(r, input)) {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else { break };
        let Ok(msg) = rx.recv_timeout(left) else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) else { continue };
        if v["phase"] != "Initialize" {
            continue;
        }
        let guid = v["deviceGuid"].as_str().unwrap_or_default();
        if let Some(r) = results.iter_mut().find(|r| r.id.eq_ignore_ascii_case(guid)) {
            match v["stage"].as_str() {
                Some("PreMix") => r.pre_mix = true,
                Some("PostMix") => r.post_mix = true,
                _ => {}
            }
        }
    }
    let _ = app.delete_value("DeviceTestPipeName");
    for (r, &input) in results.iter_mut().zip(&inputs) {
        if input {
            // En captura no hay post-mix: cuenta solo la etapa de captura
            r.post_mix = r.pre_mix;
        }
    }
    Ok(results)
}

/// Identificador completo del endpoint para `IMMDeviceEnumerator::GetDevice`
pub fn endpoint_id(id: &str, input: bool) -> HSTRING {
    HSTRING::from(format!("{{0.0.{}.00000000}}.{id}", if input { 1 } else { 0 }))
}

unsafe fn open_stream(en: &IMMDeviceEnumerator, id: &str, input: bool) -> windows::core::Result<()> {
    let dev = en.GetDevice(&endpoint_id(id, input))?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let format = client.GetMixFormat()?;
    let res = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, format, None);
    CoTaskMemFree(Some(format as _));
    res
}

unsafe fn listen(name: &HSTRING, tx: mpsc::Sender<String>) {
    // DACL nula: el APO corre dentro de audiodg.exe con otra cuenta
    let mut sd = SECURITY_DESCRIPTOR::default();
    let psd = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as _);
    if InitializeSecurityDescriptor(psd, SECURITY_DESCRIPTOR_REVISION).is_err()
        || SetSecurityDescriptorDacl(psd, true, None, false).is_err()
    {
        return;
    }
    let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: psd.0, bInheritHandle: false.into() };
    loop {
        let pipe = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_INBOUND,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            0,
            1024,
            0,
            Some(&sa),
        );
        if pipe.is_invalid() {
            return;
        }
        let connected = ConnectNamedPipe(pipe, None).is_ok() || GetLastError() == ERROR_PIPE_CONNECTED;
        if connected {
            let mut buf = [0u8; 1024];
            let mut read = 0u32;
            if ReadFile(pipe, Some(&mut buf), Some(&mut read), None).is_ok() {
                let _ = tx.send(String::from_utf8_lossy(&buf[..read as usize]).into_owned());
            }
        }
        let _ = CloseHandle(pipe);
    }
}

// -------------------------------------------------------------- diagnóstico

/// «¿Por qué no suena?»: lo que se ve sin admin ni sonido. El resto (activado, roto, bypass, salida
/// predeterminada, PoxiDSP viejo) ya lo sabe la interfaz.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// Audio espacial de Windows (Windows Sonic, Dolby Atmos, DTS) activo: se salta el post-mix
    pub spatial: bool,
    /// Una app la usa en modo exclusivo: lo que suena por ella no pasa por el motor
    pub exclusive: bool,
    /// Silenciada en Windows
    pub muted: bool,
    /// Efectos del fabricante que siguen en la cadena (los APO originales que guardamos al activar)
    pub vendor: Vec<String>,
}

pub fn diagnose(id: &str) -> Diagnosis {
    let input = is_input(id);
    let mut d = Diagnosis { vendor: vendor_effects(id), ..Default::default() };
    let id = id.to_string();
    // COM multihilo en un hilo propio (ver `default_device`)
    let (spatial, exclusive, muted) = std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return (false, false, false) };
        let Ok(dev) = en.GetDevice(&endpoint_id(&id, input)) else { return (false, false, false) };
        // Sin audio espacial la salida no da objetos dinámicos (o ni deja crear el cliente)
        let spatial = !input && dev.Activate::<ISpatialAudioClient>(CLSCTX_ALL, None).and_then(|c| c.GetMaxDynamicObjectCount()).is_ok_and(|n| n > 0);
        // Solo se prepara el flujo (sin arrancarlo): no suena nada
        let exclusive = open_stream(&en, &id, input).is_err_and(|e| e.code() == AUDCLNT_E_DEVICE_IN_USE);
        let muted = dev
            .Activate::<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume>(CLSCTX_ALL, None)
            .and_then(|v| v.GetMute())
            .is_ok_and(|m| m.as_bool());
        (spatial, exclusive, muted)
    })
    .join()
    .unwrap_or_default();
    (d.spatial, d.exclusive, d.muted) = (spatial, exclusive, muted);
    d
}

/// Nombres de los efectos originales que el motor sigue llamando (`PreMixChild`/`PostMixChild`)
fn vendor_effects(id: &str) -> Vec<String> {
    let Ok(child) = hklm().open_subkey(format!(r"{CHILD}\{id}")) else { return vec![] };
    let mut names: Vec<String> = ["PreMixChild", "PostMixChild"]
        .iter()
        .filter_map(|v| child.get_value::<String, _>(v).ok())
        .filter(|clsid| !clsid.is_empty())
        .map(|clsid| apo_name(&clsid))
        .collect();
    names.dedup();
    names
}

fn apo_name(clsid: &str) -> String {
    hklm()
        .open_subkey(format!(r"SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\{clsid}"))
        .and_then(|k| k.get_value::<String, _>("FriendlyName"))
        .or_else(|_| hklm().open_subkey(format!(r"SOFTWARE\Classes\CLSID\{clsid}")).and_then(|k| k.get_value::<String, _>("")))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| clsid.to_string())
}

/// Configuración de sonido de Windows (para quitar el audio espacial)
pub fn open_sound_settings() {
    let _ = Command::new("explorer").arg("ms-settings:sound").spawn();
}

// --------------------------------------------------------------- configuración

const INCLUDE: &str = "Include: poxieq.txt\r\n";

/// `config.txt` sigue apuntando a nuestro archivo y este existe (otro programa o una reinstalación del motor
/// pueden haberlos cambiado)
pub fn config_intact() -> bool {
    status().config_dir.map(PathBuf::from).is_none_or(|dir| {
        std::fs::read_to_string(dir.join("config.txt")).ok().as_deref() == Some(INCLUDE) && dir.join("poxieq.txt").exists()
    })
}

/// Escribe nuestro archivo de filtros. Equalizer APO lo relee al vuelo: sin admin ni reinicios.
pub fn write_config(text: &str) -> Result<()> {
    let dir = status().config_dir.ok_or("el motor no está instalado")?;
    let dir = PathBuf::from(dir);
    let main = dir.join("config.txt");
    if std::fs::read_to_string(&main).ok().as_deref() != Some(INCLUDE) {
        // Lo que hubiera se guarda una vez y config.txt pasa a apuntar solo a PoxiEQ
        let backup = dir.join("config.antes-de-poxieq.txt");
        if main.exists() && !backup.exists() {
            std::fs::copy(&main, &backup).map_err(err("config.txt"))?;
        }
        std::fs::write(&main, INCLUDE).map_err(err("config.txt"))?;
    }
    // Si no cambia nada no se escribe: el motor relee el archivo en cuanto se toca y PoxiDSP se recarga
    // (p. ej. al elegir otra salida, que no cambia lo que suena)
    let file = dir.join("poxieq.txt");
    if std::fs::read_to_string(&file).ok().as_deref() == Some(text) {
        return Ok(());
    }
    std::fs::write(file, text).map_err(err("poxieq.txt"))
}

// ------------------------------------------------- activar solos los dispositivos nuevos

/// Tarea programada que activa los dispositivos nuevos sin pedir permiso cada vez
pub const TASK: &str = r"\PoxiEQ\Activar";
/// Tope de lo que se lee del archivo de ids y de dispositivos por vez
const MAX_IDS_FILE: u64 = 4096;
const MAX_AUTO: usize = 8;

/// Copia de este exe que lanza la tarea: en la carpeta del motor, dentro de Archivos de programa, donde solo un
/// administrador puede escribir. La tarea corre elevada sin preguntar: si apuntara al exe de PoxiEQ (en la
/// carpeta del usuario), cualquier programa del usuario podría cambiarlo y saltarse el permiso.
fn activator() -> Option<PathBuf> {
    let dir = install_dir()?;
    let pf = std::env::var("ProgramFiles").ok()?;
    dir.to_string_lossy().to_lowercase().starts_with(&pf.to_lowercase()).then(|| dir.join("PoxiEQ-activar.exe"))
}

/// Archivo de ids válido: ruta absoluta que acaba en `auto-enable.txt` y sin comillas (va dentro de la orden)
fn valid_ids_file(p: &str) -> bool {
    p.len() < 400 && p.as_bytes().get(1..3) == Some(b":\\") && p.to_lowercase().ends_with(r"\auto-enable.txt") && !p.contains(['"', '\r', '\n'])
}

/// Crea la tarea (requiere admin). `ids_file`: donde la app apunta los dispositivos a activar.
pub fn task_create(ids_file: &str) -> Result<()> {
    if !valid_ids_file(ids_file) {
        return Err("archivo no válido".into());
    }
    let dst = activator().ok_or("el motor no está en Archivos de programa")?;
    let exe = std::env::current_exe().map_err(err("exe"))?;
    std::fs::copy(&exe, &dst).map_err(err("activador"))?;
    let action = format!("\"{}\" --auto-enable \"{ids_file}\"", dst.display());
    // Sin disparador propio (fecha pasada): solo se lanza a mano, con `schtasks /Run`
    let out = hidden("schtasks", &["/Create", "/F", "/TN", TASK, "/TR", &action, "/SC", "ONCE", "/ST", "00:00", "/SD", "01/01/2000", "/RL", "HIGHEST"]);
    if out.status.success() {
        Ok(())
    } else {
        Err("no se pudo crear la tarea".into())
    }
}

/// Quita la tarea y su copia del exe (requiere admin)
pub fn task_delete() {
    hidden("schtasks", &["/Delete", "/F", "/TN", TASK]);
    if let Some(p) = activator() {
        let _ = std::fs::remove_file(p);
    }
}

/// Lanza la tarea (sin admin: la tarea ya lo tiene)
pub fn task_run() -> bool {
    hidden("schtasks", &["/Run", "/TN", TASK]).status.success()
}

/// Lo que hace la tarea: lee el archivo (que puede escribir el usuario) y solo acepta GUID de salidas o entradas
/// conectadas que aún no pasan por el motor. Nada más: ni rutas, ni órdenes, ni escribe ningún archivo.
pub fn auto_enable(ids_file: &str) {
    if !valid_ids_file(ids_file) || std::fs::metadata(ids_file).map_or(true, |m| m.len() > MAX_IDS_FILE) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(ids_file) else { return };
    let ids = pick_auto(&text, |id| {
        let key = hklm().open_subkey(dev_path(id)).ok();
        let active = key.and_then(|k| k.get_value::<u32, _>("DeviceState").ok()).is_some_and(|s| s & DEVICE_STATE_ACTIVE.0 != 0);
        active && !is_enabled(id)
    });
    if !ids.is_empty() {
        let _ = setup(None, &ids, &[], false);
    }
}

/// Ids válidos del archivo, sin repetir y como mucho `MAX_AUTO`
fn pick_auto(text: &str, ok: impl Fn(&str) -> bool) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for l in text.lines().map(str::trim) {
        if is_device_id(l) && !ids.iter().any(|i| i.eq_ignore_ascii_case(l)) && ok(l) {
            ids.push(l.to_string());
        }
        if ids.len() == MAX_AUTO {
            break;
        }
    }
    ids
}

// ----------------------------------------------------------------- elevación

/// Relanza este exe como administrador (un solo aviso de UAC), espera y devuelve lo que
/// escribió en el archivo de salida.
pub fn run_elevated(args: &[String]) -> Result<String> {
    let out = std::env::temp_dir().join(format!("poxieq-{}.json", std::process::id()));
    let exe = std::env::current_exe().map_err(err("exe"))?;
    let params = std::iter::once(format!("--elevated \"{}\"", out.display()))
        .chain(args.iter().map(|a| format!("\"{a}\"")))
        .collect::<Vec<_>>()
        .join(" ");
    let (verb, file, params) = (HSTRING::from("runas"), HSTRING::from(exe.as_os_str()), HSTRING::from(params));
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).map_err(|_| "Hace falta permiso de administrador".to_string())?;
        WaitForSingleObject(info.hProcess, INFINITE);
        let _ = CloseHandle(info.hProcess);
    }
    let text = std::fs::read_to_string(&out).map_err(err("resultado"))?;
    let _ = std::fs::remove_file(&out);
    Ok(text)
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub error: Option<String>,
    pub checks: Vec<Check>,
}

/// Todo lo que necesita admin, de una vez: instalar (si hace falta), instalar/actualizar PoxiDSP
/// (`dsp`), activar/quitar, reiniciar el audio y comprobar.
pub fn setup(installer: Option<&str>, enable_ids: &[String], disable_ids: &[String], dsp: bool) -> SetupReport {
    let run = || -> Result<Vec<Check>> {
        if let Some(path) = installer {
            if !status().installed {
                install(path)?;
            }
        }
        hide_engine();
        let reload = dsp && install_dsp()?;
        for id in disable_ids {
            disable(id)?;
        }
        for id in enable_ids {
            enable(id)?;
        }
        if enable_ids.is_empty() && disable_ids.is_empty() {
            if reload {
                restart_audio()?;
                clean_old_dsp();
            }
            return Ok(vec![]);
        }
        restart_audio()?;
        clean_old_dsp();
        check(enable_ids)
    };
    match run() {
        Ok(checks) => SetupReport { error: None, checks },
        Err(e) => SetupReport { error: Some(e), checks: vec![] },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use windows::core::s;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    type Dispatcher = extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
    type Process = extern "C" fn(*mut AEffect, *const *const f64, *mut *mut f64, i32);

    /// `AEffect` de VST 2.4 tal como lo lee el motor (`aeffectx.h`)
    #[repr(C)]
    struct AEffect {
        magic: i32,
        dispatcher: Dispatcher,
        process: usize,
        set_parameter: usize,
        get_parameter: usize,
        num_programs: i32,
        num_params: i32,
        num_inputs: i32,
        num_outputs: i32,
        flags: i32,
        reserved: [isize; 2],
        initial_delay: i32,
        qualities: [i32; 2],
        io_ratio: f32,
        object: usize,
        user: usize,
        unique_id: i32,
        version: i32,
        process_replacing: usize,
        process_double_replacing: Process,
        future: [u8; 56],
    }

    #[test]
    fn cascos_bluetooth_por_aparato() {
        let c = |b: u8| vec![b; 16];
        let hf = [("{h1}".to_string(), c(1)), ("{h2}".to_string(), c(9))];
        let st = [("{s2}".to_string(), c(2)), ("{s1}".to_string(), c(1))];
        assert_eq!(pair_up(&hf, &st), vec![("{h1}".to_string(), "{s1}".to_string())]);
    }

    #[test]
    fn la_tarea_solo_acepta_guid() {
        let a = "{6af377fe-6370-495b-afe3-95fac2e918f7}";
        let text = format!("{a}\r\n{a}\n  {{0d1b4c9a-1111-4a2b-9c3d-123456789abc}}  \ncmd /c calc\n..\\x\n{{zz}}\n");
        assert_eq!(pick_auto(&text, |_| true), [a, "{0d1b4c9a-1111-4a2b-9c3d-123456789abc}"]);
        assert!(pick_auto(&text, |_| false).is_empty());
        assert!(valid_ids_file(r"C:\Users\x\AppData\Local\com.poxi.poxieq\auto-enable.txt"));
        assert!(!valid_ids_file(r#"C:\x\auto-enable.txt" & calc & ""#));
        assert!(!valid_ids_file(r"auto-enable.txt") && !valid_ids_file(r"C:\x\otro.txt"));
    }

    #[test]
    fn rotura_de_un_dispositivo_activado() {
        assert_eq!(classify(false, || true), Some(Broken::Lost));
        assert_eq!(classify(false, || false), Some(Broken::Lost));
        assert_eq!(classify(true, || true), Some(Broken::Enhancements));
        assert_eq!(classify(true, || false), None);
    }

    /// La DLL que va dentro del exe, cargada de verdad y llamada en el mismo orden que el motor
    #[test]
    fn la_dll_empaquetada_responde_como_espera_el_motor() {
        let path = std::env::temp_dir().join(format!("PoxiDSP-test-{}.dll", std::process::id()));
        std::fs::write(&path, DSP_DLL).unwrap();
        let null = std::ptr::null_mut();
        unsafe {
            let lib = LoadLibraryW(&HSTRING::from(path.as_os_str())).expect("cargar la DLL");
            let main = GetProcAddress(lib, s!("VSTPluginMain")).expect("VSTPluginMain exportada");
            let main: extern "C" fn(*const c_void) -> *mut AEffect = std::mem::transmute(main);
            let fx = main(std::ptr::null());
            assert!(!fx.is_null());
            let e = &*fx;
            assert_eq!(e.magic, i32::from_be_bytes(*b"VstP"));
            assert_eq!((e.num_inputs, e.num_outputs, e.num_params, e.initial_delay), (2, 2, 0, 0));
            assert_eq!(e.flags & (1 << 4 | 1 << 5 | 1 << 12), 1 << 4 | 1 << 5 | 1 << 12);
            let d = e.dispatcher;
            d(fx, 0, 0, 0, null, 0.0);
            d(fx, 10, 0, 0, null, 48000.0);
            d(fx, 11, 0, 480, null, 0.0);
            let mut s = poxidsp::Settings::default();
            (s.limiter.on, s.comp.on, s.leveler.on, s.bass.on, s.dialog.on, s.crossfeed.on) = (true, true, true, true, true, true);
            let chunk = s.chunk(1);
            assert_eq!(d(fx, 24, 1, chunk.len() as isize, chunk.as_ptr() as *mut c_void, 0.0), 1);
            d(fx, 12, 0, 1, null, 0.0);
            d(fx, 71, 0, 0, null, 0.0);
            // Ruido a +12 dB en bloques de 480: nada por encima del techo de −1 dBTP
            let ceil = 10f64.powf(-1.0 / 20.0);
            for block in 0..100 {
                let mut l: Vec<f64> = (0..480).map(|i| (((block * 480 + i) * 7919 % 2003) as f64 / 1001.5 - 1.0) * 4.0).collect();
                let mut r: Vec<f64> = l.iter().rev().copied().collect();
                let inputs = [l.as_ptr(), r.as_ptr()];
                let mut outputs = [l.as_mut_ptr(), r.as_mut_ptr()];
                (e.process_double_replacing)(fx, inputs.as_ptr(), outputs.as_mut_ptr(), 480);
                assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= ceil));
            }
            d(fx, 72, 0, 0, null, 0.0);
            d(fx, 12, 0, 0, null, 0.0);
            assert_eq!(d(fx, 1, 0, 0, null, 0.0), 1);
            FreeLibrary(lib).unwrap();
        }
        let _ = std::fs::remove_file(path);
    }
}
