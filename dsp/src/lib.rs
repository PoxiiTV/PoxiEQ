//! PoxiDSP: dinámica y realces de PoxiEQ.
//!
//! Se compila como plugin VST 2.4 (`PoxiDSP.dll`) que el motor carga con la línea
//! `VSTPlugin: Library "PoxiDSP.dll" ChunkData "<base64>"` y corre dentro de audiodg.exe.
//! La app usa este mismo crate (como biblioteca, con `serde`) para escribir los ajustes
//! ([`Settings::chunk`]) y leer las medidas de la memoria compartida ([`shm`]).

pub mod dsp;
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

/// Realce de graves psicoacústico: armónicos de la banda grave
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

fn clamp(v: f32, lo: f32, hi: f32, def: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        def
    }
}

/// Valores en orden fijo: es el formato del ChunkData (versión 1)
const FIELDS: usize = 20;
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
    }

    /// Con todo apagado el plugin ni se carga
    pub fn active(&self) -> bool {
        self.limiter.on || self.comp.on || self.leveler.on || self.bass.on || self.dialog.on || self.crossfeed.on
    }

    fn fields(&self) -> [f32; FIELDS] {
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        let (l, c, v, b, d, x) = (self.limiter, self.comp, self.leveler, self.bass, self.dialog, self.crossfeed);
        [
            on(l.on), l.ceiling,
            on(c.on), c.threshold, c.ratio, c.attack, c.release, c.knee, c.makeup,
            on(v.on), v.target, v.max_gain,
            on(b.on), b.freq, b.amount,
            on(d.on), d.amount,
            on(x.on), x.freq, x.feed,
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

    /// Lo contrario de [`Settings::chunk`]. Un chunk que no es nuestro (o de otra versión) da `None`:
    /// el plugin se queda en paso directo.
    pub fn from_chunk(data: &[u8]) -> Option<(Self, u32)> {
        if data.len() < 8 + FIELDS * 4 || &data[..4] != MAGIC {
            return None;
        }
        let word = |i: usize| [data[i], data[i + 1], data[i + 2], data[i + 3]];
        let key = u32::from_le_bytes(word(4));
        let mut f = [0f32; FIELDS];
        for (i, v) in f.iter_mut().enumerate() {
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
        s.crossfeed.on = true;
        s.leveler.target = -23.0;
        let (back, key) = Settings::from_chunk(&s.chunk(0xdead_beef)).unwrap();
        assert_eq!(back, s);
        assert_eq!(key, 0xdead_beef);
        assert!(Settings::from_chunk(b"PXD1").is_none(), "corto");
        assert!(Settings::from_chunk(&[0u8; 100]).is_none(), "sin firma");
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
