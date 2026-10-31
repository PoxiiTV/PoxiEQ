//! Atajos globales: formato, valores por defecto y validación de combinaciones.
//!
//! Se guardan en la forma canónica del registrador de atajos (`control+alt+Numpad0`). Con un teclado con
//! AltGr, Windows trata Ctrl+Alt como AltGr: un atajo Ctrl+Alt+tecla que escribe un carácter con AltGr
//! (en español `Ctrl+Alt+2` = `@`, `Ctrl+Alt+E` = `€`) dejaría de escribirlo en todo el sistema mientras
//! esté registrado, así que se rechaza.

use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// Huecos de preset
pub const SLOTS: usize = 9;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    /// Encender/apagar el EQ (`None`: sin atajo)
    pub bypass: Option<String>,
    pub presets: Vec<PresetKey>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetKey {
    pub keys: Option<String>,
    pub preset: Option<String>,
}

/// Por defecto, en el teclado numérico: ni choca con AltGr ni con los atajos de casi ningún programa
impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            bypass: Some("control+alt+Numpad0".into()),
            presets: (1..=SLOTS).map(|n| PresetKey { keys: Some(format!("control+alt+Numpad{n}")), preset: None }).collect(),
        }
    }
}

impl Hotkeys {
    /// Atajos del formato anterior (`bypassHotkey` + `presetKeys` en Ctrl+Alt+1…9): se conservan las
    /// asignaciones y las teclas pasan a las de por defecto (las de antes chocaban con AltGr)
    pub fn migrate(bypass_on: Option<bool>, preset_ids: Option<Vec<Option<String>>>) -> Self {
        let mut h = Self::default();
        if bypass_on == Some(false) {
            h.bypass = None;
        }
        for (slot, id) in h.presets.iter_mut().zip(preset_ids.unwrap_or_default()) {
            slot.preset = id;
        }
        h
    }

    /// Lo que no es válido se quita (sin atajo) y una combinación repetida se queda solo en el primer sitio
    pub fn sanitize(&mut self, valid_preset: impl Fn(&str) -> bool) {
        self.presets.resize(SLOTS, PresetKey::default());
        self.presets.truncate(SLOTS);
        let mut seen: Vec<String> = vec![];
        let mut keep = |k: &mut Option<String>| {
            *k = k.as_deref().and_then(|s| check(s).ok()).filter(|s| !seen.contains(s));
            if let Some(s) = k {
                seen.push(s.clone());
            }
        };
        keep(&mut self.bypass);
        for p in &mut self.presets {
            keep(&mut p.keys);
            if p.preset.as_deref().is_some_and(|id| !valid_preset(id)) {
                p.preset = None;
            }
        }
    }
}

/// Teclas que con AltGr escriben algo en casi todos los teclados europeos: la fila de números, la E (€) y
/// las de símbolos. Las letras se comprueban además con la distribución de teclado activa.
const ALTGR_KEYS: [Code; 23] = [
    Code::Digit0,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
    Code::KeyE,
    Code::Backquote,
    Code::Minus,
    Code::Equal,
    Code::BracketLeft,
    Code::BracketRight,
    Code::Backslash,
    Code::Semicolon,
    Code::Quote,
    Code::Comma,
    Code::Period,
    Code::Slash,
    Code::IntlBackslash,
];

/// Comprueba una combinación y la devuelve en forma canónica. Errores (códigos de la interfaz):
/// `keys.invalid` (no se entiende o no se puede registrar), `keys.noModifier` (hace falta Ctrl, Alt o Win:
/// Mayús sola escribiría en mayúsculas) y `keys.altGr`.
pub fn check(keys: &str) -> Result<String, &'static str> {
    if keys.len() > 64 {
        return Err("keys.invalid");
    }
    let s: Shortcut = keys.parse().map_err(|_| "keys.invalid")?;
    if !s.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        return Err("keys.noModifier");
    }
    if s.mods.contains(Modifiers::CONTROL | Modifiers::ALT) && (ALTGR_KEYS.contains(&s.key) || altgr_writes(s.key, s.mods.contains(Modifiers::SHIFT))) {
        return Err("keys.altGr");
    }
    Ok(s.into_string())
}

/// Con la distribución de teclado de la ventana activa, ¿AltGr (+ Mayús) + esta letra escribe algo?
fn altgr_writes(key: Code, shift: bool) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let letters = [
        Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF, Code::KeyG, Code::KeyH, Code::KeyI, Code::KeyJ, Code::KeyK, Code::KeyL, Code::KeyM,
        Code::KeyN, Code::KeyO, Code::KeyP, Code::KeyQ, Code::KeyR, Code::KeyS, Code::KeyT, Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX, Code::KeyY, Code::KeyZ,
    ];
    let Some(i) = letters.iter().position(|c| *c == key) else { return false };
    let vk = b'A' as u32 + i as u32;
    unsafe {
        let layout = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), None));
        let mut state = [0u8; 256];
        for k in [VK_CONTROL, VK_MENU, VK_LCONTROL, VK_RMENU] {
            state[k.0 as usize] = 0x80;
        }
        if shift {
            state[VK_SHIFT.0 as usize] = 0x80;
        }
        let scan = MapVirtualKeyExW(vk, MAPVK_VK_TO_VSC, Some(layout));
        let mut buf = [0u16; 8];
        // 0x4: sin tocar el estado del teclado (teclas muertas) del sistema
        ToUnicodeEx(vk, scan, &state, &mut buf, 0x4, Some(layout)) != 0
    }
}

/// Texto corto para el aviso en pantalla: `control+alt+Numpad0` → `Ctrl+Alt+Num 0`
pub fn label(keys: &str) -> String {
    keys.split('+')
        .map(|t| match t {
            "control" => "Ctrl".to_string(),
            "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            "super" => "Win".into(),
            k => {
                if let Some(d) = k.strip_prefix("Numpad") {
                    let sym = match d {
                        "Add" => "+",
                        "Subtract" => "−",
                        "Multiply" => "*",
                        "Divide" => "/",
                        "Decimal" => ".",
                        "Enter" => "Enter",
                        "Equal" => "=",
                        n => n,
                    };
                    format!("Num {sym}")
                } else {
                    k.strip_prefix("Key").or_else(|| k.strip_prefix("Digit")).unwrap_or(k).to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinaciones() {
        assert_eq!(check("Ctrl+Alt+Numpad0"), Ok("control+alt+Numpad0".into()));
        assert_eq!(check("control+alt+Numpad0"), Ok("control+alt+Numpad0".into()));
        assert_eq!(check("ctrl+shift+KeyM"), Ok("shift+control+KeyM".into()));
        assert_eq!(check("super+F9"), Ok("super+F9".into()));
        assert_eq!(check("alt+F10"), Ok("alt+F10".into()));
        // Sin modificador o solo con Mayús: escribiría
        assert_eq!(check("KeyA"), Err("keys.noModifier"));
        assert_eq!(check("F5"), Err("keys.noModifier"));
        assert_eq!(check("shift+KeyA"), Err("keys.noModifier"));
        // AltGr: fila de números, E y símbolos (también con Mayús)
        for k in ["control+alt+Digit2", "control+alt+KeyE", "control+alt+Backquote", "shift+control+alt+Digit3", "control+alt+BracketLeft"] {
            assert_eq!(check(k), Err("keys.altGr"), "{k}");
        }
        // Sin Ctrl+Alt a la vez, la fila de números sí vale
        assert_eq!(check("control+Digit2"), Ok("control+Digit2".into()));
        assert_eq!(check("alt+shift+Digit2"), Ok("shift+alt+Digit2".into()));
        for k in ["", "control+", "control+alt", "control+alt+Nada", "control+KeyA+KeyB", &"control+".repeat(40)] {
            assert_eq!(check(k), Err("keys.invalid"), "{k}");
        }
    }

    #[test]
    fn por_defecto_y_migracion() {
        let d = Hotkeys::default();
        assert_eq!(d.presets.len(), SLOTS);
        for k in std::iter::once(&d.bypass).chain(d.presets.iter().map(|p| &p.keys)) {
            let k = k.as_deref().unwrap();
            assert_eq!(check(k).as_deref(), Ok(k), "{k}");
        }
        assert_eq!(label(d.bypass.as_deref().unwrap()), "Ctrl+Alt+Num 0");
        assert_eq!(label("shift+control+KeyM"), "Shift+Ctrl+M");
        assert_eq!(label("super+NumpadAdd"), "Win+Num +");

        // Formato anterior: se conservan los presets de cada hueco y el bypass apagado
        let ids = vec![Some("b:bass".to_string()), None, Some("u:x".into())];
        let h = Hotkeys::migrate(Some(false), Some(ids));
        assert_eq!(h.bypass, None);
        assert_eq!(h.presets[0], PresetKey { keys: Some("control+alt+Numpad1".into()), preset: Some("b:bass".into()) });
        assert_eq!(h.presets[1].preset, None);
        assert_eq!(h.presets[2].preset.as_deref(), Some("u:x"));
        assert_eq!(Hotkeys::migrate(Some(true), None), Hotkeys::default());
    }

    #[test]
    fn sanitize_quita_lo_invalido_y_lo_repetido() {
        let mut h = Hotkeys {
            bypass: Some("Ctrl+Alt+F1".into()),
            presets: vec![
                PresetKey { keys: Some("control+alt+F1".into()), preset: Some("b:flat".into()) },
                PresetKey { keys: Some("control+alt+Digit1".into()), preset: Some("z:mal".into()) },
                PresetKey { keys: Some("control+alt+F2".into()), preset: None },
            ],
        };
        h.sanitize(|id| id.starts_with("b:"));
        assert_eq!(h.bypass.as_deref(), Some("control+alt+F1"));
        assert_eq!(h.presets.len(), SLOTS);
        assert_eq!(h.presets[0], PresetKey { keys: None, preset: Some("b:flat".into()) }, "repetida con el bypass");
        assert_eq!(h.presets[1], PresetKey::default(), "AltGr y preset inválido");
        assert_eq!(h.presets[2].keys.as_deref(), Some("control+alt+F2"));
        assert_eq!(h.presets[8], PresetKey::default());
    }
}
