//! Convolución: respuestas al impulso del usuario (BRIR de HeSuVi o Impulcifer, FIR de REW…). Se valida el WAV,
//! se copia a `config\poxieq-ir\` (la carpeta del motor, que lo lee desde audiodg) y su sección escribe
//! `Convolution:` después de la curva. Aquí también el selector de archivos nativo de Windows.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Como mucho 10 s y 8 canales (lo que se puede convolucionar en tiempo real sin pasarse)
const MAX_SECONDS: f64 = 10.0;
const MAX_CHANNELS: u16 = 8;
/// Puntos de la respuesta que se dibuja (1/12 de octava de 20 Hz a 20 kHz)
pub const POINTS: usize = 120;
pub const DIR: &str = "poxieq-ir";

/// Respuesta al impulso de un dispositivo (se guarda en su configuración)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ir {
    /// Nombre del archivo dentro de `config\poxieq-ir\`
    pub file: String,
    /// Nombre original (para la interfaz)
    pub name: String,
    pub on: bool,
    pub channels: u16,
    pub rate: u32,
    pub seconds: f64,
    /// Retardo hasta el pico (ms)
    pub latency: f64,
    /// Respuesta en dB del primer canal, `POINTS` puntos de 20 Hz a 20 kHz
    pub response: Vec<f32>,
}

impl Ir {
    /// Nombre de archivo nuestro (sin rutas) y valores dentro de lo razonable
    pub fn valid(&mut self) -> bool {
        self.name = self.name.chars().filter(|c| !c.is_control()).take(80).collect();
        self.response.truncate(POINTS);
        self.response.iter_mut().for_each(|v| *v = if v.is_finite() { v.clamp(-120.0, 60.0) } else { 0.0 });
        valid_file(&self.file) && (1..=MAX_CHANNELS).contains(&self.channels) && self.latency.is_finite() && self.seconds.is_finite()
    }
}

fn valid_file(f: &str) -> bool {
    (5..=120).contains(&f.len()) && f.ends_with(".wav") && f[..f.len() - 4].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Línea del motor para la sección de un dispositivo
pub fn line(ir: &Ir) -> String {
    format!("Convolution: {DIR}\\{}\r\n", ir.file)
}

struct Wav {
    channels: u16,
    rate: u32,
    /// Muestras intercaladas, en ±1
    data: Vec<f32>,
}

/// Lee un WAV PCM (16/24/32 bits) o flotante (32/64 bits), también WAVE_FORMAT_EXTENSIBLE
fn read_wav(bytes: &[u8]) -> Result<Wav, &'static str> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("ir.notWav");
    }
    let (mut fmt, mut data) = (None, None);
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap_or_default()) as usize;
        let body = bytes.get(i + 8..(i + 8 + len).min(bytes.len())).ok_or("ir.notWav")?;
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let (fmt, data) = (fmt.ok_or("ir.notWav")?, data.ok_or("ir.notWav")?);
    if fmt.len() < 16 {
        return Err("ir.notWav");
    }
    let u16_at = |o: usize| u16::from_le_bytes([fmt[o], fmt[o + 1]]);
    let mut tag = u16_at(0);
    let channels = u16_at(2);
    let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap_or_default());
    let bits = u16_at(14);
    if tag == 0xFFFE && fmt.len() >= 26 {
        tag = u16_at(24);
    }
    if channels == 0 || !(8000..=384_000).contains(&rate) {
        return Err("ir.notWav");
    }
    let samples: Vec<f32> = match (tag, bits) {
        (1, 16) => data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect(),
        (1, 24) => data.chunks_exact(3).map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0).collect(),
        (1, 32) => data.chunks_exact(4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0).collect(),
        (3, 32) => data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        (3, 64) => data.chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap_or_default()) as f32).collect(),
        _ => return Err("ir.format"),
    };
    Ok(Wav { channels, rate, data: samples })
}

/// Valida un WAV y saca lo que se enseña: canales, duración, retardo hasta el pico y respuesta del primer canal
fn analyze(wav: &Wav) -> Result<(f64, f64, Vec<f32>), &'static str> {
    if wav.channels > MAX_CHANNELS {
        return Err("ir.channels");
    }
    let frames = wav.data.len() / wav.channels as usize;
    let seconds = frames as f64 / wav.rate as f64;
    if frames == 0 || seconds > MAX_SECONDS {
        return Err("ir.length");
    }
    if wav.data.iter().any(|v| !v.is_finite()) {
        return Err("ir.notWav");
    }
    let first: Vec<f64> = wav.data.iter().step_by(wav.channels as usize).map(|&v| v as f64).collect();
    let peak = first.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map_or(0, |(i, _)| i);
    // DFT directa en los puntos que se dibujan (con los primeros 0,7 s basta para la curva)
    let n = first.len().min((wav.rate as f64 * 0.7) as usize);
    let response = (0..POINTS)
        .map(|k| {
            let f = 20.0 * 1000f64.powf(k as f64 / (POINTS - 1) as f64);
            let w = 2.0 * std::f64::consts::PI * f / wav.rate as f64;
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &x) in first[..n].iter().enumerate() {
                re += x * (w * i as f64).cos();
                im -= x * (w * i as f64).sin();
            }
            (20.0 * (re.hypot(im) + 1e-12).log10()) as f32
        })
        .collect();
    Ok((seconds, peak as f64 * 1000.0 / wav.rate as f64, response))
}

/// Carga un WAV del usuario: lo valida, lo copia a la carpeta del motor con un nombre nuestro y devuelve sus datos
pub fn load(path: &Path, config_dir: &Path) -> Result<Ir, &'static str> {
    let bytes = std::fs::read(path).map_err(|_| "ir.read")?;
    if bytes.len() > 200 << 20 {
        return Err("ir.length");
    }
    let wav = read_wav(&bytes)?;
    let (seconds, latency, response) = analyze(&wav)?;
    let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(60).collect();
    let hash = bytes.iter().fold(0x811c_9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193));
    let file = format!("{}-{hash:08x}.wav", if stem.is_empty() { "ir" } else { &stem });
    let dir = config_dir.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|_| "ir.write")?;
    std::fs::write(dir.join(&file), &bytes).map_err(|_| "ir.write")?;
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Ir { file, name, on: true, channels: wav.channels, rate: wav.rate, seconds, latency, response })
}

/// Borra de la carpeta del motor los WAV que ya no usa ningún dispositivo
pub fn clean(config_dir: &Path, used: &[&str]) {
    for e in std::fs::read_dir(config_dir.join(DIR)).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if valid_file(&name) && !used.contains(&name.as_str()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// ------------------------------------------------------------ selector de archivos

/// Diálogo «Abrir» de Windows. `filters`: (nombre, patrón) como ("WAV", "*.wav"). Bloquea hasta que se cierra.
pub fn pick_file(owner: Option<isize>, filters: &[(&str, &str)]) -> Option<PathBuf> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::*;
    let filters: Vec<(HSTRING, HSTRING)> = filters.iter().map(|(n, p)| (HSTRING::from(*n), HSTRING::from(*p))).collect();
    // En un hilo propio en modo apartamento (lo que pide el diálogo)
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let specs: Vec<COMDLG_FILTERSPEC> = filters.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: windows::core::PCWSTR(n.as_ptr()), pszSpec: windows::core::PCWSTR(p.as_ptr()) }).collect();
        let _ = dialog.SetFileTypes(&specs);
        dialog.Show(owner.map(|h| HWND(h as _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        path.map(PathBuf::from)
    })
    .join()
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, samples: &[f32]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4 * channels as u32).to_le_bytes());
        b.extend_from_slice(&(4 * channels).to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    #[test]
    fn impulso_plano_con_su_retardo() {
        // Delta a los 48 muestras (1 ms) en estéreo: respuesta plana a 0 dB
        let mut s = vec![0.0f32; 2 * 4800];
        s[96] = 1.0;
        s[97] = 1.0;
        let w = read_wav(&wav(2, 48000, &s)).unwrap();
        let (secs, lat, resp) = analyze(&w).unwrap();
        assert!((secs - 0.1).abs() < 1e-9 && (lat - 1.0).abs() < 1e-9);
        assert!(resp.iter().all(|v| v.abs() < 0.01), "{resp:?}");
    }

    #[test]
    fn rechaza_lo_que_no_vale() {
        assert_eq!(read_wav(b"nada").err(), Some("ir.notWav"));
        let nine = wav(9, 48000, &[0.0; 9]);
        assert_eq!(analyze(&read_wav(&nine).unwrap()).err(), Some("ir.channels"));
        let long = wav(1, 8000, &vec![0.0; 8000 * 11]);
        assert_eq!(analyze(&read_wav(&long).unwrap()).err(), Some("ir.length"));
        assert!(valid_file("sala-0a1b2c3d.wav") && !valid_file(r"..\x.wav") && !valid_file("a b.wav"));
    }
}
