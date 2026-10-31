//! Analizador de espectro: escucha la salida en modo loopback (lo mismo que suena) y manda a la
//! interfaz ~30 veces por segundo el nivel por bandas en escala logarítmica.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::core::HSTRING;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

const N: usize = 4096;
pub const BANDS: usize = 96;

/// Cada arranque sube la generación; el hilo que ve una generación distinta a la suya se para.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn stop() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub fn start(app: AppHandle, device_id: String) {
    let gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = run(&app, &device_id, gen);
    });
}

unsafe fn run(app: &AppHandle, id: &str, gen: u64) -> windows::core::Result<()> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDevice(&HSTRING::from(format!("{{0.0.0.00000000}}.{id}")))?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let fmt = client.GetMixFormat()?;
    let (channels, rate, float) = {
        let f = &*fmt;
        let ext = f.wFormatTag == 0xFFFE && (*(fmt as *const WAVEFORMATEXTENSIBLE)).SubFormat.data1 == 3;
        (f.nChannels as usize, f.nSamplesPerSec as f32, f.wBitsPerSample == 32 && (f.wFormatTag == 3 || ext))
    };
    let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None);
    CoTaskMemFree(Some(fmt as _));
    init?;
    // Límite: el modo compartido entrega float32 en la práctica; otro formato = sin analizador
    if !float || channels == 0 {
        return Ok(());
    }
    let capture: IAudioCaptureClient = client.GetService()?;
    client.Start()?;

    let window: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
    let edges = band_edges(rate);
    let mut ring = vec![0f32; N];
    let mut pos = 0usize;
    let mut last = Instant::now();

    while GENERATION.load(Ordering::SeqCst) == gen {
        std::thread::sleep(Duration::from_millis(10));
        while capture.GetNextPacketSize()? > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * channels);
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            for frame in samples.chunks_exact(channels) {
                ring[pos] = if silent { 0.0 } else { frame.iter().sum::<f32>() / channels as f32 };
                pos = (pos + 1) % N;
            }
            capture.ReleaseBuffer(frames)?;
        }
        if last.elapsed() >= Duration::from_millis(33) {
            last = Instant::now();
            let mut re: Vec<f32> = (0..N).map(|i| ring[(pos + i) % N] * window[i]).collect();
            let mut im = vec![0f32; N];
            fft(&mut re, &mut im);
            let _ = app.emit("spectrum", levels(&re, &im, &edges));
        }
    }
    let _ = client.Stop();
    Ok(())
}

/// Límites (en bins de la FFT) de cada banda, repartidas en escala logarítmica de 20 Hz a 20 kHz
fn band_edges(rate: f32) -> Vec<(usize, usize)> {
    let bin = |f: f32| ((f / rate * N as f32).round() as usize).clamp(1, N / 2 - 1);
    (0..BANDS)
        .map(|i| {
            let f = |k: f32| 20.0 * 1000f32.powf(k / BANDS as f32);
            let (a, b) = (bin(f(i as f32)), bin(f(i as f32 + 1.0)));
            (a, b.max(a + 1))
        })
        .collect()
}

/// Nivel en dBFS de cada banda (el bin más fuerte, normalizado a la ventana de Hann)
fn levels(re: &[f32], im: &[f32], edges: &[(usize, usize)]) -> Vec<f32> {
    let norm = 4.0 / N as f32;
    edges
        .iter()
        .map(|&(a, b)| {
            let peak = (a..b).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).fold(0f32, f32::max) * norm;
            (20.0 * peak.max(1e-7).log10()).max(-120.0)
        })
        .collect()
}

/// FFT radix-2 en el sitio (N potencia de 2)
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_seno_cae_en_su_banda_a_su_nivel() {
        let rate = 48000.0;
        let f = 1000.0;
        let window: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
        let mut re: Vec<f32> = (0..N).map(|i| 0.5 * (2.0 * std::f32::consts::PI * f * i as f32 / rate).sin() * window[i]).collect();
        let mut im = vec![0f32; N];
        fft(&mut re, &mut im);
        let edges = band_edges(rate);
        let l = levels(&re, &im, &edges);
        let loudest = l.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let band_freq = 20.0 * 1000f32.powf((loudest as f32 + 0.5) / BANDS as f32);
        assert!((band_freq / f).log2().abs() < 0.2, "banda {loudest} = {band_freq} Hz");
        // Amplitud 0.5 = -6 dBFS (con algo de margen por caer entre bins)
        assert!((l[loudest] + 6.0).abs() < 1.5, "{} dB", l[loudest]);
    }
}
