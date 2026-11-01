//! Supresión de ruido del micrófono con RNNoise (`nnnoiseless`, port en Rust de la librería de Xiph, BSD-3).
//!
//! RNNoise trabaja a 48 kHz en bloques de 480 muestras: hasta 50 kHz (44,1 y 48) se le pasa el audio tal cual (a
//! 44,1 sus bandas quedan un 8 % más abajo, sin efecto apreciable en la voz) y por encima se diezma por un factor
//! entero. Añade un bloque de retardo (~10 ms), solo en el micrófono. `amount` mezcla la señal limpia con la
//! original (alineada con el mismo retardo).

use nnnoiseless::DenoiseState;

const FRAME: usize = DenoiseState::FRAME_SIZE;
/// RNNoise espera muestras en la escala de 16 bits
const SCALE: f32 = 32768.0;

struct Channel {
    state: Box<DenoiseState<'static>>,
    input: [f32; FRAME],
    output: [f32; FRAME],
    /// Muestras de entrada acumuladas para diezmar
    acc: f32,
    /// Últimas dos salidas de RNNoise (para interpolar al volver a la frecuencia original)
    prev: f32,
    cur: f32,
    /// Señal original retrasada lo mismo que la limpia
    delay: Vec<f32>,
}

pub struct Denoise {
    ch: [Channel; 2],
    factor: usize,
    /// Muestras acumuladas en el diezmado (0…factor)
    n: usize,
    pos: usize,
    dpos: usize,
    mix: f64,
}

impl Denoise {
    pub fn new(fs: f64, amount: f64) -> Self {
        let factor = if fs <= 50_000.0 {
            1
        } else if fs <= 100_000.0 {
            2
        } else {
            4
        };
        // ponytail: diezmado por media de `factor` muestras e interpolación lineal; un filtro polifásico si algún
        // micro a 96/192 kHz suena raro
        let len = FRAME * factor + factor;
        let ch = || Channel { state: DenoiseState::new(), input: [0.0; FRAME], output: [0.0; FRAME], acc: 0.0, prev: 0.0, cur: 0.0, delay: vec![0.0; len] };
        Self { ch: [ch(), ch()], factor, n: 0, pos: 0, dpos: 0, mix: amount.clamp(0.0, 1.0) }
    }

    /// Retardo en muestras de la frecuencia original
    pub fn latency(&self) -> usize {
        FRAME * self.factor + self.factor
    }

    #[inline]
    pub fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let x = [l as f32, r as f32];
        let mut y = [0.0f64; 2];
        let len = self.ch[0].delay.len();
        self.n += 1;
        let ready = self.n == self.factor;
        for (c, ch) in self.ch.iter_mut().enumerate() {
            let dry = ch.delay[self.dpos];
            ch.delay[self.dpos] = x[c];
            ch.acc += x[c];
            if ready {
                // Una muestra a la frecuencia de RNNoise: entra la media y sale la limpia de hace un bloque
                ch.input[self.pos] = ch.acc / self.factor as f32 * SCALE;
                ch.prev = ch.cur;
                ch.cur = ch.output[self.pos] / SCALE;
                ch.acc = 0.0;
            }
            let t = if ready { 0.0 } else { self.n as f32 / self.factor as f32 };
            let wet = ch.prev + (ch.cur - ch.prev) * t;
            y[c] = dry as f64 * (1.0 - self.mix) + wet as f64 * self.mix;
        }
        self.dpos = (self.dpos + 1) % len;
        if ready {
            self.n = 0;
            self.pos += 1;
            if self.pos == FRAME {
                self.pos = 0;
                for ch in &mut self.ch {
                    let input = ch.input;
                    ch.state.process_frame(&mut ch.output, &input);
                }
            }
        }
        (y[0], y[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruido de fondo bajo (unos −55 dBFS RMS), como el de un micro en una habitación en silencio
    fn noise(i: usize) -> f64 {
        ((i as u32).wrapping_mul(2654435761) as f64 / u32::MAX as f64 - 0.5) * 0.004
    }

    #[test]
    fn baja_el_ruido_de_fondo() {
        for fs in [48000.0, 96000.0] {
            let mut d = Denoise::new(fs, 1.0);
            let n = (fs as usize) * 2;
            let (mut e_in, mut e_out) = (0.0, 0.0);
            for i in 0..n {
                let x = noise(i);
                let (l, r) = d.tick(x, x);
                assert!(l.is_finite() && r.is_finite());
                if i > n / 2 {
                    e_in += x * x;
                    e_out += l * l;
                }
            }
            // Ruido de fondo sin voz: RNNoise lo baja mucho (más de 10 dB)
            assert!(e_out < e_in / 10.0, "{fs}: {e_in} → {e_out}");
        }
    }

    #[test]
    fn mezcla_a_cero_es_la_original_retrasada() {
        let mut d = Denoise::new(48000.0, 0.0);
        let lat = d.latency();
        let out: Vec<f64> = (0..2000).map(|i| d.tick(noise(i), 0.0).0).collect();
        for (i, o) in out.iter().enumerate().skip(lat) {
            assert!((o - noise(i - lat) as f32 as f64).abs() < 1e-9);
        }
    }
}
