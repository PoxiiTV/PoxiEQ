// Genera `dsp/src/hrir.rs`: las respuestas al impulso de los altavoces virtuales de PoxiDSP.
//
// Origen: medidas HRTF del maniquí KEMAR del MIT Media Lab (Bill Gardner y Keith Martin, 1994), juego
// «diffuse-field equalized» a 44,1 kHz (https://sound.media.mit.edu/resources/KEMAR.html, `diffuse.zip`).
// Licencia: «provided free with no restrictions on use, provided the authors are cited» (ver THIRD-PARTY.md).
//
// Se usa solo el altavoz a 30° y 0° de elevación (`elev0/H0e030a.wav`). Ese archivo trae los dos oídos de una
// fuente a la derecha: el derecho es el oído del mismo lado (ipsilateral) y el izquierdo el del otro lado
// (contralateral). El juego compacto es simétrico, así que el altavoz izquierdo es el mismo con los oídos
// cambiados. Pasos:
//   1. Se quita el retardo común (tiempo de vuelo hasta la cabeza); la diferencia entre oídos (ITD) se queda.
//   2. Compensación de coloración: la suma ipsi + contra es lo que oye cada oído de un sonido centrado
//      (L = R). Se mide su respuesta, se suaviza a 1/3 de octava y se invierte (fase mínima, ±12 dB como
//      mucho) para que lo del centro suene con el mismo timbre que sin altavoces virtuales.
//   3. Las dos respuestas pasan por esa corrección y se recortan a lo que tiene energía.
//
// Uso: node scripts/hrir.mjs <ruta a elev0/H0e030a.wav de diffuse.zip>
import { readFileSync, writeFileSync } from 'node:fs'

const FS = 44100
const N = 2048
const MAX_TAPS = 256

const path = process.argv[2]
if (!path) {
  console.error('Uso: node scripts/hrir.mjs <elev0/H0e030a.wav>')
  process.exit(1)
}

// ——— WAV PCM 16 bits estéreo ———
const buf = readFileSync(path)
if (buf.toString('ascii', 0, 4) !== 'RIFF' || buf.readUInt16LE(22) !== 2 || buf.readUInt32LE(24) !== FS || buf.readUInt16LE(34) !== 16) {
  throw new Error('Se espera un WAV PCM de 16 bits, estéreo y 44,1 kHz')
}
let off = 12
while (buf.toString('ascii', off, off + 4) !== 'data') off += 8 + buf.readUInt32LE(off + 4)
const frames = buf.readUInt32LE(off + 4) / 4
const left = []
const right = []
for (let i = 0; i < frames; i++) {
  left.push(buf.readInt16LE(off + 8 + i * 4) / 32768)
  right.push(buf.readInt16LE(off + 10 + i * 4) / 32768)
}
const energy = (x) => x.reduce((a, v) => a + v * v, 0)
if (energy(right) <= energy(left)) throw new Error('El oído derecho debería ser el cercano (fuente a la derecha)')

// ——— FFT compleja (radix 2, en el sitio) ———
function fft(re, im, inverse = false) {
  const n = re.length
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1
    for (; j & bit; bit >>= 1) j ^= bit
    j ^= bit
    if (i < j) {
      ;[re[i], re[j]] = [re[j], re[i]]
      ;[im[i], im[j]] = [im[j], im[i]]
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = ((inverse ? 2 : -2) * Math.PI) / len
    for (let i = 0; i < n; i += len) {
      for (let k = 0; k < len / 2; k++) {
        const wr = Math.cos(ang * k)
        const wi = Math.sin(ang * k)
        const a = i + k
        const b = a + len / 2
        const tr = re[b] * wr - im[b] * wi
        const ti = re[b] * wi + im[b] * wr
        re[b] = re[a] - tr
        im[b] = im[a] - ti
        re[a] += tr
        im[a] += ti
      }
    }
  }
  if (inverse) for (let i = 0; i < n; i++) (re[i] /= n), (im[i] /= n)
}

const padded = (x) => Float64Array.from({ length: N }, (_, i) => x[i] ?? 0)

/** Potencia por bin hasta Nyquist */
function power(x) {
  const re = padded(x)
  const im = new Float64Array(N)
  fft(re, im)
  return Array.from({ length: N / 2 + 1 }, (_, k) => re[k] * re[k] + im[k] * im[k])
}

/** Suavizado a 1/3 de octava (media de potencia) */
function smooth(p) {
  return p.map((_, k) => {
    const lo = Math.max(1, Math.floor(k * 2 ** (-1 / 6)))
    const hi = Math.min(p.length - 1, Math.ceil(k * 2 ** (1 / 6)))
    let s = 0
    for (let i = lo; i <= hi; i++) s += p[i]
    return k === 0 ? p[1] : s / (hi - lo + 1)
  })
}

const db = (p) => 10 * Math.log10(Math.max(p, 1e-30))
const bin = (f) => Math.round((f * N) / FS)

// 1. Retardo común fuera (dejando 3 muestras antes del primer frente)
const peak = Math.max(...right.map(Math.abs))
const onset = right.findIndex((v) => Math.abs(v) > 0.1 * peak)
const shift = Math.max(0, onset - 3)
const ipsi = right.slice(shift)
const contra = left.slice(shift)

// 2 y 3. Corrección del centro: inversa suavizada de |ipsi + contra|, afinada en unas pasadas (el recorte y
// las muescas estrechas del pabellón hacen que una sola inversión no quede del todo plana)
// Fase mínima por cepstro real
function minPhase(magDb) {
  const re = new Float64Array(N)
  const im = new Float64Array(N)
  for (let k = 0; k <= N / 2; k++) {
    re[k] = (magDb[k] / 20) * Math.LN10
    if (k > 0 && k < N / 2) re[N - k] = re[k]
  }
  fft(re, im, true)
  for (let n = 1; n < N / 2; n++) (re[n] *= 2), (im[n] = 0)
  for (let n = N / 2 + 1; n < N; n++) (re[n] = 0), (im[n] = 0)
  im[0] = 0
  im[N / 2] = 0
  fft(re, im)
  for (let k = 0; k < N; k++) {
    const m = Math.exp(re[k])
    const ph = im[k]
    re[k] = m * Math.cos(ph)
    im[k] = m * Math.sin(ph)
  }
  fft(re, im, true)
  return Array.from(re)
}
const conv = (a, b) => Array.from({ length: a.length + b.length - 1 }, (_, n) => {
  let s = 0
  for (let k = Math.max(0, n - b.length + 1); k <= Math.min(n, a.length - 1); k++) s += a[k] * b[n - k]
  return s
})
const center = smooth(power(ipsi.map((v, i) => v + contra[i])))
// Referencia: el nivel medio entre 200 Hz y 8 kHz pasa a 0 dB
const ks = center.map((_, k) => k).filter((k) => k >= bin(200) && k <= bin(8000))
const ref = ks.reduce((a, k) => a + db(center[k]), 0) / ks.length
// Por debajo de 120 Hz la medida (128 muestras) no dice nada: la corrección se queda la de 120 Hz
const lo = bin(120)
let dev = center.map((p, k) => db(center[Math.max(k, lo)]) - ref)
let outI = []
let outC = []
let check = []
for (let pass = 0; pass < 6; pass++) {
  const corr = dev.map((d) => Math.min(12, Math.max(-12, -d)) - ref)
  const g = minPhase(corr).slice(0, 256)
  for (let i = 192; i < 256; i++) g[i] *= 0.5 * (1 + Math.cos((Math.PI * (i - 192)) / 64))
  const fi = conv(ipsi, g)
  const fc = conv(contra, g)
  // Se recorta donde lo que queda es menos de −40 dB de la energía total, con un fundido corto
  const e = fi.map((v, i) => v * v + fc[i] * fc[i])
  const total = e.reduce((a, v) => a + v, 0)
  let taps = e.length
  for (let rest = 0; taps > 32 && rest + e[taps - 1] < total * 1e-4; taps--) rest += e[taps - 1]
  taps = Math.min(MAX_TAPS, taps)
  const fade = 16
  const cut = (x) => x.slice(0, taps).map((v, i) => (i >= taps - fade ? v * 0.5 * (1 + Math.cos((Math.PI * (i - (taps - fade))) / fade)) : v))
  outI = cut(fi)
  outC = cut(fc)
  check = smooth(power(outI.map((v, i) => v + outC[i])))
  // Lo que aún se desvía se suma a la desviación de partida para la siguiente pasada
  dev = dev.map((d, k) => d + (k >= lo ? db(check[k]) : db(check[lo])))
}
const taps = outI.length

// Comprobación: el centro corregido queda plano (±1,5 dB entre 150 Hz y 12 kHz)
let worst = 0
for (let k = bin(150); k <= bin(12000); k++) worst = Math.max(worst, Math.abs(db(check[k])))
console.log(`retardo quitado: ${shift} muestras · ${taps} tomas · desviación máxima del centro: ${worst.toFixed(2)} dB`)
for (const f of [50, 100, 200, 500, 1000, 2000, 4000, 8000, 12000, 16000, 20000]) {
  console.log(`  ${String(f).padStart(5)} Hz  centro ${db(check[bin(f)]).toFixed(1).padStart(6)} dB  ipsi ${db(smooth(power(outI))[bin(f)]).toFixed(1).padStart(6)} dB  contra ${db(smooth(power(outC))[bin(f)]).toFixed(1).padStart(6)} dB`)
}
if (worst > 1.5) throw new Error('La compensación no deja el centro plano')

/** El literal más corto que vuelve al mismo f32 */
function f32(v) {
  const t = Math.fround(v)
  for (let p = 1; ; p++) {
    const s = t.toPrecision(p)
    if (Math.fround(+s) === t) return /[.e]/.test(s) ? s : `${s}.0`
  }
}
const list = (x) =>
  x
    .map(f32)
    .reduce((rows, v, i) => (i % 8 ? (rows[rows.length - 1] += `, ${v}`) : rows.push(`    ${v}`), rows), [])
    .join(',\n')
const out = `//! Respuestas al impulso de los altavoces virtuales (±30°, 0° de elevación) a ${FS} Hz.
//!
//! Generado por \`scripts/hrir.mjs\` (no editar a mano) a partir de las medidas HRTF del KEMAR del MIT Media
//! Lab, juego «diffuse-field equalized»: Bill Gardner y Keith Martin, «HRTF Measurements of a KEMAR
//! Dummy-Head Microphone», MIT Media Lab, 1994. Sin el retardo común y con la coloración del centro
//! compensada (ipsi + contra plano a 1/3 de octava).

/// Frecuencia de muestreo de las tablas
pub const RATE: f64 = ${FS}.0;

/// Oído del mismo lado que el altavoz
pub const IPSI: [f32; ${taps}] = [
${list(outI)},
];

/// Oído del otro lado (lleva la diferencia de tiempo entre oídos)
pub const CONTRA: [f32; ${taps}] = [
${list(outC)},
];
`
writeFileSync(new URL('../dsp/src/hrir.rs', import.meta.url), out)
console.log('dsp/src/hrir.rs escrito')
