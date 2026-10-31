//! Corrección de sala para altavoces.
//!
//! Se reproduce un barrido senoidal logarítmico (Farina) por la salida mientras se graba el micrófono de
//! medida (WASAPI en modo compartido: render + captura). La respuesta al impulso sale de dividir los dos
//! espectros (deconvolución por FFT, regularizada fuera del barrido); se ventana, se pasa a una rejilla
//! logarítmica fina, se promedian las posiciones medidas (en potencia) y se suaviza por fracciones de
//! octava. El ajuste coloca hasta 10 campanas, sobre todo de corte (las subidas tienen tope y no se hacen
//! por debajo de donde el altavoz ya no llega), hacia un objetivo plano o con caída hacia agudos.

use crate::analyzer::{fft, float_mix};
use crate::eq::{band_db, Band, Kind};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

/// Nivel del barrido en dBFS (pico): tope por seguridad; el volumen final lo pone además Windows
pub const MIN_LEVEL: f64 = -40.0;
pub const MAX_LEVEL: f64 = -12.0;
/// Duración del barrido para comprobar el nivel y para medir
pub const CHECK_SECS: f64 = 1.5;
pub const MEASURE_SECS: f64 = 5.0;
/// Posiciones que se pueden promediar
pub const MAX_TAKES: usize = 8;

const F1: f64 = 20.0;
const F2: f64 = 20000.0;
/// Silencio antes del barrido (de ahí sale el ruido de fondo) y después (cola de la sala)
const PRE_SECS: f64 = 0.5;
const TAIL_SECS: f64 = 1.0;
/// Rejilla de trabajo: 1/48 de octava de 20 Hz a 20,48 kHz
const PER_OCT: f64 = 48.0;
pub const GRID: usize = 481;
/// Banda de referencia (0 dB): donde suenan bien casi todos los altavoces
const REF_BAND: (f64, f64) = (250.0, 2500.0);
/// Lo más que se corta con un filtro
const MAX_CUT: f64 = 15.0;

pub fn grid_freq(i: usize) -> f64 {
    F1 * 2f64.powf(i as f64 / PER_OCT)
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

// ------------------------------------------------------------------ barrido

/// Barrido senoidal exponencial de `f1` a `f2` en `secs` segundos
#[derive(Clone, Copy, Debug)]
pub struct Sweep {
    pub f1: f64,
    pub f2: f64,
    pub secs: f64,
}

impl Sweep {
    /// El barrido que cabe en todas las frecuencias de muestreo (hasta 0,45·fs: sin aliasing)
    pub fn new(secs: f64, rates: &[f64]) -> Self {
        Self { f1: F1, f2: rates.iter().fold(F2, |f, r| f.min(0.45 * r)), secs }
    }

    /// Muestras a `fs` con pico `amp`. Fundido de 50 ms al empezar y 5 ms al acabar (sin clics); como la
    /// frecuencia sube en exponencial, el del principio solo toca 20–21 Hz.
    pub fn samples(&self, fs: f64, amp: f64) -> Vec<f64> {
        let n = (self.secs * fs) as usize;
        let l = self.secs / (self.f2 / self.f1).ln();
        let (fin, fout) = ((0.05 * fs) as usize, (0.005 * fs) as usize);
        let hann = |i: usize, len: usize| 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / len as f64).cos();
        (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let mut v = (2.0 * std::f64::consts::PI * self.f1 * l * ((t / l).exp() - 1.0)).sin();
                if i < fin {
                    v *= hann(i, fin);
                }
                if n - i <= fout {
                    v *= hann(n - i, fout);
                }
                amp * v
            })
            .collect()
    }
}

// --------------------------------------------------------------- análisis

fn padded(x: &[f64], n: usize) -> Vec<f64> {
    let mut v = vec![0.0; n];
    v[..x.len()].copy_from_slice(x);
    v
}

/// Respuesta al impulso: espectro de la grabación entre el del barrido, regularizado (dentro del barrido casi
/// nada, fuera mucho: ahí no hay señal que dividir)
pub fn impulse(rec: &[f64], reference: &[f64], fs: f64, sweep: &Sweep) -> Vec<f64> {
    let n = (rec.len() + reference.len()).next_power_of_two();
    let (mut rr, mut ri) = (padded(rec, n), vec![0.0; n]);
    let (mut xr, mut xi) = (padded(reference, n), vec![0.0; n]);
    fft(&mut rr, &mut ri, false);
    fft(&mut xr, &mut xi, false);
    let max = xr.iter().zip(&xi).map(|(a, b)| a * a + b * b).fold(0.0, f64::max);
    for k in 0..n {
        let f = k.min(n - k) as f64 * fs / n as f64;
        let eps = max * if f >= sweep.f1 && f <= sweep.f2 { 1e-7 } else { 1e-2 };
        let p = xr[k] * xr[k] + xi[k] * xi[k] + eps;
        // R · conj(X) / (|X|² + eps)
        let (re, im) = (rr[k] * xr[k] + ri[k] * xi[k], ri[k] * xr[k] - rr[k] * xi[k]);
        rr[k] = re / p;
        ri[k] = im / p;
    }
    fft(&mut rr, &mut ri, true);
    rr
}

/// Potencia de la respuesta en la rejilla. Ventana desde 2 ms antes del pico hasta 500 ms después (con
/// los últimos 100 ms en fundido): entran los modos de la sala, que es lo que se quiere corregir.
pub fn response(h: &[f64], fs: f64) -> Vec<f64> {
    let peak = h.iter().enumerate().fold((0, 0.0), |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m }).0;
    let (pre, len, fade) = ((0.002 * fs) as usize, (0.5 * fs) as usize, (0.1 * fs) as usize);
    let start = peak.saturating_sub(pre);
    let total = pre + len;
    let hann = |i: usize, w: usize| 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / w as f64).cos();
    let seg: Vec<f64> = (0..total)
        .map(|i| {
            let v = h.get(start + i).copied().unwrap_or(0.0);
            let w = if i < pre { hann(i, pre) } else if total - i <= fade { hann(total - i, fade) } else { 1.0 };
            v * w
        })
        .collect();
    let m = total.max((2.0 * fs) as usize).next_power_of_two();
    let (mut re, mut im) = (padded(&seg, m), vec![0.0; m]);
    fft(&mut re, &mut im, false);
    let bin = fs / m as f64;
    let power = |k: usize| re[k] * re[k] + im[k] * im[k];
    (0..GRID)
        .map(|i| {
            let f = grid_freq(i);
            let (lo, hi) = ((f * 2f64.powf(-0.5 / PER_OCT) / bin).ceil() as usize, (f * 2f64.powf(0.5 / PER_OCT) / bin).floor() as usize);
            let hi = hi.min(m / 2);
            if lo <= hi {
                (lo..=hi).map(power).sum::<f64>() / (hi - lo + 1) as f64
            } else {
                // Más estrecho que un bin (graves): interpolación entre los dos vecinos
                let x = f / bin;
                let k = (x.floor() as usize).min(m / 2 - 1);
                let t = x - k as f64;
                power(k) * (1.0 - t) + power(k + 1) * t
            }
        })
        .collect()
}

/// Media móvil en la rejilla logarítmica: suavizado de `octaves` de ancho (1/3, 1/6…), sobre potencia
pub fn smooth(p: &[f64], octaves: f64) -> Vec<f64> {
    let half = (octaves * PER_OCT / 2.0).round() as usize;
    let mut acc = vec![0.0; p.len() + 1];
    for (i, v) in p.iter().enumerate() {
        acc[i + 1] = acc[i] + v;
    }
    (0..p.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half).min(p.len() - 1));
            (acc[hi + 1] - acc[lo]) / (hi - lo + 1) as f64
        })
        .collect()
}

/// Media en potencia de varias posiciones
pub fn average(takes: &[Vec<f64>]) -> Vec<f64> {
    (0..GRID).map(|i| takes.iter().map(|t| t[i]).sum::<f64>() / takes.len().max(1) as f64).collect()
}

// ------------------------------------------------------------------ ajuste

/// Lo que se puede elegir en el resultado (todo validado en [`FitParams::sanitize`])
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FitParams {
    /// Ancho del suavizado en octavas (1/3, 1/6…)
    pub smoothing: f64,
    /// Caída del objetivo hacia agudos en dB/octava (0 = plano)
    pub slope: f64,
    /// Hasta dónde se corrige (Hz)
    pub high: f64,
    /// Lo más que puede subir un filtro (dB)
    pub max_boost: f64,
    pub filters: usize,
}

impl FitParams {
    pub fn sanitize(&mut self) {
        let clamp = crate::eq::clamp;
        self.smoothing = clamp(self.smoothing, 1.0 / 24.0, 1.0, 1.0 / 6.0);
        self.slope = clamp(self.slope, -1.5, 0.0, 0.0);
        self.high = clamp(self.high, 150.0, 20000.0, 500.0);
        self.max_boost = clamp(self.max_boost, 0.0, 6.0, 3.0);
        self.filters = self.filters.clamp(1, 10);
    }
}

/// Resultado para pintar y guardar: curvas en dB en la rejilla y los filtros
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Fit {
    pub freqs: Vec<f64>,
    /// Medido y suavizado, 0 dB = media de la banda de referencia
    pub measured: Vec<f64>,
    pub target: Vec<f64>,
    /// Medido + filtros
    pub corrected: Vec<f64>,
    pub bands: Vec<Band>,
    /// Donde el altavoz ya no llega (6 dB por debajo del objetivo): por debajo no se sube nada
    pub low: f64,
}

struct Problem<'a> {
    freqs: &'a [f64],
    measured: &'a [f64],
    target: &'a [f64],
    low: f64,
    high: f64,
    max_boost: f64,
    /// cos ω, cos 2ω, sin ω, sin 2ω de cada punto (a 48 kHz, como `band_db`): iguales para todos los filtros
    trig: Vec<[f64; 4]>,
}

impl Problem<'_> {
    /// Error de un punto con la corrección `c` en él. Por debajo de `low` lo que falta no cuenta (no se puede
    /// subir) pero lo que sobra sí; por encima de `high` solo cuenta lo que cambian los filtros.
    fn error(&self, i: usize, c: f64) -> f64 {
        let f = self.freqs[i];
        let over = self.measured[i] - self.target[i];
        if f > self.high {
            c * std::f64::consts::FRAC_1_SQRT_2
        } else if f < self.low {
            over.max(0.0) + c
        } else {
            over + c
        }
    }

    /// Error cuadrático total con la corrección `total` (suma de los filtros, dB por punto). Pasarse de la
    /// subida máxima con varios filtros juntos cuesta muchísimo.
    fn cost(&self, total: &[f64]) -> f64 {
        total
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let e = self.error(i, c);
                let over = (c - self.max_boost).max(0.0);
                e * e + 100.0 * over * over
            })
            .sum()
    }

    /// Respuesta de una campana en la rejilla: lo mismo que `band_db`, con los senos ya calculados
    fn resp(&self, b: &Band) -> Vec<f64> {
        let a = 10f64.powf(b.gain / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * b.freq / 48000.0;
        let (cos, alpha) = (w0.cos(), w0.sin() / (2.0 * b.q));
        let (num, den) = ([1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a]);
        let mag = |k: &[f64; 3], t: &[f64; 4]| {
            let re = k[0] + k[1] * t[0] + k[2] * t[1];
            let im = k[1] * t[2] + k[2] * t[3];
            re * re + im * im
        };
        self.trig.iter().map(|t| 10.0 * (mag(&num, t) / mag(&den, t)).log10()).collect()
    }
}

/// Límites de un filtro según sea de corte o de subida
#[derive(Clone, Copy)]
struct Limits {
    gain: (f64, f64),
    q: (f64, f64),
    freq: (f64, f64),
}

/// Descenso por coordenadas: frecuencia, ganancia y Q de todos los filtros, con pasos que se van estrechando.
/// Cada filtro guarda su respuesta y `total` la suma, así cada prueba solo recalcula un filtro.
fn refine(p: &Problem, bands: &mut [Band], limits: &[Limits], resp: &mut [Vec<f64>], total: &mut Vec<f64>) {
    let mut best = p.cost(total);
    let (mut fstep, mut gstep, mut qstep) = (2f64.powf(1.0 / 12.0), 1.0, 1.25f64);
    for _ in 0..40 {
        let mut better = false;
        for i in 0..bands.len() {
            for param in 0..3 {
                for dir in [1.0, -1.0] {
                    let mut b = bands[i].clone();
                    let l = limits[i];
                    match param {
                        0 => b.freq = (b.freq * fstep.powf(dir)).clamp(l.freq.0, l.freq.1),
                        1 => b.gain = (b.gain + gstep * dir).clamp(l.gain.0, l.gain.1),
                        _ => b.q = (b.q * qstep.powf(dir)).clamp(l.q.0, l.q.1),
                    }
                    let new = p.resp(&b);
                    let trial: Vec<f64> = total.iter().zip(&resp[i]).zip(&new).map(|((t, o), n)| t - o + n).collect();
                    let c = p.cost(&trial);
                    if c < best - 1e-9 {
                        best = c;
                        (bands[i], resp[i], *total) = (b, new, trial);
                        better = true;
                    }
                }
            }
        }
        if !better {
            if gstep < 0.05 {
                break;
            }
            (fstep, gstep, qstep) = (fstep.sqrt(), gstep / 2.0, qstep.sqrt());
        }
    }
}

/// Ancho a media altura alrededor de `k` (donde `inside` se cumple) → Q de una campana con ese ancho de banda
fn width_q(k: usize, len: usize, inside: impl Fn(usize) -> bool) -> f64 {
    let (mut l, mut r) = (k, k);
    while l > 0 && inside(l - 1) {
        l -= 1;
    }
    while r + 1 < len && inside(r + 1) {
        r += 1;
    }
    let bw = ((r - l + 1) as f64 / PER_OCT).max(1.0 / 12.0);
    2f64.powf(bw / 2.0) / (2f64.powf(bw) - 1.0)
}

/// Ajuste completo sobre la media de las posiciones (potencia en la rejilla)
pub fn fit(power: &[f64], params: &FitParams) -> Fit {
    let freqs: Vec<f64> = (0..GRID).map(grid_freq).collect();
    let to_db = |p: &[f64]| p.iter().map(|v| 10.0 * v.max(1e-30).log10()).collect::<Vec<f64>>();
    let in_ref = |f: f64| f >= REF_BAND.0 && f <= REF_BAND.1;
    let mean_ref = |v: &[f64]| {
        let (s, n) = freqs.iter().zip(v).filter(|(f, _)| in_ref(**f)).fold((0.0, 0), |(s, n), (_, v)| (s + v, n + 1));
        s / n.max(1) as f64
    };
    let raw = to_db(&smooth(power, params.smoothing));
    let m0 = mean_ref(&raw);
    let measured: Vec<f64> = raw.iter().map(|v| v - m0).collect();
    let slope: Vec<f64> = freqs.iter().map(|f| params.slope * (f / 1000.0).log2()).collect();
    let t0 = mean_ref(&slope);
    let target: Vec<f64> = slope.iter().map(|v| v - t0).collect();

    // Caída del altavoz en graves, sobre una versión muy suavizada (una octava: un hueco de la sala no la
    // engaña): bajando desde 200 Hz, hasta que queda 6 dB por debajo del objetivo. No se sube nada hasta media
    // octava por encima: en plena caída, subir solo hace trabajar de más al altavoz.
    let broad: Vec<f64> = to_db(&smooth(power, 1.0)).iter().map(|v| v - m0).collect();
    let mut lo = freqs.iter().position(|&f| f >= 200.0).unwrap_or(0);
    while lo > 0 && broad[lo - 1] - target[lo - 1] > -6.0 {
        lo -= 1;
    }
    let low = freqs[lo] * std::f64::consts::SQRT_2;
    let trig = freqs
        .iter()
        .map(|f| {
            let w = 2.0 * std::f64::consts::PI * f / 48000.0;
            [w.cos(), (2.0 * w).cos(), w.sin(), (2.0 * w).sin()]
        })
        .collect();
    let p = Problem { freqs: &freqs, measured: &measured, target: &target, low, high: params.high, max_boost: params.max_boost, trig };
    let region: Vec<usize> = (0..GRID).filter(|&i| freqs[i] <= params.high).collect();

    let mut bands: Vec<Band> = vec![];
    let mut limits: Vec<Limits> = vec![];
    let mut resp: Vec<Vec<f64>> = vec![];
    let mut total = vec![0.0; GRID];
    // Huecos: lo que falta, de media en media octava, y solo donde también falta en la medida original (nunca
    // se sube para tapar lo que ha bajado un corte). Un hueco estrecho (una cancelación, que subiendo no se
    // arregla) no cuenta: lo es si en esa media octava lo más hondo pasa del doble de la media.
    let half = (PER_OCT / 4.0) as usize;
    let window = |v: &[f64], j: usize| v[j.saturating_sub(half)..=(j + half).min(v.len() - 1)].to_vec();
    let avg = |v: &[f64], j: usize| {
        let w = window(v, j);
        w.iter().sum::<f64>() / w.len() as f64
    };
    let lack: Vec<f64> = region.iter().map(|&i| measured[i] - target[i]).collect();
    let broad_dip = |j: usize| {
        let deepest = window(&lack, j).iter().fold(0.0, |m: f64, v| m.max(-v));
        -avg(&lack, j) > 0.0 && deepest <= -2.0 * avg(&lack, j)
    };
    for _ in 0..params.filters {
        let err: Vec<f64> = region.iter().map(|&i| p.error(i, total[i])).collect();
        let dip = |j: usize| avg(&err, j).max(avg(&lack, j));
        let Some((ci, ce)) = err.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)) else { break };
        let boost = (0..err.len())
            .filter(|&j| freqs[region[j]] >= low && broad_dip(j))
            .map(|j| (j, (-dip(j)).min(params.max_boost - total[region[j]])))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|&(_, d)| d > 0.0);
        // Sobre todo cortes: un hueco pesa la mitad que un pico
        let boost_score = boost.map_or(0.0, |(_, d)| 0.5 * d);
        let cut = ce >= boost_score;
        if ce.max(boost_score) < 1.0 {
            break;
        }
        // Cada filtro se queda a menos de 1/3 de octava de lo que vino a corregir (si no, uno de subida
        // acabaría tapando los hombros de los cortes en vez de su hueco)
        let near = |f: f64, lo: f64| (lo.max(f / 2f64.powf(1.0 / 3.0)), params.high.min(f * 2f64.powf(1.0 / 3.0)));
        let (k, gain, q, lim) = if cut {
            let lim = Limits { gain: (-MAX_CUT, 0.0), q: (0.5, 12.0), freq: near(freqs[region[ci]], F1) };
            (ci, -ce.min(MAX_CUT), width_q(ci, err.len(), |j| err[j] > ce / 2.0), lim)
        } else {
            let (bi, depth) = boost.unwrap_or_default();
            let lim = Limits { gain: (0.0, params.max_boost), q: (0.5, 3.0), freq: near(freqs[region[bi]], low) };
            (bi, depth, width_q(bi, err.len(), |j| -dip(j) > depth / 2.0), lim)
        };
        let b = Band { kind: Kind::Peak, freq: freqs[region[k]], gain, q: q.clamp(lim.q.0, lim.q.1), on: true };
        let r = p.resp(&b);
        total.iter_mut().zip(&r).for_each(|(t, v)| *t += v);
        resp.push(r);
        bands.push(b);
        limits.push(lim);
        // Todos a la vez cada vez: así un corte nuevo no deja hombros que otro filtro tenga que tapar
        refine(&p, &mut bands, &limits, &mut resp, &mut total);
    }

    // Redondeo a lo que enseña la interfaz; fuera lo que no hace nada
    let mut bands: Vec<Band> = bands
        .into_iter()
        .map(|b| Band { freq: (b.freq * 10.0).round() / 10.0, gain: (b.gain * 10.0).round() / 10.0, q: (b.q * 100.0).round() / 100.0, ..b })
        .filter(|b| b.gain.abs() >= 0.2)
        .collect();
    bands.sort_by(|a, b| a.freq.total_cmp(&b.freq));
    let corrected = freqs.iter().zip(&measured).map(|(&f, m)| m + bands.iter().map(|b| band_db(b, f)).sum::<f64>()).collect();
    Fit { freqs, measured, target, corrected, bands, low }
}

// ---------------------------------------------------------------- medición

/// Lo que se ha grabado durante un barrido
pub struct Take {
    pub rate: f64,
    /// El canal del micrófono con más señal
    pub data: Vec<f64>,
    pub sweep: Sweep,
    /// dBFS: ruido de fondo (RMS antes del barrido) y pico de toda la toma
    pub noise: f64,
    pub peak: f64,
}

impl Take {
    /// Respuesta en la rejilla (potencia)
    pub fn analyze(&self) -> Vec<f64> {
        let reference = self.sweep.samples(self.rate, 1.0);
        response(&impulse(&self.data, &reference, self.rate, &self.sweep), self.rate)
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    Cancelled,
    /// El formato de mezcla no es float32
    Format,
    Device(String),
}

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error::Device(e.message())
    }
}

/// Reproduce el barrido por `output` (solo en los dos primeros canales) mientras graba `input`. `level` es el
/// pico en dBFS, con tope [`MAX_LEVEL`]. Bloquea lo que dura (unos segundos); `cancel` lo corta.
pub fn record(output: &str, input: &str, level: f64, secs: f64, cancel: &AtomicBool) -> Result<Take, Error> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let out: IAudioClient = en.GetDevice(&crate::engine::endpoint_id(output, false))?.Activate(CLSCTX_ALL, None)?;
        let inp: IAudioClient = en.GetDevice(&crate::engine::endpoint_id(input, true))?.Activate(CLSCTX_ALL, None)?;
        let (oc, orate) = open(&out)?;
        let (ic, irate) = open(&inp)?;
        let sweep = Sweep::new(secs, &[orate, irate]);
        let amp = 10f64.powf(level.clamp(MIN_LEVEL, MAX_LEVEL) / 20.0);
        let mut play = vec![0.0; (PRE_SECS * orate) as usize];
        play.extend(sweep.samples(orate, amp));
        play.extend(std::iter::repeat_n(0.0, (TAIL_SECS * orate) as usize));

        let render: IAudioRenderClient = out.GetService()?;
        let capture: IAudioCaptureClient = inp.GetService()?;
        let size = out.GetBufferSize()? as usize;
        let mut chans: Vec<Vec<f64>> = vec![Vec::with_capacity(((secs + PRE_SECS + TAIL_SECS + 1.0) * irate) as usize); ic];
        let mut run = || -> Result<(), Error> {
            inp.Start()?;
            let deadline = Instant::now() + Duration::from_secs_f64(secs + PRE_SECS + TAIL_SECS + 5.0);
            let (mut pos, mut started, mut drained) = (0, false, None::<Instant>);
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                if Instant::now() > deadline {
                    return Err(Error::Device("timeout".into()));
                }
                let pad = out.GetCurrentPadding()? as usize;
                let free = size.saturating_sub(pad).min(play.len() - pos);
                if free > 0 {
                    let buf = render.GetBuffer(free as u32)? as *mut f32;
                    let frames = std::slice::from_raw_parts_mut(buf, free * oc);
                    for (frame, v) in frames.chunks_exact_mut(oc).zip(&play[pos..pos + free]) {
                        for (c, s) in frame.iter_mut().enumerate() {
                            *s = if c < 2 { *v as f32 } else { 0.0 };
                        }
                    }
                    render.ReleaseBuffer(free as u32, 0)?;
                    pos += free;
                }
                if !started {
                    out.Start()?;
                    started = true;
                }
                if pos == play.len() && pad == 0 && drained.is_none() {
                    drained = Some(Instant::now());
                }
                while capture.GetNextPacketSize()? > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                    capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                    let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * ic);
                    for frame in samples.chunks_exact(ic) {
                        for (c, s) in frame.iter().enumerate() {
                            chans[c].push(if silent { 0.0 } else { *s as f64 });
                        }
                    }
                    capture.ReleaseBuffer(frames)?;
                }
                // La cola de la sala ya está dentro del silencio final; 300 ms más por la latencia de la entrada
                if drained.is_some_and(|t| t.elapsed() > Duration::from_millis(300)) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let result = run();
        let _ = out.Stop();
        let _ = inp.Stop();
        result?;
        let energy = |c: &Vec<f64>| c.iter().map(|v| v * v).sum::<f64>();
        let data = chans.into_iter().max_by(|a, b| energy(a).total_cmp(&energy(b))).unwrap_or_default();
        let quiet = &data[..data.len().min((0.4 * irate) as usize)];
        let noise = db((quiet.iter().map(|v| v * v).sum::<f64>() / quiet.len().max(1) as f64).sqrt());
        let peak = db(data.iter().fold(0.0, |m, v| m.max(v.abs())));
        Ok(Take { rate: irate, data, sweep, noise, peak })
    }
}

/// Prepara un cliente en modo compartido con su formato de mezcla (float32): (canales, frecuencia)
unsafe fn open(client: &IAudioClient) -> Result<(usize, f64), Error> {
    let fmt = client.GetMixFormat()?;
    let mix = float_mix(fmt);
    let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, fmt, None);
    CoTaskMemFree(Some(fmt as _));
    init?;
    mix.ok_or(Error::Format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const FS: f64 = 48000.0;

    /// Filtra con una banda (biquad RBJ en forma directa II transpuesta, a 48 kHz como `band_db`)
    fn filter(b: &Band, x: &[f64]) -> Vec<f64> {
        let a = 10f64.powf(b.gain / 40.0);
        let w0 = 2.0 * PI * b.freq / FS;
        let (cos, alpha) = (w0.cos(), w0.sin() / (2.0 * b.q));
        let c = [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a];
        let (b0, b1, b2, a1, a2) = (c[0] / c[3], c[1] / c[3], c[2] / c[3], c[4] / c[3], c[5] / c[3]);
        let (mut z1, mut z2) = (0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = b0 * v + z1;
                z1 = b1 * v - a1 * y + z2;
                z2 = b2 * v - a2 * y;
                y
            })
            .collect()
    }

    fn at(grid: &[f64], f: f64) -> f64 {
        let i = ((f / F1).log2() * PER_OCT).round() as usize;
        10.0 * grid[i].log10()
    }

    fn pk(freq: f64, gain: f64, q: f64) -> Band {
        Band { kind: Kind::Peak, freq, gain, q, on: true }
    }

    #[test]
    fn deconvolucion_recupera_un_sistema_conocido() {
        // Sistema: 10 ms de retardo, ganancia 0,5 y una campana de +6 dB en 100 Hz (Q 2); ruido muy bajo
        let sweep = Sweep::new(2.0, &[FS]);
        let x = sweep.samples(FS, 0.3);
        let room = pk(100.0, 6.0, 2.0);
        let mut rec = vec![0.0; 480];
        rec.extend(filter(&room, &[x.clone(), vec![0.0; 48000]].concat()).iter().map(|v| v * 0.5));
        let mut s = 1u64;
        for v in rec.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *v += ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 2e-6;
        }
        let take = Take { rate: FS, data: rec, sweep, noise: -120.0, peak: 0.0 };
        let h = impulse(&take.data, &sweep.samples(FS, 1.0), FS, &sweep);
        let peak = h.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert!((480..=486).contains(&peak), "retardo en la muestra {peak}");
        let grid = take.analyze();
        let ref_db = at(&grid, 2000.0) - band_db(&room, 2000.0);
        for f in [30.0, 50.0, 80.0, 100.0, 130.0, 200.0, 500.0, 1000.0, 5000.0, 12000.0] {
            let got = at(&grid, f) - ref_db;
            let want = band_db(&room, f);
            assert!((got - want).abs() < 0.5, "{f} Hz: {got:.2} dB, debería ser {want:.2}");
        }
        // La amplitud del barrido no cambia nada: 0,5 de ganancia del sistema → −6 dB respecto al barrido de 0,3
        assert!((ref_db - (db(0.5) + db(0.3))).abs() < 0.3, "nivel {ref_db:.2} dB");
    }

    #[test]
    fn el_barrido_no_pasa_de_nyquist() {
        assert_eq!(Sweep::new(5.0, &[48000.0, 44100.0]).f2, 19845.0);
        assert_eq!(Sweep::new(5.0, &[16000.0]).f2, 7200.0);
        let s = Sweep::new(1.0, &[FS]).samples(FS, 0.25);
        assert_eq!(s.len(), 48000);
        assert!(s.iter().all(|v| v.abs() <= 0.25) && s[0] == 0.0 && s[47999].abs() < 1e-3);
    }

    #[test]
    fn suavizado_por_octavas() {
        // Plano sigue plano
        assert!(smooth(&vec![2.0; GRID], 1.0 / 3.0).iter().all(|v| (v - 2.0).abs() < 1e-12));
        // Un pico de un solo punto se reparte en el ancho de 1/3 de octava (16 + 1 puntos)
        let mut spike = vec![1.0; GRID];
        spike[240] = 18.0;
        let s = smooth(&spike, 1.0 / 3.0);
        assert!((s[240] - 2.0).abs() < 1e-12, "{}", s[240]);
        assert_eq!(s[240 + 8], s[240]);
        assert_eq!(s[240 + 9], 1.0);
        // Un hueco estrecho (−20 dB en 1/24 de octava) queda en menos de 3 dB con 1/3 de octava
        let notch: Vec<f64> = (0..GRID).map(|i| if (238..=242).contains(&i) { 0.01 } else { 1.0 }).collect();
        let s = smooth(&notch, 1.0 / 3.0);
        assert!(10.0 * s[240].log10() > -3.0);
        assert!(smooth(&notch, 1.0 / 6.0)[240] < s[240], "1/6 conserva más detalle que 1/3");
        // Media de posiciones en potencia
        assert_eq!(average(&[vec![1.0; GRID], vec![3.0; GRID]])[7], 2.0);
    }

    /// Respuesta de una sala de prueba: modos a 55 y 110 Hz, un hueco ancho en 250 Hz, un hueco estrecho
    /// (cancelación) en 80 Hz y un altavoz bass-reflex que cae a 24 dB/oct por debajo de 45 Hz
    fn room() -> Vec<f64> {
        let hp = |q| Band { kind: Kind::HighPass, freq: 45.0, gain: 0.0, q, on: true };
        let parts = [pk(55.0, 8.0, 4.0), pk(110.0, 6.0, 5.0), pk(250.0, -4.0, 1.5), pk(80.0, -12.0, 8.0), hp(0.541), hp(1.307)];
        (0..GRID).map(|i| 10f64.powf(parts.iter().map(|b| band_db(b, grid_freq(i))).sum::<f64>() / 10.0)).collect()
    }

    fn params() -> FitParams {
        FitParams { smoothing: 1.0 / 6.0, slope: 0.0, high: 500.0, max_boost: 3.0, filters: 10 }
    }

    #[test]
    fn ajuste_corta_los_modos_y_respeta_los_limites() {
        let fit = fit(&room(), &params());
        assert!(!fit.bands.is_empty() && fit.bands.len() <= 10);
        assert!((35.0..60.0).contains(&fit.low), "caída del altavoz en {:.1} Hz", fit.low);
        let cuts = fit.bands.iter().filter(|b| b.gain < 0.0).count();
        assert!(cuts >= fit.bands.len() - cuts, "sobre todo cortes: {:?}", fit.bands);
        for b in &fit.bands {
            assert!(b.gain <= 3.0 && b.gain >= -MAX_CUT, "{b:?}");
            assert!(b.freq <= 500.0 && (b.gain < 0.0 || b.freq >= fit.low), "{b:?}");
            assert!(b.gain <= 0.0 || b.q <= 3.0, "las subidas son anchas: {b:?}");
        }
        // Los dos modos quedan corregidos (nada por encima de +1,5 dB en el rango) y lo que falta, salvo la
        // cancelación de 80 Hz (que no se intenta llenar), queda en menos de 3 dB
        for (i, &f) in fit.freqs.iter().enumerate() {
            let e = fit.corrected[i] - fit.target[i];
            if f >= 1.3 * fit.low && f <= 500.0 {
                assert!(e < 1.5, "{f:.0} Hz: {e:.2} dB");
                assert!(e > -3.0 || (70.0..92.0).contains(&f), "{f:.0} Hz: {e:.2} dB");
            }
            if (75.0..85.0).contains(&f) {
                assert!(fit.corrected[i] - fit.measured[i] < 1.5, "la cancelación no se rellena: {f:.0} Hz");
            }
            // Por encima del rango la corrección casi no se nota
            if f >= 1500.0 {
                assert!((fit.corrected[i] - fit.measured[i]).abs() < 0.5, "{f:.0} Hz");
            }
        }
        let before = fit.freqs.iter().zip(&fit.measured).filter(|(f, _)| (50.0..60.0).contains(*f)).map(|(_, v)| *v).fold(f64::MIN, f64::max);
        assert!(before > 5.0, "el modo de 55 Hz está en la medida: {before:.1}");
    }

    #[test]
    fn sin_subidas_si_no_se_permiten() {
        let fit = fit(&room(), &FitParams { max_boost: 0.0, ..params() });
        assert!(fit.bands.iter().all(|b| b.gain < 0.0), "{:?}", fit.bands);
        // Y el objetivo con caída: más graves que agudos, 0 dB de media en la banda de referencia
        let fit = fit_slope();
        let i100 = fit.freqs.iter().position(|&f| f >= 100.0).unwrap();
        let i5k = fit.freqs.iter().position(|&f| f >= 5000.0).unwrap();
        assert!(fit.target[i100] > 1.5 && fit.target[i5k] < -1.0);
    }

    fn fit_slope() -> Fit {
        fit(&room(), &FitParams { slope: -0.8, high: 20000.0, ..params() })
    }

    #[test]
    fn parametros_raros_se_recortan() {
        let mut p = FitParams { smoothing: f64::NAN, slope: 5.0, high: 1.0, max_boost: 99.0, filters: 0 };
        p.sanitize();
        assert_eq!((p.smoothing, p.slope, p.high, p.max_boost, p.filters), (1.0 / 6.0, 0.0, 150.0, 6.0, 1));
        // Una medida vacía o absurda no revienta el ajuste
        let f = fit(&vec![0.0; GRID], &params());
        assert!(f.corrected.iter().all(|v| v.is_finite()));
    }
}
