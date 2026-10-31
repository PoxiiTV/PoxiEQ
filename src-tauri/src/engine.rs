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
const COMBINED_DEVICE: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},41";

pub type Result<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn hklm() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}

// ------------------------------------------------------------------ estado

#[derive(Serialize, Debug)]
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
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub config_dir: Option<String>,
    pub devices: Vec<Device>,
}

pub fn status() -> Status {
    let app = hklm().open_subkey(APP).ok();
    let installed = install_dir().is_some_and(|d| d.join("EqualizerAPO.dll").exists());
    Status {
        installed,
        config_dir: app.and_then(|k| k.get_value("ConfigPath").ok()),
        devices: devices(),
    }
}

fn devices() -> Vec<Device> {
    let Ok(render) = hklm().open_subkey(RENDER) else { return vec![] };
    let default = default_device();
    let mut list: Vec<Device> = render
        .enum_keys()
        .flatten()
        .filter_map(|id| {
            let key = render.open_subkey(&id).ok()?;
            let state: u32 = key.get_value("DeviceState").ok()?;
            // Solo activos o desenchufados: los «no presentes» y deshabilitados no se tocan
            // Los deshabilitados llevan 0x10000000 en el registro, no DEVICE_STATE_DISABLED
            let unplugged = state & DEVICE_STATE_UNPLUGGED.0 != 0;
            if state & (DEVICE_STATE_DISABLED.0 | 0x1000_0000) != 0 || (state & DEVICE_STATE_ACTIVE.0 == 0 && !unplugged) {
                return None;
            }
            let props = key.open_subkey("Properties").ok()?;
            Some(Device {
                default: default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(&id)),
                enabled: is_enabled(&id),
                name: props.get_value(CONNECTION_NAME).unwrap_or_default(),
                device: props.get_value(DEVICE_NAME).unwrap_or_default(),
                unplugged,
                id,
            })
        })
        .collect();
    list.sort_by(|a, b| b.default.cmp(&a.default).then(a.unplugged.cmp(&b.unplugged)).then(a.name.cmp(&b.name)));
    list
}

fn is_enabled(id: &str) -> bool {
    let Ok(fx) = hklm().open_subkey(format!(r"{RENDER}\{id}\FxProperties")) else { return false };
    FX.iter().any(|name| {
        fx.get_value::<String, _>(name)
            .is_ok_and(|v| v.eq_ignore_ascii_case(PRE_MIX) || v.eq_ignore_ascii_case(POST_MIX))
    })
}

fn default_device() -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = dev.GetId().ok()?;
        let s = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as _));
        // «{0.0.0.00000000}.{guid}» -> «{guid}»
        s.and_then(|s| s.rsplit_once("}.").map(|(_, g)| g.to_string()))
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
pub fn enable(id: &str) -> Result<()> {
    if is_enabled(id) {
        return Ok(());
    }
    let dev_path = format!(r"{RENDER}\{id}");
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
    child.set_value("PostMixChild", &real(post)).map_err(err("Child APOs"))?;
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
            set(FX[G], POST_MIX)?;
            for i in [S, M, E] {
                let _ = fx.delete_value(FX[i]);
            }
        }
        Mode::SfxMfx | Mode::SfxEfx => {
            let _ = fx.delete_value(FX[L]);
            let _ = fx.delete_value(FX[G]);
            set(FX[S], PRE_MIX)?;
            mode_list(SFX_MODES)?;
            if mode == Mode::SfxMfx {
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
    let fx_path = format!(r"{RENDER}\{id}\FxProperties");

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
    restart_audio()?;
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
    Command::new(exe)
        .args(args)
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
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            for id in ids {
                let _ = open_stream(&en, id);
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    while results.iter().any(|r| !(r.pre_mix && r.post_mix)) {
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
    Ok(results)
}

unsafe fn open_stream(en: &IMMDeviceEnumerator, id: &str) -> windows::core::Result<()> {
    let dev = en.GetDevice(&HSTRING::from(format!("{{0.0.0.00000000}}.{id}")))?;
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

// --------------------------------------------------------------- configuración

const INCLUDE: &str = "Include: poxieq.txt\r\n";

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
    std::fs::write(dir.join("poxieq.txt"), text).map_err(err("poxieq.txt"))
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

/// Todo lo que necesita admin, de una vez: instalar (si hace falta), activar/quitar,
/// reiniciar el audio y comprobar.
pub fn setup(installer: Option<&str>, enable_ids: &[String], disable_ids: &[String]) -> SetupReport {
    let run = || -> Result<Vec<Check>> {
        if let Some(path) = installer {
            if !status().installed {
                install(path)?;
            }
        }
        hide_engine();
        for id in disable_ids {
            disable(id)?;
        }
        for id in enable_ids {
            enable(id)?;
        }
        if enable_ids.is_empty() && disable_ids.is_empty() {
            return Ok(vec![]);
        }
        restart_audio()?;
        check(enable_ids)
    };
    match run() {
        Ok(checks) => SetupReport { error: None, checks },
        Err(e) => SetupReport { error: Some(e), checks: vec![] },
    }
}
