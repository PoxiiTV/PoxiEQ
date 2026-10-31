//! Procesadores de PoxiDSP. Todo en f64 y muestra a muestra; la memoria se reserva solo en
//! [`Chain::new`] (fuera del hilo de audio), nunca en [`Chain::tick`].
//!
//! Orden: puerta de ruido → diálogos → graves (rueda) → armónicos → nivelador → compresor → crossfeed o
//! altavoces virtuales → limitador (el del usuario o, con los graves, el de protección).

use crate::Settings;
use std::f64::consts::PI;

fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

fn lin_to_db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// Coeficiente de un filtro de un polo con constante de tiempo `ms`
fn coef(ms: f64, fs: f64) -> f64 {
    (-1.0 / (ms * 0.001 * fs)).exp()
}

// ------------------------------------------------------------------ biquad

/// Biquad en forma directa II transpuesta (fórmulas del Audio EQ Cookbook de RBJ)
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        Self { b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]], a: [a[1] / a[0], a[2] / a[0]], z: [0.0; 2] }
    }

    /// (cos w, alpha) con la frecuencia por debajo de Nyquist aunque el formato sea raro
    fn w(fs: f64, f: f64, q: f64) -> (f64, f64) {
        let w = 2.0 * PI * f.min(0.45 * fs) / fs;
        (w.cos(), w.sin() / (2.0 * q))
    }

    fn lowpass(fs: f64, f: f64, q: f64) -> Self {
        let (c, al) = Self::w(fs, f, q);
        Self::new([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    fn highpass(fs: f64, f: f64, q: f64) -> Self {
        let (c, al) = Self::w(fs, f, q);
        Self::new([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    fn peak(fs: f64, f: f64, q: f64, db: f64) -> Self {
        let (c, al) = Self::w(fs, f, q);
        let a = 10f64.powf(db / 40.0);
        Self::new([1.0 + al * a, -2.0 * c, 1.0 - al * a], [1.0 + al / a, -2.0 * c, 1.0 - al / a])
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

const BUTTERWORTH: f64 = std::f64::consts::FRAC_1_SQRT_2;

// ------------------------------------------------------------ puerta

/// Histéresis: abre en el umbral y cierra 3 dB por debajo (así una voz que ronda el umbral no tartamudea)
const GATE_HYSTERESIS: f64 = 3.0;
/// Caída del detector de pico: aguanta entre crestas de una voz grave sin cerrarse a cada ciclo
const GATE_DETECTOR_MS: f64 = 10.0;

/// Puerta de ruido con estéreo enlazado. Detector de pico → estado abierto/cerrado (con histéresis y hold)
/// → ganancia suavizada en dB: hacia 0 dB con el ataque, hacia −`range` con la relajación. Abierta y
/// asentada, la ganancia es exactamente 1 (no toca la señal).
struct Gate {
    open_at: f64,
    close_at: f64,
    floor_db: f64,
    det: f64,
    det_rel: f64,
    hold_len: u32,
    hold: u32,
    open: bool,
    /// Ganancia actual en dB (≤ 0)
    g_db: f64,
    att: f64,
    rel: f64,
}

impl Gate {
    fn new(fs: f64, g: &crate::Gate) -> Self {
        let thr = g.threshold as f64;
        Self {
            open_at: db_to_lin(thr),
            close_at: db_to_lin(thr - GATE_HYSTERESIS),
            floor_db: -(g.range as f64),
            det: 0.0,
            det_rel: coef(GATE_DETECTOR_MS, fs),
            hold_len: (g.hold as f64 * 0.001 * fs) as u32,
            hold: 0,
            open: false,
            // Arranca cerrada: el ruido de fondo no se cuela al cargar
            g_db: -(g.range as f64),
            att: coef(g.attack as f64, fs),
            rel: coef(g.release as f64, fs),
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let a = l.abs().max(r.abs());
        self.det = if a > self.det { a } else { a + (self.det - a) * self.det_rel };
        if self.det >= if self.open { self.close_at } else { self.open_at } {
            self.open = true;
            self.hold = self.hold_len;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.open = false;
        }
        let target = if self.open { 0.0 } else { self.floor_db };
        let k = if target > self.g_db { self.att } else { self.rel };
        self.g_db = target + (self.g_db - target) * k;
        if (self.g_db - target).abs() < 1e-6 {
            self.g_db = target;
        }
        if self.g_db == 0.0 {
            return (l, r);
        }
        let g = db_to_lin(self.g_db);
        (l * g, r * g)
    }
}

// ------------------------------------------------------------- diálogos

/// Campana ancha (1–4 kHz) solo en el mid: la voz centrada sube, el ambiente estéreo no se toca
struct Voice {
    eq: Biquad,
}

impl Voice {
    fn new(fs: f64, db: f64) -> Self {
        Self { eq: Biquad::peak(fs, 2000.0, 0.7, db) }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let (m, s) = (0.5 * (l + r), 0.5 * (l - r));
        let m = self.eq.tick(m);
        (m + s, m - s)
    }
}

// ---------------------------------------------------------------- graves

/// Paso bajo de los graves: fijo, es lo que da su carácter al realce
const BOOST_FREQ: f64 = 100.0;
const BOOST_Q: f64 = 1.0;
/// k con la rueda al máximo
const BOOST_MAX: f64 = 2.0;
/// Techo (dBTP) de la protección que va con los graves cuando el limitador del usuario está apagado
const PROTECT_CEILING: f64 = -0.1;

/// «Graves»: `y = x + k·LP(x)` en cada canal, con LP un paso bajo RBJ de 2.º orden (100 Hz, Q 1) y
/// k = 2 · rueda. Al máximo sube +9,8 dB por debajo de 63 Hz y +7 dB a 100 Hz; por encima, la fase del
/// paso bajo resta y deja un hueco suave (−4 dB hacia 160–250 Hz) que evita que embarre los medios-graves.
/// Lineal: sin armónicos, sin retardo y sin mezclar canales.
struct LowBoost {
    lp: [Biquad; 2],
    k: f64,
}

impl LowBoost {
    fn new(fs: f64, amount: f64) -> Self {
        Self { lp: [Biquad::lowpass(fs, BOOST_FREQ, BOOST_Q); 2], k: BOOST_MAX * amount }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        (l + self.k * self.lp[0].tick(l), r + self.k * self.lp[1].tick(r))
    }
}

/// «Armónicos» (graves psicoacústicos): de la banda grave (mono) salen sus armónicos 2.º y 3.º (polinomios de
/// Chebyshev sobre la señal normalizada por su envolvente), que el oído completa con la fundamental.
struct Harmonics {
    low: [Biquad; 2],
    band: [Biquad; 3],
    env: f64,
    att: f64,
    rel: f64,
    mix: f64,
}

impl Harmonics {
    fn new(fs: f64, freq: f64, amount: f64) -> Self {
        Self {
            low: [Biquad::lowpass(fs, freq, BUTTERWORTH); 2],
            // Fuera la fundamental y el continuo; arriba solo hasta el 4.º armónico
            band: [Biquad::highpass(fs, freq, BUTTERWORTH), Biquad::highpass(fs, freq, BUTTERWORTH), Biquad::lowpass(fs, freq * 4.0, BUTTERWORTH)],
            env: 0.0,
            att: coef(1.0, fs),
            rel: coef(100.0, fs),
            mix: amount,
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let b = self.low.iter_mut().fold(0.5 * (l + r), |x, f| f.tick(x));
        let a = b.abs();
        let k = if a > self.env { self.att } else { self.rel };
        self.env = a + (self.env - a) * k;
        let e = self.env.max(1e-9);
        let u = (b / e).clamp(-1.0, 1.0);
        // Con un seno de amplitud e: T2 = 2.º armónico y T3 = 3.º, a amplitud e y e/2
        let h = e * ((2.0 * u * u - 1.0) + 0.5 * (4.0 * u * u * u - 3.0 * u));
        let h = self.band.iter_mut().fold(h, |x, f| f.tick(x)) * self.mix;
        (l + h, r + h)
    }
}

// ------------------------------------------------------------- nivelador

/// Nivelador lento (AGC) por sonoridad: mide en bloques de 100 ms con ponderación K (ITU-R BS.1770),
/// promedia ~3 s, ignora silencios y pausas (puertas absoluta y relativa) y lleva la ganancia hacia
/// `objetivo − medida` despacio: sube en ~3 s y baja en ~1 s.
///
/// Va antes del compresor (que así trabaja siempre sobre el mismo nivel) y, si el compresor está
/// encendido, otro igual después (`trim`) recupera el volumen que se come la compresión: los dos miden
/// su propia entrada, así que ninguno se realimenta y la salida acaba en el objetivo.
struct Agc {
    k: [[Biquad; 2]; 2],
    acc: f64,
    n: u32,
    block: u32,
    ema: f64,
    /// Energía ponderada promedio (−1 = aún sin medir)
    ms: f64,
    target: f64,
    max_gain: f64,
    up: f64,
    down: f64,
    gain: f64,
    want: f64,
}

fn k_weighting(fs: f64) -> [Biquad; 2] {
    // Coeficientes de BS.1770 recalculados para cualquier frecuencia de muestreo (como libebur128)
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad::new(
        [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    );
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Biquad::new([1.0, -2.0, 1.0], [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]);
    [shelf, hp]
}

fn loudness(ms: f64) -> f64 {
    -0.691 + 10.0 * ms.max(1e-20).log10()
}

impl Agc {
    fn new(fs: f64, target: f64, max_gain: f64) -> Self {
        Self {
            k: [k_weighting(fs); 2],
            acc: 0.0,
            n: 0,
            block: (fs * 0.1) as u32,
            ema: (-0.1f64 / 3.0).exp(),
            ms: -1.0,
            target,
            max_gain,
            up: 1.0 - coef(3000.0, fs),
            down: 1.0 - coef(1000.0, fs),
            gain: 1.0,
            want: 1.0,
        }
    }

    /// Lo que guardó la instancia anterior del mismo dispositivo (el motor crea una nueva en cada
    /// cambio de ajustes): así no vuelve a empezar de cero
    fn restore(&mut self, ms: f64, gain_db: f64) {
        if ms.is_finite() && ms > 0.0 && gain_db.is_finite() {
            self.ms = ms;
            self.gain = db_to_lin(gain_db.clamp(-30.0, self.max_gain));
            self.want = self.gain;
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let kl = self.k[0].iter_mut().fold(l, |x, f| f.tick(x));
        let kr = self.k[1].iter_mut().fold(r, |x, f| f.tick(x));
        self.acc += kl * kl + kr * kr;
        self.n += 1;
        if self.n >= self.block {
            let blk = self.acc / self.n as f64;
            (self.acc, self.n) = (0.0, 0);
            let lk = loudness(blk);
            if lk > -70.0 && (self.ms < 0.0 || lk > loudness(self.ms) - 20.0) {
                self.ms = if self.ms < 0.0 { blk } else { blk + (self.ms - blk) * self.ema };
            }
            if self.ms > 0.0 {
                self.want = db_to_lin((self.target - loudness(self.ms)).clamp(-30.0, self.max_gain));
            }
        }
        let k = if self.want > self.gain { self.up } else { self.down };
        self.gain += (self.want - self.gain) * k;
        (l * self.gain, r * self.gain)
    }
}

// ------------------------------------------------------------- compresor

/// Compresor feed-forward con estéreo enlazado, detector de pico y rodilla suave; la envolvente
/// trabaja sobre la reducción en dB (ataque/relajación ramificados)
struct Compressor {
    thr: f64,
    slope: f64,
    knee: f64,
    makeup: f64,
    att: f64,
    rel: f64,
    /// Reducción actual en dB (≥ 0)
    env: f64,
}

impl Compressor {
    fn new(fs: f64, c: &crate::Comp) -> Self {
        Self {
            thr: c.threshold as f64,
            slope: 1.0 / c.ratio as f64 - 1.0,
            knee: c.knee as f64,
            makeup: c.makeup as f64,
            att: coef(c.attack as f64, fs),
            rel: coef(c.release as f64, fs),
            env: 0.0,
        }
    }

    /// Reducción estática (dB, ≥ 0) para un nivel de entrada
    fn curve(&self, x_db: f64) -> f64 {
        let over = x_db - self.thr;
        if self.knee > 0.0 && 2.0 * over.abs() <= self.knee {
            -self.slope * (over + self.knee / 2.0).powi(2) / (2.0 * self.knee)
        } else if over > 0.0 {
            -self.slope * over
        } else {
            0.0
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let gr = self.curve(lin_to_db(l.abs().max(r.abs())));
        let k = if gr > self.env { self.att } else { self.rel };
        self.env = gr + (self.env - gr) * k;
        let g = db_to_lin(self.makeup - self.env);
        (l * g, r * g)
    }
}

// ------------------------------------------------------------- crossfeed

/// Crossfeed de Bauer con los filtros de bs2b (Boris Mikhaylov, MIT): paso bajo hacia el otro oído y
/// realce de agudos en el directo, normalizado para que lo mono no cambie de nivel en graves
struct Bs2b {
    a0_lo: f64,
    b1_lo: f64,
    a0_hi: f64,
    a1_hi: f64,
    b1_hi: f64,
    gain: f64,
    lo: [f64; 2],
    hi: [f64; 2],
    prev: [f64; 2],
}

impl Bs2b {
    fn new(fs: f64, fc: f64, feed: f64) -> Self {
        let gb_lo = feed * -5.0 / 6.0 - 3.0;
        let gb_hi = feed / 6.0 - 3.0;
        let g_lo = 10f64.powf(gb_lo / 20.0);
        let g_hi = 1.0 - 10f64.powf(gb_hi / 20.0);
        let fc_hi = fc * 2f64.powf((gb_lo - 20.0 * g_hi.log10()) / 12.0);
        let x_lo = (-2.0 * PI * fc.min(0.45 * fs) / fs).exp();
        let x_hi = (-2.0 * PI * fc_hi.min(0.45 * fs) / fs).exp();
        Self {
            a0_lo: g_lo * (1.0 - x_lo),
            b1_lo: x_lo,
            a0_hi: 1.0 - g_hi * (1.0 - x_hi),
            a1_hi: -x_hi,
            b1_hi: x_hi,
            gain: 1.0 / (1.0 - g_hi + g_lo),
            lo: [0.0; 2],
            hi: [0.0; 2],
            prev: [0.0; 2],
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        for (c, x) in [l, r].into_iter().enumerate() {
            self.lo[c] = self.a0_lo * x + self.b1_lo * self.lo[c];
            self.hi[c] = self.a0_hi * x + self.a1_hi * self.prev[c] + self.b1_hi * self.hi[c];
            self.prev[c] = x;
        }
        ((self.hi[0] + self.lo[1]) * self.gain, (self.hi[1] + self.lo[0]) * self.gain)
    }
}

// ------------------------------------------------- altavoces virtuales

/// Tomas como mucho de cada respuesta (a frecuencias de muestreo muy altas se recorta la cola)
const MAX_TAPS: usize = 1024;

/// FIR directo sobre un historial duplicado: la ventana de las últimas `n` muestras siempre es un tramo
/// continuo, sin módulos en el bucle (el compilador lo vectoriza)
struct Fir {
    h: Vec<f64>,
    buf: Vec<f64>,
    pos: usize,
}

impl Fir {
    fn new(h: Vec<f64>) -> Self {
        let n = h.len();
        Self { h, buf: vec![0.0; 2 * n], pos: 0 }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let n = self.h.len();
        self.pos = if self.pos == 0 { n - 1 } else { self.pos - 1 };
        self.buf[self.pos] = x;
        self.buf[self.pos + n] = x;
        // h[k] · x[t − k], con cuatro sumas parciales para que no dependa cada suma de la anterior
        let win = &self.buf[self.pos..self.pos + n];
        let mut acc = [0.0; 4];
        let (hc, wc) = (self.h.chunks_exact(4), win.chunks_exact(4));
        let tail: f64 = hc.remainder().iter().zip(wc.remainder()).map(|(a, b)| a * b).sum();
        for (a, b) in hc.zip(wc) {
            for i in 0..4 {
                acc[i] += a[i] * b[i];
            }
        }
        acc[0] + acc[1] + acc[2] + acc[3] + tail
    }
}

/// Cambia la frecuencia de muestreo de una respuesta al impulso (interpolación con sinc enventanada; al
/// bajar de frecuencia filtra antes). Conserva la respuesta en frecuencia, no la amplitud de cada muestra.
fn resample(src: &[f32], from: f64, to: f64) -> Vec<f64> {
    // Ceros delante para que el principio de la sinc (antes del primer frente) no se corte
    const PAD: usize = 8;
    const ZEROS: f64 = 16.0;
    let r = to / from;
    let c = r.min(1.0);
    let half = ZEROS / c;
    let x: Vec<f64> = std::iter::repeat_n(0.0, PAD).chain(src.iter().map(|&v| v as f64)).collect();
    let full = ((x.len() as f64 + half) * r).ceil() as usize;
    let len = full.clamp(1, MAX_TAPS);
    let mut out: Vec<f64> = (0..len)
        .map(|n| {
            let t = n as f64 / r;
            let lo = (t - half).ceil().max(0.0) as usize;
            let hi = ((t + half).floor() as usize).min(x.len() - 1);
            (lo..=hi)
                .map(|k| {
                    let d = t - k as f64;
                    let sinc = if d == 0.0 { 1.0 } else { (PI * c * d).sin() / (PI * c * d) };
                    x[k] * sinc * 0.5 * (1.0 + (PI * d / half).cos())
                })
                .sum::<f64>()
                * c
                / r
        })
        .collect();
    // Si se ha recortado por el límite de tomas, fundido corto al final
    let fade = if full > len { 64 } else { 0 };
    for i in 0..fade {
        out[len - 1 - i] *= 0.5 * (1.0 - (PI * (i as f64 + 0.5) / fade as f64).cos());
    }
    out
}

/// Altavoces virtuales a ±30° con las HRIR del KEMAR ([`crate::hrir`]): cada oído recibe su canal por
/// el trayecto cercano (ipsi) y el otro canal por el lejano (contra), cuatro trayectos en total:
///
/// ```text
/// oído L = ipsi * L + contra * ((1 − k)·L + k·R)
/// oído R = ipsi * R + contra * ((1 − k)·R + k·L)
/// ```
///
/// `k` es la intensidad. Con k = 0 cada oído oye solo su canal (por ipsi + contra) y con k = 1 los
/// altavoces completos. Así lo que está en el centro (L = R) pasa siempre por ipsi + contra, que las
/// tablas ya traen compensado a plano: el timbre del centro no cambia con la intensidad y no hay filtro
/// peine entre una señal seca y otra retrasada.
///
/// Como el juego es simétrico (LL = RR, LR = RL), se calcula en mid/side con dos convoluciones en vez
/// de cuatro: `mid · (ipsi + contra)` y `side · (ipsi + (1 − 2k)·contra)`.
struct Speakers {
    mid: Fir,
    side: Fir,
}

impl Speakers {
    fn new(fs: f64, amount: f64) -> Self {
        let ipsi = resample(&crate::hrir::IPSI, crate::hrir::RATE, fs);
        let contra = resample(&crate::hrir::CONTRA, crate::hrir::RATE, fs);
        let k = 1.0 - 2.0 * amount.clamp(0.0, 1.0);
        Self {
            mid: Fir::new(ipsi.iter().zip(&contra).map(|(i, c)| i + c).collect()),
            side: Fir::new(ipsi.iter().zip(&contra).map(|(i, c)| i + k * c).collect()),
        }
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let m = self.mid.tick(0.5 * (l + r));
        let s = self.side.tick(0.5 * (l - r));
        (m + s, m - s)
    }
}

// ------------------------------------------------------------- limitador

/// Lookahead del limitador
pub const LOOKAHEAD_MS: f64 = 1.5;
/// Fases intermedias del detector de picos entre muestras (sobremuestreo ×4)
const TAPS: usize = 16;
/// Muestra del historial que mira el detector (la central)
const MID: usize = TAPS / 2 - 1;

/// Limitador brickwall con lookahead y detector de true peak (×4).
///
/// La ganancia necesaria de cada muestra (`techo / pico`) pasa por un mínimo deslizante de `w`
/// muestras, una relajación (bajar es instantáneo) y una media móvil de `w` muestras; el audio va
/// retrasado lo justo (`w + MID`) para que la media ya haya llegado al mínimo cuando pasa el pico.
/// Así ninguna muestra pide más ganancia de la que tiene, y al final un recorte al techo cubre los
/// errores de redondeo.
struct Brickwall {
    ceil: f64,
    /// Techo del detector: 0.05 dB por debajo, lo que se escapa entre muestras al mover la ganancia
    aim: f64,
    w: usize,
    rel: f64,
    phases: [[f64; TAPS]; 3],
    hist: [[f64; TAPS]; 2],
    hpos: usize,
    delay: Vec<[f64; 2]>,
    dpos: usize,
    /// Mínimo deslizante: cola monótona (índice, ganancia) en un anillo
    mins: Vec<(u64, f64)>,
    mhead: usize,
    mlen: usize,
    n: u64,
    smooth: f64,
    boxes: Vec<f64>,
    bpos: usize,
    sum: f64,
    /// Ganancia mínima aplicada desde la última lectura del medidor
    pub low: f64,
}

impl Brickwall {
    fn new(fs: f64, ceiling_db: f64) -> Self {
        let w = ((fs * LOOKAHEAD_MS / 1000.0).round() as usize).max(4);
        // Interpolación con sinc enventanada (Hann), 16 tomas, normalizada a ganancia 1 en continua
        let mut phases = [[0.0; TAPS]; 3];
        for (p, taps) in phases.iter_mut().enumerate() {
            let frac = (p + 1) as f64 / 4.0;
            for (k, t) in taps.iter_mut().enumerate() {
                let x = MID as f64 + frac - k as f64;
                let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
                *t = sinc * 0.5 * (1.0 + (PI * x / (TAPS as f64 / 2.0 + 0.5)).cos());
            }
            let s: f64 = taps.iter().sum();
            taps.iter_mut().for_each(|t| *t /= s);
        }
        Self {
            ceil: db_to_lin(ceiling_db),
            aim: db_to_lin(ceiling_db - 0.05),
            w,
            rel: coef(80.0, fs),
            phases,
            hist: [[0.0; TAPS]; 2],
            hpos: 0,
            delay: vec![[0.0; 2]; w + MID],
            dpos: 0,
            mins: vec![(0, 1.0); w + 1],
            mhead: 0,
            mlen: 0,
            n: 0,
            smooth: 1.0,
            boxes: vec![1.0; w],
            bpos: 0,
            sum: w as f64,
            low: 1.0,
        }
    }

    fn latency(&self) -> usize {
        self.delay.len()
    }

    #[inline]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        // Detector: la muestra central del historial y los tres puntos entre ella y la siguiente
        self.hist[0][self.hpos] = l;
        self.hist[1][self.hpos] = r;
        self.hpos = (self.hpos + 1) % TAPS;
        let mut peak = 0f64;
        for h in &self.hist {
            let x = |k: usize| h[(self.hpos + k) % TAPS];
            peak = peak.max(x(MID).abs());
            for taps in &self.phases {
                let v: f64 = (0..TAPS).map(|k| taps[k] * x(k)).sum();
                peak = peak.max(v.abs());
            }
        }
        let need = if peak > self.aim { self.aim / peak } else { 1.0 };

        // Mínimo de las últimas `w` ganancias necesarias
        let cap = self.mins.len();
        while self.mlen > 0 && self.mins[(self.mhead + self.mlen - 1) % cap].1 >= need {
            self.mlen -= 1;
        }
        self.mins[(self.mhead + self.mlen) % cap] = (self.n, need);
        self.mlen += 1;
        while self.mins[self.mhead].0 + self.w as u64 <= self.n {
            self.mhead = (self.mhead + 1) % cap;
            self.mlen -= 1;
        }
        self.n += 1;
        let hold = self.mins[self.mhead].1;

        // Bajar al momento, subir con la relajación; nunca por encima de `hold`
        self.smooth = if hold < self.smooth { hold } else { hold + (self.smooth - hold) * self.rel };

        // Media móvil (la suma se rehace en cada vuelta para que no acumule error)
        self.sum += self.smooth - self.boxes[self.bpos];
        self.boxes[self.bpos] = self.smooth;
        self.bpos += 1;
        if self.bpos == self.w {
            self.bpos = 0;
            self.sum = self.boxes.iter().sum();
        }
        let gain = (self.sum / self.w as f64).min(1.0);
        self.low = self.low.min(gain);

        let [dl, dr] = std::mem::replace(&mut self.delay[self.dpos], [l, r]);
        self.dpos = (self.dpos + 1) % self.delay.len();
        let c = self.ceil;
        ((dl * gain).clamp(-c, c), (dr * gain).clamp(-c, c))
    }
}

// ------------------------------------------------------------------ cadena

/// Medidas acumuladas desde la última lectura
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    /// Reducción de ganancia en dB (≥ 0)
    pub gr_comp: f32,
    pub gr_limit: f32,
    /// Pico de salida en dBFS
    pub peak: f32,
}

pub struct Chain {
    gate: Option<Gate>,
    voice: Option<Voice>,
    boost: Option<LowBoost>,
    harmonics: Option<Harmonics>,
    agc: Option<Agc>,
    comp: Option<Compressor>,
    /// Recupera tras el compresor lo que este baja de media (solo con nivelador y compresor)
    trim: Option<Agc>,
    bs2b: Option<Bs2b>,
    speakers: Option<Speakers>,
    /// El limitador del usuario o, si está apagado y los graves suben, uno de protección a −0,1 dBTP
    limiter: Option<Brickwall>,
    /// Algo salió no finito (no debería): desde ahí, paso directo hasta la siguiente instancia
    failed: bool,
    gr_comp: f64,
    peak: f64,
}

/// Lo que puede subir el nivelador de después del compresor
const TRIM_MAX: f64 = 12.0;

/// Límite de entrada: muy por encima de cualquier audio real, pero lejos de desbordar nada
const MAX_IN: f64 = 1000.0;

impl Chain {
    pub fn new(s: &Settings, fs: f64) -> Self {
        let fs = if fs.is_finite() { fs.clamp(8000.0, 768_000.0) } else { 48000.0 };
        let boost = s.bass_boost.active();
        Self {
            gate: s.gate.on.then(|| Gate::new(fs, &s.gate)),
            voice: s.dialog.on.then(|| Voice::new(fs, s.dialog.amount as f64)),
            boost: boost.then(|| LowBoost::new(fs, s.bass_boost.amount as f64)),
            harmonics: s.bass.on.then(|| Harmonics::new(fs, s.bass.freq as f64, s.bass.amount as f64)),
            agc: s.leveler.on.then(|| Agc::new(fs, s.leveler.target as f64, s.leveler.max_gain as f64)),
            comp: s.comp.on.then(|| Compressor::new(fs, &s.comp)),
            trim: (s.leveler.on && s.comp.on).then(|| Agc::new(fs, s.leveler.target as f64, TRIM_MAX)),
            // Nunca los dos (ya lo impide `Settings::sanitize`): con altavoces virtuales, el crossfeed sobra
            bs2b: (s.crossfeed.on && !s.speakers.on).then(|| Bs2b::new(fs, s.crossfeed.freq as f64, s.crossfeed.feed as f64)),
            speakers: s.speakers.on.then(|| Speakers::new(fs, s.speakers.amount as f64)),
            // Los graves pueden subir casi 10 dB: la salida nunca pasa de 0 dBFS aunque el limitador esté apagado
            limiter: (s.limiter.on || boost).then(|| Brickwall::new(fs, if s.limiter.on { s.limiter.ceiling as f64 } else { PROTECT_CEILING })),
            failed: false,
            gr_comp: 0.0,
            peak: 0.0,
        }
    }

    /// Retardo real en muestras (solo el limitador tiene lookahead)
    pub fn latency(&self) -> usize {
        self.limiter.as_ref().map_or(0, Brickwall::latency)
    }

    /// Estado de los niveladores (antes y después del compresor) para la siguiente instancia:
    /// (energía promedio, ganancia en dB); energía ≤ 0 = aún sin medir
    pub fn leveler_state(&self) -> Option<[(f64, f64); 2]> {
        let state = |a: &Option<Agc>| a.as_ref().map_or((-1.0, 0.0), |a| (a.ms, lin_to_db(a.gain)));
        self.agc.as_ref().filter(|a| a.ms > 0.0).map(|_| [state(&self.agc), state(&self.trim)])
    }

    pub fn restore_leveler(&mut self, state: [(f64, f64); 2]) {
        for (agc, (ms, gain)) in [&mut self.agc, &mut self.trim].into_iter().zip(state) {
            if let Some(a) = agc {
                a.restore(ms, gain);
            }
        }
    }

    #[inline]
    pub fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let clean = |x: f64| if x.is_finite() { x.clamp(-MAX_IN, MAX_IN) } else { 0.0 };
        let dry = (clean(l), clean(r));
        if self.failed {
            return dry;
        }
        let (mut l, mut r) = dry;
        if let Some(p) = &mut self.gate {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.voice {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.boost {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.harmonics {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.agc {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.comp {
            (l, r) = p.tick(l, r);
            self.gr_comp = self.gr_comp.max(p.env);
        }
        if let Some(p) = &mut self.trim {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.bs2b {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.speakers {
            (l, r) = p.tick(l, r);
        }
        if let Some(p) = &mut self.limiter {
            (l, r) = p.tick(l, r);
        }
        if !(l.is_finite() && r.is_finite()) {
            self.failed = true;
            return dry;
        }
        self.peak = self.peak.max(l.abs()).max(r.abs());
        (l, r)
    }

    /// Medidas desde la última llamada (y vuelta a empezar)
    pub fn take_meters(&mut self) -> Meters {
        let gr_limit = self.limiter.as_mut().map_or(0.0, |p| -lin_to_db(std::mem::replace(&mut p.low, 1.0)));
        let m = Meters { gr_comp: self.gr_comp as f32, gr_limit: gr_limit.max(0.0) as f32, peak: lin_to_db(self.peak) as f32 };
        self.gr_comp = 0.0;
        self.peak = 0.0;
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    const FS: f64 = 48000.0;

    fn sine(f: f64, amp: f64, secs: f64) -> Vec<f64> {
        (0..(FS * secs) as usize).map(|i| amp * (2.0 * PI * f * i as f64 / FS).sin()).collect()
    }

    /// Amplitud de una frecuencia concreta (Goertzel) en el tramo final de la señal
    fn tone(x: &[f64], f: f64) -> f64 {
        let x = &x[x.len() - 24000..];
        let w = 2.0 * PI * f / FS;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &v in x {
            let s = v + 2.0 * w.cos() * s1 - s2;
            (s2, s1) = (s1, s);
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt() * 2.0 / x.len() as f64
    }

    fn run(chain: &mut Chain, l: &[f64], r: &[f64]) -> (Vec<f64>, Vec<f64>) {
        l.iter().zip(r).map(|(&a, &b)| chain.tick(a, b)).unzip()
    }

    fn true_peak(l: &[f64], r: &[f64]) -> f64 {
        let mut m = ebur128::EbuR128::new(2, FS as u32, ebur128::Mode::TRUE_PEAK).unwrap();
        let inter: Vec<f64> = l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect();
        m.add_frames_f64(&inter).unwrap();
        lin_to_db(m.true_peak(0).unwrap().max(m.true_peak(1).unwrap()))
    }

    fn lufs(l: &[f64], r: &[f64]) -> f64 {
        let mut m = ebur128::EbuR128::new(2, FS as u32, ebur128::Mode::I).unwrap();
        let inter: Vec<f64> = l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect();
        m.add_frames_f64(&inter).unwrap();
        m.loudness_global().unwrap()
    }

    fn with(f: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = Settings::default();
        f(&mut s);
        s
    }

    /// Ruido determinista (xorshift) en [-1, 1]
    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 11) as f64 / (1u64 << 52) as f64 * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn limitador_nunca_pasa_del_techo() {
        for ceiling in [-1.0, -0.1, -6.0] {
            let s = with(|s| {
                s.limiter.on = true;
                s.limiter.ceiling = ceiling;
            });
            let ceil = db_to_lin(ceiling as f64);
            // Seno muy caliente, fs/4 desfasado (picos entre muestras de +3 dB), ruido +12 dB, impulsos y escalones
            let mut l = sine(997.0, 4.0, 0.5);
            l.extend((0..24000).map(|i| 2.0 * (PI / 2.0 * i as f64 + PI / 4.0).sin()));
            l.extend(noise(48000, 7).iter().map(|v| v * 4.0));
            l.extend((0..48000).map(|i| if i % 4800 == 0 { 8.0 } else if (i / 2400) % 2 == 0 { 0.9 } else { -0.05 }));
            let r: Vec<f64> = noise(l.len(), 99).iter().zip(&l).map(|(n, x)| x * 0.5 + n * 2.0).collect();
            let mut chain = Chain::new(&s, FS);
            let (ol, or) = run(&mut chain, &l, &r);
            let max = ol.iter().chain(&or).fold(0f64, |m, v| m.max(v.abs()));
            assert!(max <= ceil, "pico de muestra {} dB con techo {ceiling}", lin_to_db(max));
            let tp = true_peak(&ol, &or);
            assert!(tp <= ceiling as f64 + 0.01, "true peak {tp:.2} dBTP con techo {ceiling}");
            assert!(chain.take_meters().gr_limit > 10.0);
        }
    }

    #[test]
    fn limitador_transparente_por_debajo_del_techo() {
        let s = with(|s| s.limiter.on = true);
        let mut chain = Chain::new(&s, FS);
        let d = chain.latency();
        assert!(d as f64 / FS <= 0.002, "lookahead {d} muestras");
        let x = sine(1000.0, 0.5, 0.2);
        let (ol, _) = run(&mut chain, &x, &x);
        assert!(ol[d..].iter().zip(&x).all(|(a, b)| a == b), "debe salir idéntico, solo retrasado");
        assert_eq!(chain.take_meters().gr_limit, 0.0);
    }

    #[test]
    fn compresor_reduce_lo_que_toca() {
        let s = with(|s| {
            s.comp.on = true;
            s.comp.threshold = -20.0;
            s.comp.ratio = 4.0;
            s.comp.knee = 0.0;
        });
        // Seno a −6 dBFS: 14 dB por encima, 4:1 → 10.5 dB en la cresta; con detector de pico la
        // envolvente se asienta algo por debajo (entre crestas relaja)
        let mut chain = Chain::new(&s, FS);
        let x = sine(1000.0, 0.5, 1.0);
        let (ol, _) = run(&mut chain, &x, &x);
        let gr = chain.take_meters().gr_comp as f64;
        assert!((8.5..=10.5).contains(&gr), "reducción medida {gr}");
        let out = ol[ol.len() - 4800..].iter().fold(0f64, |m, v| m.max(v.abs()));
        assert!((lin_to_db(out) - (-6.0 - gr)).abs() < 1.0, "pico de salida {}", lin_to_db(out));

        // Por debajo del umbral (y sin makeup) no toca nada
        let mut chain = Chain::new(&s, FS);
        let x = sine(1000.0, 0.05, 0.3);
        let (ol, _) = run(&mut chain, &x, &x);
        assert_eq!(ol, x);
        assert_eq!(chain.take_meters().gr_comp, 0.0);
    }

    #[test]
    fn rodilla_suave_es_continua() {
        let c = Compressor::new(FS, &crate::Comp { on: true, threshold: -20.0, ratio: 4.0, attack: 10.0, release: 100.0, knee: 6.0, makeup: 0.0 });
        assert_eq!(c.curve(-23.0), 0.0);
        assert!((c.curve(-17.0) - 3.0 * 0.75).abs() < 1e-9);
        assert!(c.curve(-20.0) > 0.0 && c.curve(-20.0) < 0.75 * 3.0);
    }

    #[test]
    fn nivelador_llega_al_objetivo_sin_pasarse_del_limite() {
        let s = with(|s| {
            s.leveler.on = true;
            s.leveler.target = -14.0;
            s.leveler.max_gain = 24.0;
        });
        let x: Vec<f64> = noise(48000 * 25, 3).iter().map(|v| v * 0.03).collect();
        let before = lufs(&x, &x);
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &x);
        let tail = ol.len() - 48000 * 5;
        let after = lufs(&ol[tail..], &or[tail..]);
        assert!((after - -14.0).abs() < 1.0, "de {before:.1} a {after:.1} LUFS");

        // Límite de subida: con 6 dB como máximo no llega
        let s = with(|s| {
            s.leveler.on = true;
            s.leveler.max_gain = 6.0;
        });
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &x);
        let after = lufs(&ol[tail..], &or[tail..]);
        assert!((after - (before + 6.0)).abs() < 0.5, "con límite: {after:.1}");

        // Con un compresor fuerte detrás (como en «Noche») la salida también acaba en el objetivo
        let s = with(|s| {
            s.leveler.on = true;
            s.leveler.target = -23.0;
            s.leveler.max_gain = 18.0;
            s.comp.on = true;
            s.comp.threshold = -32.0;
            s.comp.ratio = 6.0;
            s.comp.attack = 2.0;
            s.comp.release = 300.0;
        });
        // Escenas alternas: 1 s fuerte, 1 s flojo
        let x: Vec<f64> = noise(48000 * 30, 11).iter().enumerate().map(|(i, v)| v * if (i / 48000) % 2 == 0 { 0.5 } else { 0.05 }).collect();
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &x);
        let tail = ol.len() - 48000 * 8;
        let after = lufs(&ol[tail..], &or[tail..]);
        assert!((after - -23.0).abs() < 1.5, "con compresor: {after:.1} LUFS");
        assert!(chain.take_meters().gr_comp > 3.0);

        // El silencio no se sube
        let mut chain = Chain::new(&s, FS);
        let z = vec![0.0; 48000 * 3];
        run(&mut chain, &z, &z);
        assert!(chain.leveler_state().is_none());
    }

    #[test]
    fn nivelador_retoma_el_estado_anterior() {
        let s = with(|s| s.leveler.on = true);
        let x: Vec<f64> = noise(48000 * 20, 5).iter().map(|v| v * 0.03).collect();
        let mut a = Chain::new(&s, FS);
        run(&mut a, &x, &x);
        let state = a.leveler_state().unwrap();
        let g = state[0].1;
        let mut b = Chain::new(&s, FS);
        b.restore_leveler(state);
        let (l, _) = b.tick(0.01, 0.01);
        assert!((lin_to_db(l / 0.01) - g).abs() < 0.01, "debe arrancar con {g:.1} dB");
    }

    #[test]
    fn armonicos_de_los_graves() {
        let s = with(|s| {
            s.bass.on = true;
            s.bass.amount = 0.5;
        });
        let x = sine(50.0, 0.3, 1.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &x);
        assert_eq!(ol, or);
        let (h2, h3) = (tone(&ol, 100.0), tone(&ol, 150.0));
        assert!(h2 > 0.03 && h3 > 0.01, "armónicos {h2:.3} {h3:.3}");
        assert!(tone(&x, 100.0) < 1e-6);
        // La fundamental no cambia
        assert!((tone(&ol, 50.0) / 0.3 - 1.0).abs() < 0.1);
    }

    fn boost(amount: f32) -> Settings {
        with(|s| s.bass_boost = crate::BassBoost { on: true, amount })
    }

    /// Lo que pide la especificación de «Graves»: |1 + k·H(e^jw)| en dB, con H el paso bajo RBJ (100 Hz, Q 1)
    /// calculado aquí aparte, a partir de las fórmulas del Cookbook
    fn boost_spec_db(amount: f64, f: f64, fs: f64) -> f64 {
        let w0 = 2.0 * PI * 100.0 / fs;
        let alpha = w0.sin() / 2.0;
        let (b0, b1, b2) = ((1.0 - w0.cos()) / 2.0, 1.0 - w0.cos(), (1.0 - w0.cos()) / 2.0);
        let (a0, a1, a2) = (1.0 + alpha, -2.0 * w0.cos(), 1.0 - alpha);
        let w = 2.0 * PI * f / fs;
        // Polinomio en z^-1 = e^-jw
        let poly = |c: [f64; 3]| (c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos(), -c[1] * w.sin() - c[2] * (2.0 * w).sin());
        let (nr, ni) = poly([b0, b1, b2]);
        let (dr, di) = poly([a0, a1, a2]);
        let den = dr * dr + di * di;
        let (hr, hi) = ((nr * dr + ni * di) / den, (ni * dr - nr * di) / den);
        let k = 2.0 * amount;
        10.0 * ((1.0 + k * hr).powi(2) + (k * hi).powi(2)).log10()
    }

    #[test]
    fn graves_respuesta_como_la_especificacion() {
        for fs in [44100.0, 48000.0, 96000.0] {
            for amount in [0.5, 1.0] {
                // Respuesta al impulso de la cadena (pequeño: la protección no actúa y solo retrasa)
                let mut chain = Chain::new(&boost(amount as f32), fs);
                let mut x = vec![0.0; fs as usize];
                x[0] = 0.01;
                let (h, hr) = run(&mut chain, &x, &x);
                assert_eq!(h, hr);
                assert!(chain.take_meters().gr_limit == 0.0);
                let h: Vec<f64> = h.iter().map(|v| v * 100.0).collect();
                // De 20 Hz a 20 kHz por tercios de octava
                for i in 0..=30 {
                    let f = 20.0 * 2f64.powf(i as f64 / 3.0);
                    let (got, want) = (fir_db(&h, f, fs), boost_spec_db(amount, f, fs));
                    assert!((got - want).abs() < 0.1, "{fs} Hz, rueda {amount}: {got:.2} dB a {f:.0} Hz (debe ser {want:.2})");
                }
                // Los puntos medidos en el original
                let at = |f: f64| fir_db(&h, f, fs);
                let near = |got: f64, want: f64, what: &str| assert!((got - want).abs() <= 0.1, "{fs} Hz, rueda {amount}, {what}: {got:.2} dB");
                if amount == 1.0 {
                    near(at(40.0), 9.8, "40 Hz");
                    near(at(100.0), 7.0, "100 Hz");
                    near(at(200.0), -4.1, "200 Hz");
                } else {
                    near(at(40.0), 6.1, "40 Hz");
                }
                // A 1 kHz la fórmula aún resta un poco (−0,17 dB al máximo); a 10 kHz ya nada
                assert!(at(1000.0) < 0.0 && at(1000.0) > -0.2, "{fs} Hz, rueda {amount}, 1 kHz: {:.2} dB", at(1000.0));
                near(at(10000.0), 0.0, "10 kHz");
            }
        }
    }

    #[test]
    fn graves_sin_mezclar_canales_ni_retardo_propio() {
        // Solo izquierda: la derecha sigue en silencio
        let mut chain = Chain::new(&boost(1.0), FS);
        let x = sine(50.0, 0.1, 1.0);
        let zero = vec![0.0; x.len()];
        let (ol, or) = run(&mut chain, &x, &zero);
        assert!(or.iter().all(|&v| v == 0.0));
        assert!(tone(&ol, 50.0) > 0.25);
        // Sin armónicos: a 100 y 150 Hz no aparece nada
        assert!(tone(&ol, 100.0) < 1e-6 && tone(&ol, 150.0) < 1e-6);
        // El único retardo es el lookahead de la protección
        assert_eq!(chain.latency(), Brickwall::new(FS, PROTECT_CEILING).latency());
    }

    #[test]
    fn graves_a_cero_no_tocan_nada() {
        // Encendido con la rueda a 0: ni procesador ni protección, la salida es la entrada bit a bit
        let s = boost(0.0);
        assert!(!s.active());
        let mut chain = Chain::new(&s, FS);
        assert_eq!(chain.latency(), 0);
        let x = noise(48000, 41);
        let y = noise(48000, 42);
        let (ol, or) = run(&mut chain, &x, &y);
        assert!(ol.iter().zip(&x).chain(or.iter().zip(&y)).all(|(a, b)| a.to_bits() == b.to_bits()));
        // Con otro realce encendido, igual que si los graves no existieran
        let mut with_boost = boost(0.0);
        with_boost.dialog.on = true;
        let without = with(|s| s.dialog.on = true);
        let (a, _) = run(&mut Chain::new(&with_boost, FS), &x, &y);
        let (b, _) = run(&mut Chain::new(&without, FS), &x, &y);
        assert_eq!(a, b);
    }

    #[test]
    fn graves_protegidos_contra_la_saturacion() {
        // 50 Hz a −6 dBFS al máximo: sin protección saldría a +3,8 dBFS; con ella sube unos +6 dB y no pasa del techo
        let mut chain = Chain::new(&boost(1.0), FS);
        let x = sine(50.0, 0.5, 2.0);
        let (ol, or) = run(&mut chain, &x, &x);
        let ceil = db_to_lin(PROTECT_CEILING);
        let max = ol.iter().chain(&or).fold(0f64, |m, v| m.max(v.abs()));
        assert!(max <= ceil, "pico {} dBFS", lin_to_db(max));
        let gain = lin_to_db(tone(&ol, 50.0) / 0.5);
        assert!((5.5..=6.0).contains(&gain), "a 50 Hz sube {gain:.2} dB");
        assert!(chain.take_meters().gr_limit > 3.0);
        // Señal fuerte de todo tipo (ruido a 0 dBFS, graves a tope, escalones): ni muestras ni true peak por encima
        let mut l = noise(48000, 51);
        l.extend(sine(40.0, 1.0, 1.0));
        l.extend((0..48000).map(|i| if (i / 4800) % 2 == 0 { 1.0 } else { -1.0 }));
        let r: Vec<f64> = l.iter().map(|v| -v * 0.8).collect();
        let mut chain = Chain::new(&boost(1.0), FS);
        let (ol, or) = run(&mut chain, &l, &r);
        let max = ol.iter().chain(&or).fold(0f64, |m, v| m.max(v.abs()));
        assert!(max <= ceil, "pico {} dBFS", lin_to_db(max));
        assert!(true_peak(&ol, &or) <= PROTECT_CEILING + 0.01);
        // Con el limitador del usuario encendido manda su techo
        let mut s = boost(1.0);
        (s.limiter.on, s.limiter.ceiling) = (true, -3.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, _) = run(&mut chain, &x, &x);
        assert!(ol.iter().all(|v| v.abs() <= db_to_lin(-3.0)));
    }

    #[test]
    fn dialogos_solo_en_el_centro() {
        let s = with(|s| {
            s.dialog.on = true;
            s.dialog.amount = 6.0;
        });
        let x = sine(2000.0, 0.1, 1.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, _) = run(&mut chain, &x, &x);
        assert!((lin_to_db(tone(&ol, 2000.0) / 0.1) - 6.0).abs() < 0.2);
        // Lo que no está en el centro (L = −R) sale igual
        let neg: Vec<f64> = x.iter().map(|v| -v).collect();
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &neg);
        assert!(ol.iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-12));
        assert!(or.iter().zip(&neg).all(|(a, b)| (a - b).abs() < 1e-12));
    }

    #[test]
    fn crossfeed_cruza_graves_y_respeta_lo_mono() {
        let s = with(|s| s.crossfeed.on = true);
        let zero = vec![0.0; 48000];
        // Solo izquierda a 200 Hz: llega al derecho unos dB por debajo
        let x = sine(200.0, 0.5, 1.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &zero);
        let cross = lin_to_db(tone(&or, 200.0) / tone(&ol, 200.0));
        assert!((-12.0..-3.0).contains(&cross), "cruce a 200 Hz: {cross:.1} dB");
        // A 10 kHz casi no cruza
        let x = sine(10000.0, 0.5, 1.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, or) = run(&mut chain, &x, &zero);
        assert!(lin_to_db(tone(&or, 10000.0) / tone(&ol, 10000.0)) < -20.0);
        // Mono en graves: mismo nivel
        let x = sine(100.0, 0.5, 1.0);
        let mut chain = Chain::new(&s, FS);
        let (ol, _) = run(&mut chain, &x, &x);
        assert!(lin_to_db(tone(&ol, 100.0) / 0.5).abs() < 0.5);
    }

    /// Respuesta (dB) de un FIR a una frecuencia
    fn fir_db(h: &[f64], f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (re, im) = h.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &v)| (re + v * (w * n as f64).cos(), im - v * (w * n as f64).sin()));
        10.0 * (re * re + im * im).log10()
    }

    fn speakers(amount: f32) -> Settings {
        with(|s| {
            s.speakers.on = true;
            s.speakers.amount = amount;
        })
    }

    #[test]
    fn remuestreo_conserva_la_respuesta() {
        let base: Vec<f64> = crate::hrir::IPSI.iter().map(|&v| v as f64).collect();
        // A la misma frecuencia sale idéntica (con los ceros de delante)
        let same = resample(&crate::hrir::IPSI, crate::hrir::RATE, crate::hrir::RATE);
        assert!(same[8..8 + base.len()].iter().zip(&base).all(|(a, b)| (a - b).abs() < 1e-9));
        for fs in [32000.0, 48000.0, 96000.0, 192000.0] {
            let h = resample(&crate::hrir::IPSI, crate::hrir::RATE, fs);
            for f in [100.0, 1000.0, 5000.0, 10000.0, 14000.0] {
                let d = fir_db(&h, f, fs) - fir_db(&base, f, crate::hrir::RATE);
                assert!(d.abs() < 0.25, "{fs} Hz: {d:.2} dB de diferencia a {f} Hz");
            }
        }
        // A frecuencias absurdas no pasa del límite de tomas
        assert!(resample(&crate::hrir::IPSI, crate::hrir::RATE, 768_000.0).len() <= MAX_TAPS);
    }

    #[test]
    fn altavoces_igual_que_cuatro_convoluciones() {
        // Referencia: los cuatro trayectos (LL, LR, RL, RR) convolucionados a mano (intensidad exacta en f32)
        let amount = 0.75;
        let ipsi = resample(&crate::hrir::IPSI, crate::hrir::RATE, FS);
        let contra = resample(&crate::hrir::CONTRA, crate::hrir::RATE, FS);
        let (l, r) = (noise(3000, 21), noise(3000, 22));
        let conv = |h: &[f64], x: &dyn Fn(usize) -> f64, t: usize| (0..h.len().min(t + 1)).map(|k| h[k] * x(t - k)).sum::<f64>();
        let mut chain = Chain::new(&speakers(amount as f32), FS);
        let (ol, or) = run(&mut chain, &l, &r);
        for t in 0..l.len() {
            let to_l = |i: usize| (1.0 - amount) * l[i] + amount * r[i];
            let to_r = |i: usize| (1.0 - amount) * r[i] + amount * l[i];
            let el = conv(&ipsi, &|i| l[i], t) + conv(&contra, &to_l, t);
            let er = conv(&ipsi, &|i| r[i], t) + conv(&contra, &to_r, t);
            assert!((ol[t] - el).abs() < 1e-9 && (or[t] - er).abs() < 1e-9, "muestra {t}");
        }
    }

    #[test]
    fn altavoces_conservan_el_centro() {
        for amount in [0.0, 0.4, 1.0] {
            // Ruido mono de 100 Hz a 10 kHz (donde está casi toda la energía de la música): misma energía
            // (±1 dB) y los dos oídos idénticos
            let mut band = [Biquad::highpass(FS, 100.0, BUTTERWORTH), Biquad::lowpass(FS, 10000.0, BUTTERWORTH), Biquad::lowpass(FS, 10000.0, BUTTERWORTH)];
            let x: Vec<f64> = noise(48000 * 2, 31).iter().map(|v| band.iter_mut().fold(v * 0.3, |x, f| f.tick(x))).collect();
            let mut chain = Chain::new(&speakers(amount), FS);
            let (ol, or) = run(&mut chain, &x, &x);
            assert_eq!(ol, or);
            let e = |v: &[f64]| v.iter().map(|s| s * s).sum::<f64>();
            let d = 10.0 * (e(&ol) / e(&x)).log10();
            assert!(d.abs() < 1.0, "energía del centro {d:.2} dB con intensidad {amount}");
            // Y tonos sueltos en la zona de la voz y los instrumentos, sin cambios de más de 2 dB
            for f in [125.0, 250.0, 500.0, 1000.0, 2000.0, 3000.0, 5000.0] {
                let x = sine(f, 0.3, 1.0);
                let mut chain = Chain::new(&speakers(amount), FS);
                let (ol, _) = run(&mut chain, &x, &x);
                let d = lin_to_db(tone(&ol, f) / 0.3);
                assert!(d.abs() < 2.0, "centro a {f} Hz: {d:.2} dB con intensidad {amount}");
            }
        }
    }

    #[test]
    fn altavoces_dan_diferencias_entre_oidos() {
        // Solo canal izquierdo a 1 kHz: llega al oído derecho más bajo (ILD) y más tarde (ITD)
        let zero = vec![0.0; 48000];
        let x = sine(1000.0, 0.3, 1.0);
        let mut chain = Chain::new(&speakers(1.0), FS);
        let (ol, or) = run(&mut chain, &x, &zero);
        let ild = lin_to_db(tone(&ol, 1000.0) / tone(&or, 1000.0));
        assert!((3.0..12.0).contains(&ild), "ILD a 1 kHz: {ild:.1} dB");
        let mut imp = vec![0.0; 2000];
        imp[0] = 1.0;
        let mut chain = Chain::new(&speakers(1.0), FS);
        let (ol, or) = run(&mut chain, &imp, &zero[..2000]);
        let first = |v: &[f64]| {
            let top = v.iter().fold(0f64, |m, s| m.max(s.abs()));
            v.iter().position(|s| s.abs() > 0.3 * top).unwrap()
        };
        let itd = (first(&or) as f64 - first(&ol) as f64) / FS * 1000.0;
        assert!((0.1..0.5).contains(&itd), "ITD {itd:.3} ms");
        // Con intensidad 0 no cruza nada al otro oído
        let mut chain = Chain::new(&speakers(0.0), FS);
        let (_, or) = run(&mut chain, &x, &zero);
        assert!(or.iter().all(|&v| v == 0.0));
    }

    fn gate(threshold: f32, range: f32, hold: f32) -> Settings {
        with(|s| s.gate = crate::Gate { on: true, threshold, range, attack: 1.0, hold, release: 50.0 })
    }

    /// Ganancia (dB) que aplica la cadena en cada muestra (con la misma entrada en L y R)
    fn gains(chain: &mut Chain, x: &[f64]) -> Vec<f64> {
        x.iter().map(|&v| if v == 0.0 { f64::NAN } else { lin_to_db(chain.tick(v, v).0 / v) }).collect()
    }

    #[test]
    fn puerta_baja_el_ruido_y_deja_pasar_la_voz() {
        // Ruido de fondo a −60 dBFS con umbral −45: cerrada, baja exactamente la atenuación
        let mut chain = Chain::new(&gate(-45.0, 30.0, 100.0), FS);
        let hiss: Vec<f64> = noise(48000, 4).iter().map(|v| v * 0.001).collect();
        let (ol, or) = run(&mut chain, &hiss, &hiss);
        assert_eq!(ol, or);
        let tail = 24000;
        let atten = lin_to_db(rms(&ol[tail..]) / rms(&hiss[tail..]));
        assert!((atten + 30.0).abs() < 0.1, "atenuación {atten:.2} dB");

        // Voz (tono a −20 dBFS): abierta, sale bit a bit igual en cuanto se asienta el ataque
        let mut chain = Chain::new(&gate(-45.0, 30.0, 100.0), FS);
        let x = sine(220.0, 0.1, 0.5);
        let (ol, _) = run(&mut chain, &x, &x);
        assert_eq!(&ol[4800..], &x[4800..], "abierta no toca la señal");
        // El ataque abre rápido: a los 5 ms ya no queda ni 1 dB
        let mut chain = Chain::new(&gate(-45.0, 30.0, 100.0), FS);
        let g = gains(&mut chain, &x);
        assert!(g[240..].iter().filter(|v| v.is_finite()).all(|&v| v > -1.0));

        // Atenuación 0: la puerta no hace nada aunque esté cerrada
        let mut chain = Chain::new(&gate(-45.0, 0.0, 100.0), FS);
        let (ol, _) = run(&mut chain, &hiss, &hiss);
        assert_eq!(ol, hiss);
    }

    #[test]
    fn puerta_aguanta_el_hold_y_luego_cierra() {
        // 200 ms de voz y después ruido de fondo: sigue abierta durante el hold (150 ms) y luego cierra
        let mut chain = Chain::new(&gate(-40.0, 40.0, 150.0), FS);
        let mut x = sine(300.0, 0.2, 0.2);
        x.extend(noise(48000, 8).iter().map(|v| v * 0.0005 + 1e-9));
        let g = gains(&mut chain, &x);
        let at = |ms: f64| g[(FS * ms / 1000.0) as usize];
        assert!(at(300.0) > -0.5, "en el hold sigue abierta: {:.1} dB", at(300.0));
        // Hold 150 ms (más lo que tarda el detector en caer) y relajación de 50 ms: a los 700 ms ya ha cerrado
        assert!(at(700.0) < -39.0, "cerrada: {:.1} dB", at(700.0));
        assert!(at(1100.0) < -39.9);
    }

    #[test]
    fn puerta_no_tartamudea_en_el_umbral() {
        // Un tono que oscila ±1 dB alrededor del umbral: con la histéresis de 3 dB, abre una vez y se queda
        let mut gate = Gate::new(FS, &crate::Gate { on: true, threshold: -30.0, range: 40.0, attack: 1.0, hold: 0.0, release: 20.0 });
        let mut changes = 0;
        let mut was = gate.open;
        for i in 0..(FS as usize * 2) {
            let t = i as f64 / FS;
            let amp = db_to_lin(-30.0 + (2.0 * PI * 3.0 * t).sin());
            gate.tick(amp * (2.0 * PI * 200.0 * t).sin(), 0.0);
            if gate.open != was {
                changes += 1;
                was = gate.open;
            }
        }
        assert_eq!(changes, 1, "cambios de estado: {changes}");
    }

    fn rms(x: &[f64]) -> f64 {
        (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn la_cadena_aguanta_basura() {
        let mut s = Settings::default();
        (s.limiter.on, s.comp.on, s.leveler.on, s.bass.on, s.dialog.on, s.crossfeed.on) = (true, true, true, true, true, true);
        s.speakers.on = true;
        s.bass_boost = crate::BassBoost { on: true, amount: 1.0 };
        s.gate.on = true;
        let mut chain = Chain::new(&s, FS);
        for x in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300, -1e300, 0.0, 1e-310] {
            let (l, r) = chain.tick(x, -x);
            assert!(l.is_finite() && r.is_finite() && l.abs() <= 1.0 && r.abs() <= 1.0);
        }
        let x = noise(48000, 1);
        let (ol, or) = run(&mut chain, &x, &x);
        assert!(ol.iter().chain(&or).all(|v| v.is_finite() && v.abs() <= db_to_lin(-1.0)));
        // Frecuencias de muestreo raras: no revientan
        for fs in [0.0, f64::NAN, 8000.0, 384000.0] {
            let mut c = Chain::new(&s, fs);
            let (l, _) = c.tick(0.5, 0.5);
            assert!(l.is_finite());
        }
    }
}
