//! PoxiDSP: dinámica y realces de PoxiEQ.
//!
//! Se compila como plugin VST 2.4 (`PoxiDSP.dll`) que el motor carga con la línea
//! `VSTPlugin: Library "PoxiDSP.dll" ChunkData "<base64>"` y corre dentro de audiodg.exe.
//! La app usa este mismo crate (como biblioteca, con `serde`) para escribir los ajustes
//! ([`Settings::chunk`]) y leer las medidas de la memoria compartida ([`shm`]).

pub mod dsp;
mod hrir;
pub mod shm;
mod vst;

/// Nombre con el que se instala en `<motor>\VSTPlugins` (el motor resuelve ahí las rutas relativas)
pub const DLL_NAME: &str = "PoxiDSP.dll";

/// Ajustes de todos los procesadores. Lo que llega de fuera (interfaz o ChunkData) pasa siempre por
/// [`Settings::sanitize`]: rangos fijos y nada que no sea un número finito.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Settings {
    pub limiter: Limiter,
    pub comp: Comp,
    pub leveler: Leveler,
    pub bass: Bass,
    pub dialog: Dialog,
    pub crossfeed: Crossfeed,
    pub speakers: Speakers,
    /// Puerta de ruido (micrófono). Va la primera de la cadena.
    pub gate: Gate,
    /// «Graves»: realce de graves con una rueda. `bass` es otra cosa (los armónicos).
    pub bass_boost: BassBoost,
}

/// Limitador brickwall con lookahead
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Limiter {
    pub on: bool,
    /// Techo en dBTP
    pub ceiling: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Comp {
    pub on: bool,
    /// dBFS
    pub threshold: f32,
    pub ratio: f32,
    /// ms
    pub attack: f32,
    /// ms
    pub release: f32,
    /// dB (ancho de la rodilla suave)
    pub knee: f32,
    /// dB
    pub makeup: f32,
}

/// Nivelador lento por sonoridad (LUFS, ponderación K)
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Leveler {
    pub on: bool,
    /// LUFS objetivo
    pub target: f32,
    /// dB máximos de subida (lo que puede levantar algo muy flojo)
    pub max_gain: f32,
}

/// «Armónicos» en la interfaz: graves psicoacústicos (armónicos de la banda grave). Se llama `bass` por
/// compatibilidad con las configuraciones guardadas.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Bass {
    pub on: bool,
    /// Hz: por debajo se generan armónicos
    pub freq: f32,
    /// 0…1: mezcla de armónicos
    pub amount: f32,
}

/// Realce de diálogos: refuerzo de la zona de voz en el canal mid
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Dialog {
    pub on: bool,
    /// dB
    pub amount: f32,
}

/// Crossfeed para auriculares (Bauer, como bs2b)
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Crossfeed {
    pub on: bool,
    /// Hz: corte del paso bajo hacia el otro oído
    pub freq: f32,
    /// dB: nivel del cruce (más = más intenso)
    pub feed: f32,
}

/// Altavoces virtuales para auriculares: el estéreo suena desde dos altavoces delante (±30°) con
/// respuestas de cabeza reales (HRIR del KEMAR del MIT). Excluye el crossfeed (ver [`Settings::sanitize`]).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Speakers {
    pub on: bool,
    /// 0…1: 0 = cada oído oye solo su canal, 1 = altavoces completos
    pub amount: f32,
}

/// Puerta de ruido: por debajo del umbral baja la señal `range` dB (para el ruido de fondo del micrófono)
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct Gate {
    pub on: bool,
    /// dBFS (pico) a partir del cual se abre; cierra 3 dB por debajo (histéresis)
    pub threshold: f32,
    /// dB que baja con la puerta cerrada (atenuación)
    pub range: f32,
    /// ms para abrir
    pub attack: f32,
    /// ms que sigue abierta cuando la señal ya ha bajado
    pub hold: f32,
    /// ms para cerrar
    pub release: f32,
}

/// «Graves»: `y = x + k·LP(x)` por canal, con un paso bajo de 2.º orden a 100 Hz (ver `dsp::LowBoost`)
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "camelCase", default))]
pub struct BassBoost {
    pub on: bool,
    /// 0…1: posición de la rueda (k = 2 · amount). A 0 no se procesa nada.
    pub amount: f32,
}

impl BassBoost {
    /// Encendido y con la rueda por encima de 0: solo entonces suma el paso bajo (y la protección)
    pub fn active(&self) -> bool {
        self.on && self.amount > 0.0
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self { on: false, ceiling: -1.0 }
    }
}
impl Default for Comp {
    fn default() -> Self {
        Self { on: false, threshold: -24.0, ratio: 3.0, attack: 10.0, release: 150.0, knee: 6.0, makeup: 0.0 }
    }
}
impl Default for Leveler {
    fn default() -> Self {
        Self { on: false, target: -14.0, max_gain: 12.0 }
    }
}
impl Default for Bass {
    fn default() -> Self {
        Self { on: false, freq: 100.0, amount: 0.4 }
    }
}
impl Default for Dialog {
    fn default() -> Self {
        Self { on: false, amount: 4.0 }
    }
}
impl Default for Crossfeed {
    fn default() -> Self {
        Self { on: false, freq: 700.0, feed: 4.5 }
    }
}
impl Default for Speakers {
    fn default() -> Self {
        Self { on: false, amount: 1.0 }
    }
}
impl Default for BassBoost {
    fn default() -> Self {
        Self { on: false, amount: 0.5 }
    }
}
impl Default for Gate {
    fn default() -> Self {
        Self { on: false, threshold: -50.0, range: 30.0, attack: 1.0, hold: 150.0, release: 200.0 }
    }
}

fn clamp(v: f32, lo: f32, hi: f32, def: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        def
    }
}

/// Valores en orden fijo: es el formato del ChunkData (versión 1). Lo nuevo va siempre al final: un plugin
/// anterior lee lo suyo e ignora el resto, y uno nuevo completa con los valores por defecto lo que no trae
/// un chunk anterior (como mínimo [`MIN_FIELDS`]).
const FIELDS: usize = 30;
/// Campos de la primera versión publicada del chunk (hasta los altavoces virtuales)
const MIN_FIELDS: usize = 22;
const MAGIC: &[u8; 4] = b"PXD1";

impl Settings {
    pub fn sanitize(&mut self) {
        let d = Settings::default();
        let l = &mut self.limiter;
        l.ceiling = clamp(l.ceiling, -12.0, 0.0, d.limiter.ceiling);
        let c = &mut self.comp;
        c.threshold = clamp(c.threshold, -60.0, 0.0, d.comp.threshold);
        c.ratio = clamp(c.ratio, 1.0, 20.0, d.comp.ratio);
        c.attack = clamp(c.attack, 0.1, 200.0, d.comp.attack);
        c.release = clamp(c.release, 10.0, 2000.0, d.comp.release);
        c.knee = clamp(c.knee, 0.0, 24.0, d.comp.knee);
        c.makeup = clamp(c.makeup, 0.0, 24.0, d.comp.makeup);
        let v = &mut self.leveler;
        v.target = clamp(v.target, -36.0, -6.0, d.leveler.target);
        v.max_gain = clamp(v.max_gain, 0.0, 24.0, d.leveler.max_gain);
        let b = &mut self.bass;
        b.freq = clamp(b.freq, 40.0, 200.0, d.bass.freq);
        b.amount = clamp(b.amount, 0.0, 1.0, d.bass.amount);
        self.dialog.amount = clamp(self.dialog.amount, 0.0, 12.0, d.dialog.amount);
        let x = &mut self.crossfeed;
        x.freq = clamp(x.freq, 300.0, 2000.0, d.crossfeed.freq);
        x.feed = clamp(x.feed, 1.0, 15.0, d.crossfeed.feed);
        self.speakers.amount = clamp(self.speakers.amount, 0.0, 1.0, d.speakers.amount);
        let g = &mut self.gate;
        g.threshold = clamp(g.threshold, -90.0, 0.0, d.gate.threshold);
        g.range = clamp(g.range, 0.0, 90.0, d.gate.range);
        g.attack = clamp(g.attack, 0.1, 50.0, d.gate.attack);
        g.hold = clamp(g.hold, 0.0, 2000.0, d.gate.hold);
        g.release = clamp(g.release, 5.0, 2000.0, d.gate.release);
        self.bass_boost.amount = clamp(self.bass_boost.amount, 0.0, 1.0, d.bass_boost.amount);
        // Crossfeed y altavoces virtuales hacen lo mismo de dos formas: juntos se cruzaría dos veces
        if self.speakers.on {
            self.crossfeed.on = false;
        }
    }

    /// Con todo apagado el plugin ni se carga
    pub fn active(&self) -> bool {
        self.limiter.on || self.comp.on || self.leveler.on || self.bass.on || self.dialog.on || self.crossfeed.on || self.speakers.on || self.gate.on || self.bass_boost.active()
    }

    fn fields(&self) -> [f32; FIELDS] {
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        let (l, c, v, b, d, x, h, g, w) = (self.limiter, self.comp, self.leveler, self.bass, self.dialog, self.crossfeed, self.speakers, self.gate, self.bass_boost);
        [
            on(l.on), l.ceiling,
            on(c.on), c.threshold, c.ratio, c.attack, c.release, c.knee, c.makeup,
            on(v.on), v.target, v.max_gain,
            on(b.on), b.freq, b.amount,
            on(d.on), d.amount,
            on(x.on), x.freq, x.feed,
            on(h.on), h.amount,
            on(g.on), g.threshold, g.range, g.attack, g.hold, g.release,
            on(w.on), w.amount,
        ]
    }

    fn from_fields(f: [f32; FIELDS]) -> Self {
        let on = |v: f32| v >= 0.5;
        let mut s = Settings {
            limiter: Limiter { on: on(f[0]), ceiling: f[1] },
            comp: Comp { on: on(f[2]), threshold: f[3], ratio: f[4], attack: f[5], release: f[6], knee: f[7], makeup: f[8] },
            leveler: Leveler { on: on(f[9]), target: f[10], max_gain: f[11] },
            bass: Bass { on: on(f[12]), freq: f[13], amount: f[14] },
            dialog: Dialog { on: on(f[15]), amount: f[16] },
            crossfeed: Crossfeed { on: on(f[17]), freq: f[18], feed: f[19] },
            speakers: Speakers { on: on(f[20]), amount: f[21] },
            gate: Gate { on: on(f[22]), threshold: f[23], range: f[24], attack: f[25], hold: f[26], release: f[27] },
            bass_boost: BassBoost { on: on(f[28]), amount: f[29] },
        };
        s.sanitize();
        s
    }

    /// ChunkData binario: `PXD1`, clave del dispositivo (u32) y los valores en f32, todo little-endian.
    pub fn chunk(&self, key: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + FIELDS * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&key.to_le_bytes());
        for v in self.fields() {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Lo contrario de [`Settings::chunk`]. Un chunk que no es nuestro da `None`: el plugin se queda en paso
    /// directo. Uno de una versión anterior (más corto) completa lo que le falta con los valores por defecto.
    pub fn from_chunk(data: &[u8]) -> Option<(Self, u32)> {
        if data.len() < 8 + MIN_FIELDS * 4 || &data[..4] != MAGIC {
            return None;
        }
        let word = |i: usize| [data[i], data[i + 1], data[i + 2], data[i + 3]];
        let key = u32::from_le_bytes(word(4));
        let mut f = Settings::default().fields();
        let n = ((data.len() - 8) / 4).min(FIELDS);
        for (i, v) in f.iter_mut().enumerate().take(n) {
            *v = f32::from_le_bytes(word(8 + i * 4));
        }
        Some((Self::from_fields(f), key))
    }
}

/// Identificador corto de un dispositivo (FNV-1a de su GUID en minúsculas) para repartir las medidas
/// por dispositivo en la memoria compartida. Nunca es 0 (0 = hueco libre).
pub fn device_key(id: &str) -> u32 {
    let h = id.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(0x0100_0193));
    h.max(1)
}

/// Base64 estándar con relleno (lo que decodifica el motor con `CryptStringToBinaryW`)
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_ida_y_vuelta() {
        let mut s = Settings::default();
        s.comp.on = true;
        s.comp.ratio = 4.5;
        s.speakers.on = true;
        s.speakers.amount = 0.6;
        s.leveler.target = -23.0;
        let (back, key) = Settings::from_chunk(&s.chunk(0xdead_beef)).unwrap();
        assert_eq!(back, s);
        assert_eq!(key, 0xdead_beef);
        assert!(Settings::from_chunk(b"PXD1").is_none(), "corto");
        assert!(Settings::from_chunk(&[0u8; 200]).is_none(), "sin firma");
    }

    #[test]
    fn chunk_de_una_version_anterior() {
        // Un chunk de 22 campos (antes de la puerta de ruido) se lee igual y la puerta queda por defecto
        let mut s = Settings::default();
        s.limiter.on = true;
        s.speakers.amount = 0.3;
        s.gate.on = true;
        s.gate.threshold = -40.0;
        let full = s.chunk(7);
        let old = &full[..8 + MIN_FIELDS * 4];
        let (back, key) = Settings::from_chunk(old).unwrap();
        assert_eq!(key, 7);
        assert!(back.limiter.on && back.speakers.amount == 0.3);
        assert_eq!(back.gate, Gate::default());
        // Uno más largo (de una versión futura) se lee hasta donde sabemos
        let mut future = full.clone();
        future.extend_from_slice(&1f32.to_le_bytes());
        assert_eq!(Settings::from_chunk(&future).unwrap().0, back_with_gate(&back, s.gate));
        // Ni la mitad de un campo
        assert!(Settings::from_chunk(&full[..8 + MIN_FIELDS * 4 - 1]).is_none());
    }

    #[test]
    fn chunk_sin_los_graves_se_sigue_leyendo() {
        // Un chunk de 28 campos (con la puerta, antes de «Graves») se lee entero y los graves quedan por defecto
        let mut s = Settings::default();
        s.gate.on = true;
        s.gate.hold = 300.0;
        s.bass_boost = BassBoost { on: true, amount: 0.65 };
        let full = s.chunk(9);
        assert_eq!(full.len(), 8 + FIELDS * 4);
        assert_eq!(Settings::from_chunk(&full).unwrap().0, s);
        let (old, _) = Settings::from_chunk(&full[..8 + 28 * 4]).unwrap();
        assert_eq!(old, Settings { bass_boost: BassBoost::default(), ..s });
        assert!(!old.bass_boost.on);
        // Rueda a 0: encendido pero sin nada que procesar (el plugin ni se carga)
        let mut s = Settings { bass_boost: BassBoost { on: true, amount: 0.0 }, ..Settings::default() };
        assert!(!s.active());
        s.bass_boost.amount = 0.01;
        assert!(s.active());
        // Fuera de rango o no finito
        s.bass_boost.amount = 3.0;
        s.sanitize();
        assert_eq!(s.bass_boost.amount, 1.0);
        s.bass_boost.amount = f32::NAN;
        s.sanitize();
        assert_eq!(s.bass_boost.amount, BassBoost::default().amount);
    }

    fn back_with_gate(s: &Settings, gate: Gate) -> Settings {
        Settings { gate, ..*s }
    }

    #[test]
    fn sanitize_no_deja_pasar_nada_raro() {
        let mut c = Settings::default().chunk(1);
        // ratio = NaN, techo = +20 dB
        c[8 + 4 * 4..8 + 5 * 4].copy_from_slice(&f32::NAN.to_le_bytes());
        c[8 + 4..8 + 8].copy_from_slice(&20f32.to_le_bytes());
        let (s, _) = Settings::from_chunk(&c).unwrap();
        assert_eq!(s.comp.ratio, 3.0);
        assert_eq!(s.limiter.ceiling, 0.0);
        assert!(!Settings::default().active());
        // Crossfeed y altavoces virtuales nunca juntos
        let mut s = Settings::default();
        (s.crossfeed.on, s.speakers.on, s.speakers.amount) = (true, true, 7.0);
        s.sanitize();
        assert!(!s.crossfeed.on && s.speakers.on && s.speakers.amount == 1.0);
    }

    #[test]
    fn base64_estandar() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn clave_de_dispositivo() {
        let a = device_key("{6AF377FE-6370-495B-AFE3-95FAC2E918F7}");
        assert_eq!(a, device_key("{6af377fe-6370-495b-afe3-95fac2e918f7}"));
        assert_ne!(a, device_key("{00000000-6370-495b-afe3-95fac2e918f7}"));
        assert_ne!(a, 0);
    }
}
