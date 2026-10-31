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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub kind: Kind,
    pub freq: f64,
    pub gain: f64,
    pub q: f64,
    pub on: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Curve {
    pub preamp: f64,
    pub auto_preamp: bool,
    pub bands: Vec<Band>,
    /// Preset del que sale (para marcarlo en la lista); se borra al tocar la curva
    pub preset: Option<String>,
    /// Ajuste de cada oído por separado (sale del test de audición), después de las bandas comunes
    pub ears: Option<Ears>,
}

/// Bandas solo para el oído izquierdo y solo para el derecho
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Ears {
    pub left: Vec<Band>,
    pub right: Vec<Band>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub curve: Curve,
    /// Corrección de sala: la salida en la que se midió (la página «Sala» enseña su último resultado)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

pub const MAX_BANDS: usize = 32;

pub fn clamp(v: f64, lo: f64, hi: f64, def: f64) -> f64 {
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
        sanitize_bands(&mut self.bands);
        if let Some(e) = &mut self.ears {
            sanitize_bands(&mut e.left);
            sanitize_bands(&mut e.right);
        }
        if let Some(p) = &mut self.preset {
            p.truncate(64);
        }
    }
}

fn sanitize_bands(bands: &mut Vec<Band>) {
    bands.truncate(MAX_BANDS);
    for b in bands {
        b.freq = clamp(b.freq, 20.0, 20_000.0, 1000.0);
        b.gain = clamp(b.gain, -24.0, 24.0, 0.0);
        b.q = clamp(b.q, 0.1, 20.0, 1.0);
    }
}

/// Línea `Filter:` del motor para una banda
fn filter_line(b: &Band) -> String {
    let f = match b.kind {
        Kind::Peak => format!("PK Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
        Kind::LowShelf => format!("LSC Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
        Kind::HighShelf => format!("HSC Fc {:.1} Hz Gain {:.1} dB Q {:.3}", b.freq, b.gain, b.q),
        Kind::LowPass => format!("LPQ Fc {:.1} Hz Q {:.3}", b.freq, b.q),
        Kind::HighPass => format!("HPQ Fc {:.1} Hz Q {:.3}", b.freq, b.q),
    };
    format!("Filter: ON {f}\r\n")
}

fn filters<'a>(bands: impl IntoIterator<Item = &'a Band>) -> String {
    bands.into_iter().filter(|b| b.on).map(filter_line).collect()
}

/// Respuesta en dB de una banda a `f` (biquads RBJ a 48 kHz, como `bandDb` en `eq.ts` y el motor)
pub fn band_db(b: &Band, f: f64) -> f64 {
    use std::f64::consts::PI;
    let a = 10f64.powf(b.gain / 40.0);
    let w0 = 2.0 * PI * b.freq / 48000.0;
    let (cos, alpha) = (w0.cos(), w0.sin() / (2.0 * b.q));
    let sq = 2.0 * a.sqrt() * alpha;
    let c = match b.kind {
        Kind::Peak => [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        Kind::LowShelf => [
            a * (a + 1.0 - (a - 1.0) * cos + sq),
            2.0 * a * (a - 1.0 - (a + 1.0) * cos),
            a * (a + 1.0 - (a - 1.0) * cos - sq),
            a + 1.0 + (a - 1.0) * cos + sq,
            -2.0 * (a - 1.0 + (a + 1.0) * cos),
            a + 1.0 + (a - 1.0) * cos - sq,
        ],
        Kind::HighShelf => [
            a * (a + 1.0 + (a - 1.0) * cos + sq),
            -2.0 * a * (a - 1.0 + (a + 1.0) * cos),
            a * (a + 1.0 + (a - 1.0) * cos - sq),
            a + 1.0 - (a - 1.0) * cos + sq,
            2.0 * (a - 1.0 - (a + 1.0) * cos),
            a + 1.0 - (a - 1.0) * cos - sq,
        ],
        Kind::LowPass => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        Kind::HighPass => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
    };
    let w = 2.0 * PI * f / 48000.0;
    let resp = |k: [f64; 3]| {
        let re = k[0] + k[1] * w.cos() + k[2] * (2.0 * w).cos();
        let im = -(k[1] * w.sin() + k[2] * (2.0 * w).sin());
        re * re + im * im
    };
    10.0 * (resp([c[0], c[1], c[2]]) / resp([c[3], c[4], c[5]])).log10()
}

pub fn sanitize_presets(presets: &mut Vec<Preset>) {
    presets.truncate(200);
    for p in presets.iter_mut() {
        p.name = clean_name(&p.name);
        p.id.truncate(64);
        p.curve.sanitize();
        if p.room.as_deref().is_some_and(|r| r.len() != 38 || !is_guid(r)) {
            p.room = None;
        }
    }
    presets.retain(|p| !p.name.is_empty() && !p.id.is_empty());
}

/// Corrección de sonoridad del motor: más graves y agudos cuanto más bajo está el volumen de Windows
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Loudness {
    pub on: bool,
    /// dB que se suman al nivel de referencia (calibración a tu volumen de escucha habitual)
    pub offset: f64,
    /// 0…1: cuánto se aplica
    pub strength: f64,
}

impl Default for Loudness {
    fn default() -> Self {
        Self { on: false, offset: 0.0, strength: 0.4 }
    }
}

impl Loudness {
    pub fn sanitize(&mut self) {
        self.offset = clamp(self.offset, -30.0, 30.0, 0.0).round();
        self.strength = clamp(self.strength, 0.0, 1.0, 1.0);
    }
}

// ------------------------------------------------------------ estudio

/// Qué se escucha (monitorización). «Mono» es el mid: (L + R) / 2 en los dos oídos.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub enum Listen {
    #[default]
    Stereo,
    Mono,
    Side,
    Left,
    Right,
}

/// Otros sistemas de escucha para comprobar cómo traduce una mezcla
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Sim {
    Phone,
    Laptop,
    Car,
    Earbuds,
    Tv,
    Speaker,
}

/// Herramientas de estudio: momentáneas, no se guardan y se apagan al cerrar la ventana
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Studio {
    pub listen: Listen,
    /// Polaridad invertida del canal izquierdo / derecho
    pub flip_l: bool,
    pub flip_r: bool,
    /// L y R intercambiados
    pub swap: bool,
    pub sim: Option<Sim>,
}

const fn band(kind: Kind, freq: f64, gain: f64, q: f64) -> Band {
    Band { kind, freq, gain, q, on: true }
}
const fn pk(freq: f64, gain: f64, q: f64) -> Band {
    band(Kind::Peak, freq, gain, q)
}
const fn hp(freq: f64, q: f64) -> Band {
    band(Kind::HighPass, freq, 0.0, q)
}
const fn lp(freq: f64, q: f64) -> Band {
    band(Kind::LowPass, freq, 0.0, q)
}

/// Curva aproximada de un sistema: anchura estéreo (1 = tal cual, 0 = mono), preamp (el pico de la curva
/// queda en 0 dB: nunca sube el nivel) y filtros. Los pasos altos de 24 dB/oct son dos biquads con las Q de
/// Butterworth (0,541 y 1,307).
struct SimCurve {
    width: f64,
    preamp: f64,
    bands: &'static [Band],
}

/// Respuestas típicas de cada tipo de aparato (aproximadas a partir de medidas publicadas), no de un modelo
/// concreto. El preamp es el pico de cada curva cambiado de signo (lo comprueba un test).
const SIMS: [(Sim, SimCurve); 6] = [
    // Altavoz de móvil: mono, nada por debajo de ~380 Hz, joroba del altavoz cerca de 1 kHz, presencia
    // marcada a 3 kHz y caída por encima de 9 kHz
    (Sim::Phone, SimCurve { width: 0.0, preamp: -5.1, bands: &[hp(380.0, 0.541), hp(380.0, 1.307), pk(900.0, 2.0, 1.2), pk(3000.0, 5.0, 1.0), lp(9000.0, 0.7)] }),
    // Portátil: altavoces juntos (estéreo estrecho), corte en ~180 Hz con resonancia justo encima, medios
    // «de caja» y algo de brillo a 4 kHz
    (Sim::Laptop, SimCurve { width: 0.6, preamp: -3.2, bands: &[hp(180.0, 0.541), hp(180.0, 1.307), pk(250.0, 3.0, 1.5), pk(600.0, 2.0, 1.0), pk(4000.0, 3.0, 1.5), lp(14000.0, 0.7)] }),
    // Coche: ganancia del habitáculo en graves (+5 dB a 50 Hz), hueco de modos a 250 Hz, agudos absorbidos
    // por la tapicería e imagen estéreo descentrada (anchura reducida)
    (Sim::Car, SimCurve { width: 0.5, preamp: -5.2, bands: &[hp(25.0, 0.7), band(Kind::LowShelf, 90.0, 6.0, 0.7), pk(250.0, -3.0, 1.4), band(Kind::HighShelf, 9000.0, -4.0, 0.7)] }),
    // Auriculares de botón sin sellar (tipo AirPods): pierden graves por la fuga, resonancia del canal
    // auditivo a 3 kHz, valle a 6 kHz y agudos algo apagados
    (Sim::Earbuds, SimCurve { width: 1.0, preamp: -2.8, bands: &[hp(90.0, 0.6), band(Kind::LowShelf, 200.0, -3.0, 0.7), pk(3000.0, 3.0, 1.2), pk(6000.0, -3.0, 2.0), band(Kind::HighShelf, 10000.0, -2.0, 0.7)] }),
    // Tele plana: altavoces pequeños hacia abajo o atrás, corte en ~120 Hz, algo de caja a 200 Hz, medios
    // hundidos y realce de voz a 3,5 kHz; estéreo casi nulo a distancia de sofá
    (Sim::Tv, SimCurve { width: 0.4, preamp: -2.8, bands: &[hp(120.0, 0.541), hp(120.0, 1.307), pk(200.0, 2.0, 1.0), pk(1200.0, -2.0, 1.0), pk(3500.0, 3.0, 1.2), lp(12000.0, 0.7)] }),
    // Altavoz bluetooth pequeño: mono, joroba de graves del radiador pasivo a 110 Hz y nada por debajo de
    // 75 Hz, medios algo hundidos y presencia
    (Sim::Speaker, SimCurve { width: 0.0, preamp: -4.7, bands: &[hp(75.0, 0.541), hp(75.0, 1.307), pk(110.0, 5.0, 1.5), pk(400.0, -2.0, 1.0), pk(3000.0, 2.0, 1.2), lp(15000.0, 0.7)] }),
];

impl Sim {
    fn curve(self) -> &'static SimCurve {
        &SIMS.iter().find(|(s, _)| *s == self).expect("todas las simulaciones tienen curva").1
    }
}

/// `Copy:` del motor para una matriz 2×2 (`m[salida][entrada]`, entradas L y R); nada si es la identidad.
/// El motor lee cada factor con `wcstod` (`-0.5*R`); los términos a cero no se escriben.
fn copy_line(m: [[f64; 2]; 2]) -> String {
    if m == [[1.0, 0.0], [0.0, 1.0]] {
        return String::new();
    }
    let row = |r: [f64; 2]| {
        let terms: Vec<String> = r.iter().zip(["L", "R"]).filter(|(k, _)| **k != 0.0).map(|(k, ch)| format!("{k}*{ch}")).collect();
        terms.join("+")
    };
    format!("Copy: L={} R={}\r\n", row(m[0]), row(m[1]))
}

impl Studio {
    /// Matriz de la monitorización (`m[salida][entrada]`): polaridad → intercambio → escucha
    pub fn matrix(&self) -> [[f64; 2]; 2] {
        let pl = if self.flip_l { -1.0 } else { 1.0 };
        let pr = if self.flip_r { -1.0 } else { 1.0 };
        // Lo que va por cada lado tras la polaridad y el intercambio
        let (l, r) = if self.swap { ([0.0, pr], [pl, 0.0]) } else { ([pl, 0.0], [0.0, pr]) };
        let mix = |a: f64, b: f64| [a * l[0] + b * r[0], a * l[1] + b * r[1]];
        match self.listen {
            Listen::Stereo => [l, r],
            Listen::Mono => [mix(0.5, 0.5); 2],
            Listen::Side => [mix(0.5, -0.5); 2],
            Listen::Left => [l, l],
            Listen::Right => [r, r],
        }
    }

    /// Líneas para todas las salidas, delante de la curva. La matriz y la simulación son lineales e iguales
    /// en los dos canales, igual que el EQ común: el orden entre ellos no cambia el resultado.
    fn render(&self) -> String {
        let mut out = copy_line(self.matrix());
        if let Some(sim) = self.sim {
            let c = sim.curve();
            let (a, b) = ((1.0 + c.width) / 2.0, (1.0 - c.width) / 2.0);
            out.push_str(&copy_line([[a, b], [b, a]]));
            out.push_str(&format!("Preamp: {:.1} dB\r\n", c.preamp));
            out.push_str(&filters(c.bands));
        }
        if !out.is_empty() {
            // Solo para las salidas: sin `Stage`, lo de antes del primer `Device:` lo procesaría también el
            // micrófono (en captura todas las etapas coinciden por defecto)
            out.insert_str(0, "# Herramientas de estudio (momentáneas)\r\nStage: post-mix\r\n");
        }
        out
    }
}

/// Micrófono: su propio bypass, aparte del de las salidas (el atajo de bypass no quita la puerta en un
/// directo). Su EQ va en `Config.curves` y su puerta/compresor en `Config.per_device`, con el GUID de la entrada.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Mic {
    pub bypass: bool,
    /// Formato anterior: una puerta y un compresor para todos los micrófonos. Solo se lee, para repartirlo.
    #[serde(skip_serializing)]
    pub dsp: Option<poxidsp::Settings>,
}

// ------------------------------------------------- ajustes por dispositivo y presets de sección

/// Lo que cada dispositivo recuerda aparte de su EQ (que va en `Config.curves`)
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceSettings {
    /// PoxiDSP: en una salida, Sonido y Cascos; en un micrófono, su puerta y su compresor
    pub dsp: poxidsp::Settings,
    /// Último preset elegido en cada sección
    pub presets: Picks,
}

/// Id del último preset elegido en cada sección (`None`: ninguno)
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Picks {
    pub sound: Option<String>,
    pub phones: Option<String>,
    pub mic: Option<String>,
}

/// Secciones con presets propios (los del Ecualizador son [`Preset`])
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Section {
    Sound,
    Phones,
    Studio,
    Mic,
}

/// Preset de una sección. Cada una guarda solo lo suyo (lo demás no se escribe y se descarta al validar):
/// Sonido y Cascos sus partes de PoxiDSP, Micrófono su EQ y su puerta/compresor, Estudio la monitorización
/// y la simulación.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SectionPreset {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsp: Option<poxidsp::Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio: Option<Studio>,
}

/// Presets propios de cada sección
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SectionPresets {
    pub sound: Vec<SectionPreset>,
    pub phones: Vec<SectionPreset>,
    pub studio: Vec<SectionPreset>,
    pub mic: Vec<SectionPreset>,
}

/// Presets como mucho en cada sección
pub const MAX_SECTION_PRESETS: usize = 100;

/// Nombre de preset: sin caracteres de control, sin espacios a los lados y como mucho 48 caracteres
pub fn clean_name(name: &str) -> String {
    name.trim().chars().filter(|c| !c.is_control()).take(48).collect::<String>().trim().to_string()
}

/// Solo las partes de PoxiDSP de una sección; el resto, por defecto (apagado)
pub fn section_dsp(dsp: &poxidsp::Settings, section: Section) -> poxidsp::Settings {
    let d = poxidsp::Settings::default();
    match section {
        Section::Sound => poxidsp::Settings { leveler: dsp.leveler, comp: dsp.comp, limiter: dsp.limiter, bass_boost: dsp.bass_boost, bass: dsp.bass, dialog: dsp.dialog, ..d },
        Section::Phones => poxidsp::Settings { crossfeed: dsp.crossfeed, speakers: dsp.speakers, ..d },
        Section::Mic => poxidsp::Settings { gate: dsp.gate, comp: dsp.comp, ..d },
        Section::Studio => d,
    }
}

impl SectionPresets {
    fn lists(&mut self) -> [(Section, &mut Vec<SectionPreset>); 4] {
        [(Section::Sound, &mut self.sound), (Section::Phones, &mut self.phones), (Section::Studio, &mut self.studio), (Section::Mic, &mut self.mic)]
    }

    /// Ids propios válidos, nombres limpios, sin repetidos, rangos de cada ajuste y solo lo de su sección
    /// (lo que le falta a una sección para tener sentido descarta el preset)
    pub fn sanitize(&mut self) {
        for (section, list) in self.lists() {
            let mut seen = std::collections::HashSet::new();
            list.retain_mut(|p| {
                p.name = clean_name(&p.name);
                let keep = !p.name.is_empty() && p.id.starts_with("u:") && crate::apps::valid_preset_id(&p.id) && seen.insert(p.id.clone());
                let dsp = p.dsp.take().map(|mut d| {
                    d.sanitize();
                    section_dsp(&d, section)
                });
                let (curve, studio) = (p.curve.take(), p.studio.take());
                match section {
                    Section::Sound | Section::Phones => p.dsp = dsp,
                    Section::Mic => {
                        p.dsp = dsp;
                        p.curve = curve.map(|mut c| {
                            c.sanitize();
                            Curve { preset: None, ears: None, ..c }
                        });
                    }
                    Section::Studio => p.studio = studio,
                }
                keep && match section {
                    Section::Sound | Section::Phones => p.dsp.is_some(),
                    Section::Mic => p.dsp.is_some() && p.curve.is_some(),
                    Section::Studio => p.studio.is_some(),
                }
            });
            list.truncate(MAX_SECTION_PRESETS);
        }
    }

    fn has(&self, section: Section, id: &str) -> bool {
        let list = match section {
            Section::Sound => &self.sound,
            Section::Phones => &self.phones,
            Section::Studio => &self.studio,
            Section::Mic => &self.mic,
        };
        list.iter().any(|p| p.id == id)
    }
}

/// Ajustes de cada dispositivo: rangos de PoxiDSP y el último preset de cada sección solo si existe
pub fn sanitize_devices(devices: &mut HashMap<String, DeviceSettings>, presets: &SectionPresets) {
    devices.retain(|id, _| id.len() == 38 && is_guid(id));
    for d in devices.values_mut() {
        d.dsp.sanitize();
        let p = &mut d.presets;
        for (section, pick) in [(Section::Sound, &mut p.sound), (Section::Phones, &mut p.phones), (Section::Mic, &mut p.mic)] {
            if pick.as_deref().is_some_and(|id| !presets.has(section, id)) {
                *pick = None;
            }
        }
    }
}
/// Lo que suena ahora sin guardarse en la configuración
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Live {
    /// A/B: solo el preamp indicado, el original igualado en volumen a la curva
    pub compare: Option<f64>,
    pub studio: Studio,
    /// Test de audición: todo en plano (sin EQ, dinámica ni herramientas)
    pub hearing: bool,
    /// Medición de sala: todo en plano, también el micrófono (graba lo que suena de verdad)
    pub measuring: bool,
}

/// Solo GUIDs: es lo único que Equalizer APO compara sin ambigüedad
fn is_guid(id: &str) -> bool {
    id.starts_with('{') && id.ends_with('}') && id[1..id.len() - 1].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Texto de `poxieq.txt`. Primero las herramientas de estudio, antes de cualquier `Device:` (valen para todas
/// las salidas, también en bypass: son de escucha, no del EQ); luego una sección `Device:` por salida, nada
/// si está en bypass. Cada dispositivo lleva su propio PoxiDSP al final (`devices`), solo en L/R y solo si
/// tiene algo encendido. Un dispositivo sin curva guardada suena plano (con su PoxiDSP si lo tiene).
///
/// Las entradas (`is_input`) llevan su propia sección con `Stage: capture` (su única etapa), su EQ y su
/// puerta/compresor. No dependen del bypass, del A/B ni del test de audición, solo de `mic_bypass`.
pub fn render(curves: &HashMap<String, Curve>, devices: &HashMap<String, DeviceSettings>, bypass: bool, loudness: &Loudness, mic_bypass: bool, live: &Live, is_input: impl Fn(&str) -> bool) -> String {
    let mut out = String::from("# PoxiEQ: archivo generado automáticamente, los cambios a mano se pierden\r\n");
    if live.measuring {
        out.push_str("# Medición de sala: todo en plano\r\n");
        return out;
    }
    let flat = Curve::default();
    let none = poxidsp::Settings::default();
    // Con curva o con algo de PoxiDSP encendido; ordenados para que el archivo no cambie sin motivo
    let ids: std::collections::BTreeSet<&String> = curves
        .keys()
        .chain(devices.iter().filter(|(_, d)| d.dsp.active()).map(|(id, _)| id))
        .filter(|id| is_guid(id))
        .collect();
    let (inputs, outputs): (Vec<_>, Vec<_>) = ids
        .into_iter()
        .map(|id| (id.as_str(), curves.get(id).unwrap_or(&flat), devices.get(id).map_or(&none, |d| &d.dsp)))
        .partition(|(id, _, _)| is_input(id));
    if live.hearing {
        out.push_str("# Test de audición: todo en plano\r\n");
    } else {
        out.push_str(&live.studio.render());
        if bypass {
            out.push_str("# Bypass activado\r\n");
        } else {
            render_outputs(&mut out, &outputs, loudness, live);
        }
    }
    if !mic_bypass {
        for (id, c, dsp) in inputs {
            out.push_str(&format!("\r\nDevice: {id}\r\nStage: capture\r\nPreamp: {:.1} dB\r\n", c.preamp));
            out.push_str(&filters(&c.bands));
            // Todos los canales (un micro mono es «C», no «L R»): el motor reparte de dos en dos
            if dsp.active() {
                let chunk = poxidsp::base64(&dsp.chunk(poxidsp::device_key(id)));
                out.push_str(&format!("Channel: all\r\nVSTPlugin: Library \"{}\" ChunkData \"{chunk}\"\r\n", poxidsp::DLL_NAME));
            }
        }
    }
    out
}

fn render_outputs(out: &mut String, outputs: &[(&str, &Curve, &poxidsp::Settings)], loudness: &Loudness, live: &Live) {
    for &(id, c, dsp) in outputs {
        if let Some(g) = live.compare {
            out.push_str(&format!("\r\nDevice: {id}\r\nPreamp: {:.1} dB\r\n", clamp(g, -30.0, 12.0, 0.0)));
            continue;
        }
        out.push_str(&format!("\r\nDevice: {id}\r\n"));
        if loudness.on {
            // El motor compara la referencia con el volumen de Windows en dB (100 % = 0 dB, el resto
            // negativo): la referencia es el volumen máximo. Con 75 (dB SPL) metía +100 dB y silenciaba todo.
            out.push_str(&format!(
                "LoudnessCorrection: State 1 ReferenceLevel 0 ReferenceOffset {} Attenuation {:.2}\r\n",
                loudness.offset as i32, loudness.strength
            ));
        }
        out.push_str(&format!("Preamp: {:.1} dB\r\n", c.preamp));
        out.push_str(&filters(&c.bands));
        // Cada oído lo suyo, antes de PoxiDSP para que el limitador siga siendo lo último
        if let Some(e) = c.ears.as_ref().filter(|e| e.left.iter().chain(&e.right).any(|b| b.on)) {
            out.push_str(&format!("Channel: L\r\n{}Channel: R\r\n{}Channel: all\r\n", filters(&e.left), filters(&e.right)));
        }
        if dsp.active() {
            let chunk = poxidsp::base64(&dsp.chunk(poxidsp::device_key(id)));
            out.push_str(&format!("Channel: L R\r\nVSTPlugin: Library \"{}\" ChunkData \"{chunk}\"\r\nChannel: all\r\n", poxidsp::DLL_NAME));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `render` sin micrófonos y con el mismo PoxiDSP en todas las salidas con curva
    fn out(curves: &HashMap<String, Curve>, bypass: bool, loudness: &Loudness, dsp: &poxidsp::Settings, live: &Live) -> String {
        let devices = curves.keys().map(|id| (id.clone(), DeviceSettings { dsp: *dsp, ..DeviceSettings::default() })).collect();
        render(curves, &devices, bypass, loudness, false, live, |_| false)
    }

    fn with_dsp(dsp: poxidsp::Settings) -> DeviceSettings {
        DeviceSettings { dsp, ..DeviceSettings::default() }
    }

    #[test]
    fn microfono_en_su_etapa() {
        let out_id = "{6af377fe-6370-495b-afe3-95fac2e918f7}".to_string();
        let mic_id = "{0d1b4c9a-1111-4a2b-9c3d-123456789abc}".to_string();
        let pk = Band { kind: Kind::Peak, freq: 200.0, gain: -3.0, q: 1.0, on: true };
        let curves = HashMap::from([
            (out_id.clone(), Curve { bands: vec![pk.clone()], ..Curve::default() }),
            (mic_id.clone(), Curve { preamp: 4.0, bands: vec![pk], ..Curve::default() }),
        ]);
        let is_input = |id: &str| id == mic_id;
        let mut dsp = poxidsp::Settings::default();
        dsp.gate.on = true;
        dsp.comp.on = true;
        let devices = HashMap::from([(mic_id.clone(), with_dsp(dsp))]);
        let r = |devices: &HashMap<String, DeviceSettings>, bypass: bool, mic_bypass: bool, live: &Live| render(&curves, devices, bypass, &Loudness::default(), mic_bypass, live, is_input);
        let live = Live { studio: Studio { listen: Listen::Mono, ..Studio::default() }, ..Live::default() };
        let text = r(&devices, false, false, &live);
        let chunk = poxidsp::base64(&dsp.chunk(poxidsp::device_key(&mic_id)));
        let section = format!(
            "Device: {mic_id}\r\nStage: capture\r\nPreamp: 4.0 dB\r\nFilter: ON PK Fc 200.0 Hz Gain -3.0 dB Q 1.000\r\nChannel: all\r\nVSTPlugin: Library \"PoxiDSP.dll\" ChunkData \"{chunk}\"\r\n"
        );
        assert!(text.contains(&section), "{text}");
        // Las herramientas de estudio (antes de cualquier `Device:`) no llegan al micrófono
        let studio_at = text.find("Copy:").unwrap();
        assert!(text[..studio_at].contains("Stage: post-mix\r\n"));
        // La salida no tiene nada de PoxiDSP: ni se carga
        assert!(text.contains(&format!("Device: {out_id}\r\nPreamp: 0.0 dB\r\nFilter: ON PK Fc 200.0 Hz Gain -3.0 dB Q 1.000\r\n\r\nDevice: {mic_id}")));
        assert_eq!(text.matches("VSTPlugin").count(), 1);

        // Ni el bypass, ni el A/B ni el test de audición le quitan la puerta al micrófono
        for (bypass, live) in [(true, Live::default()), (false, Live { compare: Some(-1.0), ..Live::default() }), (false, Live { hearing: true, ..Live::default() })] {
            assert!(r(&devices, bypass, false, &live).contains(&section));
        }
        // Su propio bypass sí
        assert!(!r(&devices, false, true, &Live::default()).contains(&mic_id));
        // Con la puerta y el compresor apagados el plugin ni se carga
        let plain = r(&HashMap::new(), false, false, &Live::default());
        assert!(plain.contains(&mic_id) && !plain.contains("VSTPlugin"));
        // La medición de sala lo deja todo en plano, micrófono incluido
        let m = r(&devices, false, false, &Live { measuring: true, ..live });
        assert!(!m.contains("Device") && !m.contains("Copy"));
    }

    #[test]
    fn cada_dispositivo_con_su_poxidsp() {
        let a = "{00000000-0000-0000-0000-00000000000a}".to_string();
        let b = "{00000000-0000-0000-0000-00000000000b}".to_string();
        let c = "{00000000-0000-0000-0000-00000000000c}".to_string();
        let d = "{00000000-0000-0000-0000-00000000000d}".to_string();
        let mut night = poxidsp::Settings::default();
        night.leveler.on = true;
        night.leveler.target = -23.0;
        let mut phones = poxidsp::Settings::default();
        phones.crossfeed.on = true;
        // A: con curva y nivelador; B: sin curva, solo crossfeed; C: con curva y nada encendido; D: nada de nada
        let curves = HashMap::from([(a.clone(), Curve { preamp: -2.0, ..Curve::default() }), (c.clone(), Curve::default())]);
        let devices = HashMap::from([(a.clone(), with_dsp(night)), (b.clone(), with_dsp(phones)), (c.clone(), DeviceSettings::default()), (d.clone(), DeviceSettings::default())]);
        let text = render(&curves, &devices, false, &Loudness::default(), false, &Live::default(), |_| false);
        let plugin = |dsp: &poxidsp::Settings, id: &str| {
            format!("Channel: L R\r\nVSTPlugin: Library \"PoxiDSP.dll\" ChunkData \"{}\"\r\nChannel: all\r\n", poxidsp::base64(&dsp.chunk(poxidsp::device_key(id))))
        };
        assert!(text.contains(&format!("Device: {a}\r\nPreamp: -2.0 dB\r\n{}", plugin(&night, &a))), "{text}");
        assert!(text.contains(&format!("Device: {b}\r\nPreamp: 0.0 dB\r\n{}", plugin(&phones, &b))), "{text}");
        // C sin plugin y D ni aparece
        assert!(text.contains(&format!("Device: {c}\r\nPreamp: 0.0 dB\r\n")) && !text.contains(&d));
        assert_eq!(text.matches("VSTPlugin").count(), 2);
        // Cada sección lleva sus ajustes (no los de otra salida)
        let (sa, sb) = (text.find(&a).unwrap(), text.find(&b).unwrap());
        assert!(sa < sb && text[sa..sb].contains(&plugin(&night, &a)) && !text[sa..sb].contains(&plugin(&phones, &b)));
        // En bypass ninguna salida suena procesada
        assert!(!render(&curves, &devices, true, &Loudness::default(), false, &Live::default(), |_| false).contains("VSTPlugin"));
    }

    #[test]
    fn presets_de_seccion_validados() {
        let mut dsp = poxidsp::Settings::default();
        (dsp.comp.on, dsp.comp.ratio, dsp.crossfeed.on, dsp.gate.on) = (true, 99.0, true, true);
        let p = |id: &str, name: &str| SectionPreset { id: id.into(), name: name.into(), dsp: Some(dsp), curve: Some(Curve::default()), studio: Some(Studio::default()) };
        let mut presets = SectionPresets {
            sound: vec![p("u:a", "  Noche\u{7} "), p("u:a", "Repetido"), p("b:flat", "De serie"), p("u:mal id", "X"), p("u:b", "   "), SectionPreset { dsp: None, ..p("u:c", "Sin ajustes") }],
            phones: (0..150).map(|i| p(&format!("u:{i}"), "P")).collect(),
            studio: vec![p("u:s", "Mono"), SectionPreset { studio: None, ..p("u:t", "Vacío") }],
            mic: vec![p("u:m", "Voz"), SectionPreset { curve: None, ..p("u:n", "Sin EQ") }],
        };
        presets.sanitize();
        // Solo el primero de Sonido es válido: nombre limpio, rangos recortados y solo sus partes
        assert_eq!(presets.sound.len(), 1);
        let s = &presets.sound[0];
        assert_eq!(s.name, "Noche");
        let sd = s.dsp.unwrap();
        assert!(sd.comp.on && sd.comp.ratio == 20.0 && !sd.crossfeed.on && !sd.gate.on);
        assert!(s.curve.is_none() && s.studio.is_none());
        // Cascos: como mucho 100 y solo crossfeed/altavoces
        assert_eq!(presets.phones.len(), MAX_SECTION_PRESETS);
        let pd = presets.phones[0].dsp.unwrap();
        assert!(pd.crossfeed.on && !pd.comp.on);
        // Estudio solo con su escucha; micrófono con EQ, puerta y compresor
        assert_eq!(presets.studio.len(), 1);
        assert!(presets.studio[0].dsp.is_none() && presets.studio[0].curve.is_none());
        assert_eq!(presets.mic.len(), 1);
        let md = presets.mic[0].dsp.unwrap();
        assert!(md.gate.on && md.comp.on && !md.crossfeed.on && presets.mic[0].curve.is_some());

        // El último preset de cada dispositivo solo si existe
        let picks = Picks { sound: Some("u:a".into()), phones: Some("u:borrado".into()), mic: None };
        let mut devices = HashMap::from([
            ("{00000000-0000-0000-0000-00000000000a}".to_string(), DeviceSettings { dsp, presets: picks }),
            ("../x".to_string(), DeviceSettings::default()),
        ]);
        sanitize_devices(&mut devices, &presets);
        assert_eq!(devices.len(), 1);
        let d = &devices["{00000000-0000-0000-0000-00000000000a}"];
        assert_eq!((d.presets.sound.as_deref(), d.presets.phones.as_deref()), (Some("u:a"), None));
        assert_eq!(d.dsp.comp.ratio, 20.0);
    }

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
            ears: Some(Ears { left: vec![Band { kind: Kind::Peak, freq: 1e9, gain: -50.0, q: 1.0, on: true }], right: vec![] }),
        };
        c.sanitize();
        assert_eq!(c.ears.as_ref().unwrap().left[0].freq, 20_000.0);
        assert_eq!(c.ears.as_ref().unwrap().left[0].gain, -24.0);
        c.ears = None;
        assert_eq!(c.preamp, -30.0);
        assert_eq!(c.bands[0].freq, 20.0);
        assert_eq!(c.bands[1].q, 1.0);

        let id = "{6af377fe-6370-495b-afe3-95fac2e918f7}".to_string();
        let mut curves = HashMap::from([(id.clone(), c), ("Device: all\r\nPreamp: 20 dB".into(), Curve::default())]);
        let off = Loudness::default();
        let none = poxidsp::Settings::default();
        let live = Live::default();
        let text = out(&curves, false, &off, &none, &live);
        assert!(text.contains(&format!("Device: {id}\r\nPreamp: -30.0 dB")));
        assert!(text.contains("Filter: ON PK Fc 20.0 Hz Gain 3.0 dB Q 1.410"));
        assert!(text.contains("Filter: ON HPQ Fc 30.0 Hz Q 1.000"));
        assert!(!text.contains("LSC"), "las bandas apagadas no se escriben");
        assert!(!text.contains("20 dB"), "un id que no es GUID no se cuela en el archivo");
        assert!(!text.contains("Loudness"));
        assert!(!text.contains("VSTPlugin"), "con la dinámica apagada el plugin ni se carga");
        // «Graves» con la rueda a 0 tampoco lo carga; en cuanto sube, sí
        let mut boost = none;
        boost.bass_boost = poxidsp::BassBoost { on: true, amount: 0.0 };
        assert!(!out(&curves, false, &off, &boost, &live).contains("VSTPlugin"));
        boost.bass_boost.amount = 0.3;
        assert!(out(&curves, false, &off, &boost, &live).contains("VSTPlugin"));
        let on = Loudness { on: true, offset: -6.0, strength: 0.5 };
        assert!(out(&curves, false, &on, &none, &live).contains("LoudnessCorrection: State 1 ReferenceLevel 0 ReferenceOffset -6 Attenuation 0.50"));
        let mut dsp = none;
        dsp.limiter.on = true;
        let text = out(&curves, false, &on, &dsp, &live);
        let chunk = poxidsp::base64(&dsp.chunk(poxidsp::device_key(&id)));
        assert!(text.ends_with(&format!("Filter: ON HPQ Fc 30.0 Hz Q 1.000\r\nChannel: L R\r\nVSTPlugin: Library \"PoxiDSP.dll\" ChunkData \"{chunk}\"\r\nChannel: all\r\n")));
        let ab = out(&curves, false, &on, &dsp, &Live { compare: Some(-2.5), ..live });
        assert!(ab.contains("Preamp: -2.5 dB") && !ab.contains("Filter") && !ab.contains("Loudness") && !ab.contains("VSTPlugin"));

        assert!(!text.contains("Copy") && !text.contains("estudio"), "sin herramientas de estudio no se escribe nada");

        curves.clear();
        assert!(!out(&curves, true, &off, &dsp, &live).contains("Device"));
    }

    #[test]
    fn monitorizacion_exacta() {
        let st = |listen, flip_l, flip_r, swap| Studio { listen, flip_l, flip_r, swap, sim: None };
        let apply = |s: Studio, l: f64, r: f64| {
            let m = s.matrix();
            (m[0][0] * l + m[0][1] * r, m[1][0] * l + m[1][1] * r)
        };
        // Estéreo sin nada: identidad y ni una línea (bit a bit lo mismo)
        assert_eq!(st(Listen::Stereo, false, false, false).render(), "");
        assert_eq!(apply(st(Listen::Mono, false, false, false), 1.0, 0.0), (0.5, 0.5));
        assert_eq!(apply(st(Listen::Mono, false, false, false), 0.3, 0.3), (0.3, 0.3), "lo mono no cambia de nivel");
        assert_eq!(apply(st(Listen::Side, false, false, false), 0.3, 0.3), (0.0, 0.0), "lo mono desaparece en el side");
        assert_eq!(apply(st(Listen::Side, false, false, false), 1.0, -1.0), (1.0, 1.0));
        assert_eq!(apply(st(Listen::Left, false, false, false), 0.2, 0.7), (0.2, 0.2));
        assert_eq!(apply(st(Listen::Right, false, false, false), 0.2, 0.7), (0.7, 0.7));
        assert_eq!(apply(st(Listen::Stereo, true, false, false), 0.2, 0.7), (-0.2, 0.7));
        assert_eq!(apply(st(Listen::Stereo, false, true, false), 0.2, 0.7), (0.2, -0.7));
        assert_eq!(apply(st(Listen::Stereo, false, false, true), 0.2, 0.7), (0.7, 0.2));
        // Con un canal invertido, lo que era mono se anula al sumar en mono (prueba de fase)
        assert_eq!(apply(st(Listen::Mono, true, false, false), 0.3, 0.3), (0.0, 0.0));
        // Intercambio + solo L: suena lo que va por la izquierda (el canal R original)
        assert_eq!(apply(st(Listen::Left, false, false, true), 0.2, 0.7), (0.7, 0.7));

        // Líneas del motor
        assert_eq!(copy_line(st(Listen::Mono, false, false, false).matrix()), "Copy: L=0.5*L+0.5*R R=0.5*L+0.5*R\r\n");
        assert_eq!(copy_line(st(Listen::Side, false, false, false).matrix()), "Copy: L=0.5*L+-0.5*R R=0.5*L+-0.5*R\r\n");
        assert_eq!(copy_line(st(Listen::Stereo, true, false, true).matrix()), "Copy: L=1*R R=-1*L\r\n");
        assert_eq!(copy_line(st(Listen::Left, false, false, false).matrix()), "Copy: L=1*L R=1*L\r\n");

        // Delante de todo, también en bypass; el test de audición lo quita todo
        let curves = HashMap::from([("{6af377fe-6370-495b-afe3-95fac2e918f7}".to_string(), Curve::default())]);
        let mut dsp = poxidsp::Settings::default();
        dsp.limiter.on = true;
        let live = Live { studio: st(Listen::Mono, false, false, false), ..Live::default() };
        let text = out(&curves, false, &Loudness::default(), &dsp, &live);
        assert!(text.find("Copy:").unwrap() < text.find("Device:").unwrap());
        assert!(out(&curves, true, &Loudness::default(), &dsp, &live).contains("Copy: L=0.5*L"));
        let test = out(&curves, false, &Loudness::default(), &dsp, &Live { hearing: true, ..live });
        assert!(!test.contains("Copy") && !test.contains("Device") && !test.contains("VSTPlugin"));
    }

    #[test]
    fn simulaciones_nunca_suben_el_nivel() {
        let freqs: Vec<f64> = (0..800).map(|i| 20.0 * 1000f64.powf(i as f64 / 799.0)).collect();
        for (sim, c) in &SIMS {
            let db = |f: f64| c.preamp + c.bands.iter().map(|b| band_db(b, f)).sum::<f64>();
            let peak = freqs.iter().map(|&f| db(f)).fold(f64::MIN, f64::max);
            assert!((-0.15..=0.0).contains(&peak), "{sim:?}: pico {peak:.2} dB");
            assert!((0.0..=1.0).contains(&c.width));
        }
        // El móvil y el altavoz son mono, y del móvil no salen graves
        let phone = Sim::Phone.curve();
        assert!(phone.preamp + phone.bands.iter().map(|b| band_db(b, 100.0)).sum::<f64>() < -40.0);
        let text = Studio { sim: Some(Sim::Phone), ..Studio::default() }.render();
        assert!(text.contains("Copy: L=0.5*L+0.5*R R=0.5*L+0.5*R\r\nPreamp: -5.1 dB\r\nFilter: ON HPQ Fc 380.0 Hz Q 0.541"));
        // Unos auriculares no mezclan los canales
        assert!(!Studio { sim: Some(Sim::Earbuds), ..Studio::default() }.render().contains("Copy"));
    }

    #[test]
    fn ajuste_por_oido() {
        let id = "{6af377fe-6370-495b-afe3-95fac2e918f7}".to_string();
        let pk = |freq, gain| Band { kind: Kind::Peak, freq, gain, q: 1.41, on: true };
        let curve = Curve {
            bands: vec![pk(4000.0, 3.0)],
            ears: Some(Ears { left: vec![pk(4000.0, -1.5)], right: vec![] }),
            ..Curve::default()
        };
        let curves = HashMap::from([(id, curve)]);
        let mut dsp = poxidsp::Settings::default();
        dsp.limiter.on = true;
        let text = out(&curves, false, &Loudness::default(), &dsp, &Live::default());
        assert!(text.contains(
            "Filter: ON PK Fc 4000.0 Hz Gain 3.0 dB Q 1.410\r\nChannel: L\r\nFilter: ON PK Fc 4000.0 Hz Gain -1.5 dB Q 1.410\r\nChannel: R\r\nChannel: all\r\nChannel: L R\r\nVSTPlugin"
        ));
        // En el A/B suena el original: sin el ajuste
        assert!(!out(&curves, false, &Loudness::default(), &dsp, &Live { compare: Some(0.0), ..Live::default() }).contains("Channel"));
    }
}
