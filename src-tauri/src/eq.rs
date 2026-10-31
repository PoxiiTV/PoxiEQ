//! Modelo del ecualizador, validación y traducción al formato de Equalizer APO.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub kind: Kind,
    pub freq: f64,
    pub gain: f64,
    pub q: f64,
    pub on: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Curve {
    pub preamp: f64,
    pub auto_preamp: bool,
    pub bands: Vec<Band>,
    /// Preset del que sale (para marcarlo en la lista); se borra al tocar la curva
    pub preset: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub curve: Curve,
}

pub const MAX_BANDS: usize = 32;

fn clamp(v: f64, lo: f64, hi: f64, def: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        def
    }
}

impl Curve {
    /// Lo que llega de la interfaz no se da por bueno: rangos que Equalizer APO acepta sin sorpresas.
    pub fn sanitize(&mut self) {
        self.preamp = clamp(self.preamp, -30.0, 12.0, 0.0);
        self.bands.truncate(MAX_BANDS);
        for b in &mut self.bands {
            b.freq = clamp(b.freq, 20.0, 20_000.0, 1000.0);
            b.gain = clamp(b.gain, -24.0, 24.0, 0.0);
            b.q = clamp(b.q, 0.1, 20.0, 1.0);
        }
        if let Some(p) = &mut self.preset {
            p.truncate(64);
        }
    }
}

pub fn sanitize_presets(presets: &mut Vec<Preset>) {
    presets.truncate(200);
    for p in presets.iter_mut() {
        p.name = p.name.trim().chars().filter(|c| !c.is_control()).take(48).collect();
        p.id.truncate(64);
        p.curve.sanitize();
    }
    presets.retain(|p| !p.name.is_empty() && !p.id.is_empty());
}

/// Texto de `poxieq.txt`: una sección `Device:` por dispositivo, nada si está en bypass.
pub fn render(curves: &HashMap<String, Curve>, bypass: bool) -> String {
    let mut out = String::from("# PoxiEQ: archivo generado automáticamente, los cambios a mano se pierden\r\n");
    if bypass {
        out.push_str("# Bypass activado\r\n");
        return out;
    }
    let mut ids: Vec<&String> = curves.keys().collect();
    ids.sort();
    for id in ids {
        // Solo GUIDs: es lo único que Equalizer APO compara sin ambigüedad
        if !id.starts_with('{') || !id.ends_with('}') || !id[1..id.len() - 1].chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            continue;
        }
        let c = &curves[id];
        out.push_str(&format!("\r\nDevice: {id}\r\nPreamp: {:.1} dB\r\n", c.preamp));
        for b in c.bands.iter().filter(|b| b.on) {
            let line = match b.kind {
                Kind::Peak => format!("PK Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
                Kind::LowShelf => format!("LSC Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
                Kind::HighShelf => format!("HSC Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
                Kind::LowPass => format!("LPQ Fc {:.1} Hz Q {:.3}", b.freq, b.q),
                Kind::HighPass => format!("HPQ Fc {:.1} Hz Q {:.3}", b.freq, b.q),
            };
            out.push_str(&format!("Filter: ON {line}\r\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_sanitize() {
        let mut c = Curve {
            preamp: -99.0,
            auto_preamp: false,
            bands: vec![
                Band { kind: Kind::Peak, freq: 5.0, gain: 3.0, q: 1.41, on: true },
                Band { kind: Kind::HighPass, freq: 30.0, gain: 0.0, q: f64::NAN, on: true },
                Band { kind: Kind::LowShelf, freq: 100.0, gain: 6.0, q: 0.7, on: false },
            ],
            preset: None,
        };
        c.sanitize();
        assert_eq!(c.preamp, -30.0);
        assert_eq!(c.bands[0].freq, 20.0);
        assert_eq!(c.bands[1].q, 1.0);

        let id = "{6af377fe-6370-495b-afe3-95fac2e918f7}".to_string();
        let mut curves = HashMap::from([(id.clone(), c), ("Device: all\r\nPreamp: 20 dB".into(), Curve::default())]);
        let text = render(&curves, false);
        assert!(text.contains(&format!("Device: {id}\r\nPreamp: -30.0 dB")));
        assert!(text.contains("Filter: ON PK Fc 20.0 Hz Gain 3.0 dB Q 1.410"));
        assert!(text.contains("Filter: ON HPQ Fc 30.0 Hz Q 1.000"));
        assert!(!text.contains("LSC"), "las bandas apagadas no se escriben");
        assert!(!text.contains("20 dB"), "un id que no es GUID no se cuela en el archivo");

        curves.clear();
        assert!(!render(&curves, true).contains("Device"));
    }
}
