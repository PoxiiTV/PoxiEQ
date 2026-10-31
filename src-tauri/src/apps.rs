//! Preset automático por aplicación: qué app está en primer plano, qué apps hay abiertas
//! (con su nombre legible) y la lógica de reglas «esta app → este preset, y al salir, el de antes».

use crate::eq::Curve;
use serde::{Deserialize, Serialize};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const MAX_RULES: usize = 50;

/// Cuando `exe` está en primer plano suena `preset`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    /// Nombre del ejecutable en minúsculas (`spotify.exe`)
    pub exe: String,
    /// Nombre legible para la interfaz
    pub name: String,
    pub preset: String,
    pub on: bool,
}

/// Id de preset válido: `b:…` (de serie) o `u:…` (propio), corto y sin nada raro
pub fn valid_preset_id(id: &str) -> bool {
    (3..=64).contains(&id.len()) && (id.starts_with("b:") || id.starts_with("u:")) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-:".contains(&b))
}

/// Solo un nombre de archivo `.exe`: ni rutas, ni comodines, ni caracteres de control
fn valid_exe(exe: &str) -> bool {
    (5..=128).contains(&exe.len()) && exe.ends_with(".exe") && !exe.chars().any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
}

/// Lo que llega de la interfaz no se da por bueno: se descarta lo que no encaja y se acota el resto.
pub fn sanitize_rules(rules: &mut Vec<AppRule>) {
    let mut seen = std::collections::HashSet::new();
    rules.retain_mut(|r| {
        r.exe = r.exe.trim().to_lowercase();
        r.name = r.name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
        if r.name.is_empty() {
            r.name = r.exe.trim_end_matches(".exe").to_string();
        }
        valid_exe(&r.exe) && valid_preset_id(&r.preset) && seen.insert(r.exe.clone())
    });
    rules.truncate(MAX_RULES);
}

// ------------------------------------------------------------------ reglas

/// Qué hacer tras un cambio de app en primer plano
#[derive(Debug, PartialEq)]
pub enum Step {
    Keep,
    Apply(String),
    /// Devolver a `dispositivo` la curva que tenía antes de la primera regla
    Restore(String, Curve),
}

/// Estado del preset automático. Al entrar en una app con regla se guarda la curva de antes (una vez,
/// aunque se salte de una app con regla a otra) y al volver a una sin regla se restaura, salvo que se
/// haya tocado la curva entretanto: entonces manda lo que hizo la persona.
pub struct Auto {
    /// Nuestro propio exe: con PoxiEQ delante no se cambia nada (se está ajustando)
    own: String,
    last: String,
    applied: Option<String>,
    saved: Option<(String, Curve)>,
}

impl Auto {
    pub fn new(own: &str) -> Self {
        Self { own: own.to_lowercase(), last: String::new(), applied: None, saved: None }
    }

    /// `current`: dispositivo que se edita y su curva en este momento
    pub fn step(&mut self, exe: &str, rules: &[AppRule], current: Option<(&str, &Curve)>) -> Step {
        if exe == self.last {
            return Step::Keep;
        }
        self.last = exe.to_string();
        if exe == self.own {
            return Step::Keep;
        }
        let Some(rule) = rules.iter().find(|r| r.on && r.exe == exe) else { return self.release(current) };
        if self.applied.as_deref() == Some(rule.preset.as_str()) {
            return Step::Keep;
        }
        if self.saved.is_none() {
            self.saved = current.map(|(d, c)| (d.to_string(), c.clone()));
        }
        self.applied = Some(rule.preset.clone());
        Step::Apply(rule.preset.clone())
    }

    /// Sin regla delante (o reglas apagadas): vuelve la curva de antes si sigue puesto nuestro preset
    pub fn release(&mut self, current: Option<(&str, &Curve)>) -> Step {
        let (applied, saved) = (self.applied.take(), self.saved.take());
        match (applied, saved, current) {
            (Some(p), Some((dev, curve)), Some((cur_dev, cur))) if cur_dev == dev && cur.preset.as_deref() == Some(p.as_str()) => Step::Restore(dev, curve),
            _ => Step::Keep,
        }
    }

    /// Es la última app vista (no hace falta mirar nada más)
    pub fn seen(&self, exe: &str) -> bool {
        exe == self.last
    }

    pub fn active(&self) -> bool {
        self.applied.is_some()
    }

    /// Olvida la última app vista: la siguiente comprobación vuelve a decidir (p. ej. tras cambiar las reglas)
    pub fn rearm(&mut self) {
        self.last.clear();
    }
}

// ------------------------------------------------------------------ Windows

/// Ejecutable (en minúsculas) de la ventana en primer plano
pub fn foreground_exe() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        process_path(hwnd).map(|p| file_name(&p))
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase()
}

unsafe fn process_path(hwnd: HWND) -> Option<String> {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
    let _ = CloseHandle(process);
    ok.ok().map(|_| String::from_utf16_lossy(&buf[..len as usize]))
}

/// App abierta con ventana, para elegirla al crear una regla
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenApp {
    pub exe: String,
    pub name: String,
}

/// Procesos del sistema que tienen ventanas «visibles» pero no son apps que se usen
const HIDDEN: &[&str] = &["explorer.exe", "applicationframehost.exe", "textinputhost.exe", "shellexperiencehost.exe", "searchhost.exe", "startmenuexperiencehost.exe", "lockapp.exe"];

/// Apps con una ventana principal a la vista (como en Alt+Tab), sin repetir y ordenadas por nombre
pub fn list() -> Vec<OpenApp> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HWND>);
        list.push(hwnd);
        true.into()
    }
    let mut windows: Vec<HWND> = vec![];
    let own = std::env::current_exe().ok().map(|p| file_name(&p.to_string_lossy())).unwrap_or_default();
    let mut apps: Vec<OpenApp> = vec![];
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
        for hwnd in windows {
            if !main_window(hwnd) {
                continue;
            }
            let Some(path) = process_path(hwnd) else { continue };
            let exe = file_name(&path);
            if exe == own || HIDDEN.contains(&exe.as_str()) || !valid_exe(&exe) || apps.iter().any(|a| a.exe == exe) {
                continue;
            }
            let name = describe(&path).unwrap_or_else(|| {
                let mut stem = exe.trim_end_matches(".exe").chars();
                stem.next().map(|c| c.to_uppercase().chain(stem).collect()).unwrap_or_default()
            });
            apps.push(OpenApp { exe, name });
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// Ventana de nivel superior, visible, con título, sin dueño, que no es de herramientas ni está oculta por DWM
unsafe fn main_window(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 || GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
        return false;
    }
    if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
        return false;
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as _, size_of::<u32>() as u32);
    cloaked == 0
}

/// Descripción del ejecutable (`Spotify`, `Google Chrome`…) o, si no tiene, su nombre de producto
unsafe fn describe(path: &str) -> Option<String> {
    let file = HSTRING::from(path);
    let size = GetFileVersionInfoSizeW(&file, None);
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    GetFileVersionInfoW(&file, None, size, data.as_mut_ptr() as _).ok()?;
    let query = |sub: &str| -> Option<(*mut u16, u32)> {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let key = HSTRING::from(sub);
        VerQueryValueW(data.as_ptr() as _, PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool().then_some((ptr as *mut u16, len))
    };
    // Primer idioma/página de códigos de la tabla de traducciones
    let (t, len) = query(r"\VarFileInfo\Translation")?;
    if len < 4 {
        return None;
    }
    let (lang, cp) = (*t, *t.add(1));
    ["FileDescription", "ProductName"].iter().find_map(|field| {
        let (p, n) = query(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\{field}"))?;
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, n as usize));
        let text: String = text.trim_end_matches('\0').trim().chars().filter(|c| !c.is_control()).take(64).collect();
        (!text.is_empty()).then_some(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(exe: &str, preset: &str) -> AppRule {
        AppRule { exe: exe.into(), name: String::new(), preset: preset.into(), on: true }
    }

    fn curve(preset: Option<&str>) -> Curve {
        Curve { preamp: -1.0, auto_preamp: false, bands: vec![], preset: preset.map(String::from) }
    }

    #[test]
    fn sanitize() {
        let mut rules = vec![
            AppRule { exe: " Spotify.EXE ".into(), name: "Spo\ntify".into(), preset: "b:music".into(), on: true },
            rule("spotify.exe", "b:bass"),
            rule(r"C:\juegos\cs2.exe", "b:footsteps"),
            rule("notepad.txt", "b:flat"),
            rule("game.exe", "b:x\"; rm"),
            rule("game.exe", "z:raro"),
            rule("discord.exe", "u:lq3k9"),
        ];
        sanitize_rules(&mut rules);
        assert_eq!(rules.len(), 2, "{rules:?}");
        assert_eq!(rules[0], AppRule { exe: "spotify.exe".into(), name: "Spotify".into(), preset: "b:music".into(), on: true });
        assert_eq!(rules[1].name, "discord", "sin nombre se usa el del exe");
        let mut many: Vec<AppRule> = (0..80).map(|i| rule(&format!("app{i}.exe"), "b:flat")).collect();
        sanitize_rules(&mut many);
        assert_eq!(many.len(), MAX_RULES);
    }

    #[test]
    fn apply_and_restore() {
        let rules = vec![rule("spotify.exe", "b:music"), rule("cs2.exe", "b:footsteps"), AppRule { on: false, ..rule("vlc.exe", "b:cinema") }];
        let mine = curve(Some("u:mine"));
        let mut a = Auto::new("PoxiEQ.exe");
        let dev = "{dev}";
        assert_eq!(a.step("chrome.exe", &rules, Some((dev, &mine))), Step::Keep);
        assert_eq!(a.step("spotify.exe", &rules, Some((dev, &mine))), Step::Apply("b:music".into()));
        assert_eq!(a.step("spotify.exe", &rules, Some((dev, &curve(Some("b:music"))))), Step::Keep, "sin cambio de app no se hace nada");
        // De una app con regla a otra: la curva guardada sigue siendo la original
        assert_eq!(a.step("cs2.exe", &rules, Some((dev, &curve(Some("b:music"))))), Step::Apply("b:footsteps".into()));
        // PoxiEQ delante: se está ajustando, no se toca
        assert_eq!(a.step("poxieq.exe", &rules, Some((dev, &curve(Some("b:footsteps"))))), Step::Keep);
        assert_eq!(a.step("cs2.exe", &rules, Some((dev, &curve(Some("b:footsteps"))))), Step::Keep, "volver a la misma app no reaplica");
        // Regla apagada = como si no hubiera regla
        assert_eq!(a.step("vlc.exe", &rules, Some((dev, &curve(Some("b:footsteps"))))), Step::Restore(dev.into(), mine.clone()));
        assert!(!a.active());
    }

    #[test]
    fn manual_change_wins() {
        let rules = vec![rule("spotify.exe", "b:music")];
        let mut a = Auto::new("poxieq.exe");
        assert_eq!(a.step("spotify.exe", &rules, Some(("{a}", &curve(None)))), Step::Apply("b:music".into()));
        // La persona eligió otro preset con Spotify delante: al salir se queda el suyo
        assert_eq!(a.step("chrome.exe", &rules, Some(("{a}", &curve(Some("b:bass"))))), Step::Keep);
        // Y si cambió de salida, la curva guardada no se lleva a otro dispositivo
        assert_eq!(a.step("spotify.exe", &rules, Some(("{a}", &curve(None)))), Step::Apply("b:music".into()));
        assert_eq!(a.step("chrome.exe", &rules, Some(("{b}", &curve(Some("b:music"))))), Step::Keep);
        // Reglas desactivadas con el preset puesto: vuelve lo de antes
        assert_eq!(a.step("spotify.exe", &rules, Some(("{a}", &curve(None)))), Step::Apply("b:music".into()));
        assert_eq!(a.release(Some(("{a}", &curve(Some("b:music"))))), Step::Restore("{a}".into(), curve(None)));
        assert_eq!(a.release(Some(("{a}", &curve(Some("b:music"))))), Step::Keep);
    }

    #[test]
    fn rearm_after_rules_change() {
        let mut a = Auto::new("poxieq.exe");
        assert_eq!(a.step("spotify.exe", &[], None), Step::Keep);
        let rules = vec![rule("spotify.exe", "b:music")];
        assert_eq!(a.step("spotify.exe", &rules, None), Step::Keep);
        a.rearm();
        assert_eq!(a.step("spotify.exe", &rules, None), Step::Apply("b:music".into()));
    }
}
