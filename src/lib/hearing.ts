// Test de audición: de los umbrales medidos (dBFS por oído y frecuencia) a una curva de compensación suave.
//
// Los umbrales se comparan con 1 kHz y con lo que oye un oído normal (ISO 226:2003, umbral en campo libre,
// también relativo a 1 kHz): así un oído sano da una curva plana aunque de por sí oigamos peor los graves y
// los agudos extremos. Lo que falta se compensa solo a medias, con límites y suavizado. La medida incluye la
// respuesta de los cascos con los que se hace (EQ y dinámica van en plano durante el test).
import { autoPreamp, clamp, curveDb, type Band, type Curve } from './eq.ts'

export const FREQS = [250, 500, 1000, 2000, 3000, 4000, 6000, 8000, 10000, 12000]

/** Umbral normal relativo a 1 kHz (dB) en cada frecuencia de `FREQS`: ISO 226:2003, interpolado en log f */
export const NORMAL = [9.0, 2.0, 0, -3.7, -8.0, -7.8, 2.0, 10.2, 11.5, 10.2]

/** Nivel del tono (dBFS): empieza muy bajo y nunca pasa del tope, sea cual sea el volumen de Windows */
export const MIN_LEVEL = -100
export const MAX_LEVEL = -35
export const START_LEVEL = -90

/** Límites de la compensación (dB) */
export const MAX_BOOST = 6
export const MAX_CUT = -4

/** Diferencia entre oídos (dB) a partir de la que se aconseja una revisión */
export const ASYMMETRY = 15

export type Ear = 'left' | 'right'
export const EARS: Ear[] = ['left', 'right']

/** Orden de las pruebas en cada oído: primero 1 kHz (la referencia, la más fácil), luego hacia arriba y al final los graves */
export const ORDER = [1000, 2000, 3000, 4000, 6000, 8000, 10000, 12000, 500, 250].map((f) => FREQS.indexOf(f))

/** Umbral de cada frecuencia (en el orden de `FREQS`), en dBFS */
export type Thresholds = Record<Ear, number[]>

const round1 = (v: number): number => Math.round(v * 10) / 10

/** Compensación de un oído (dB por frecuencia): media pérdida respecto a lo normal, con límites y suavizada */
export function compensation(thr: number[]): number[] {
  const ref = thr[FREQS.indexOf(1000)]
  const half = thr.map((t, i) => clamp(0.5 * (t - ref - NORMAL[i]), MAX_CUT, MAX_BOOST))
  // Suavizado 1-2-1 entre frecuencias vecinas (en los extremos, con la propia)
  return half.map((g, i) => round1(0.25 * (half[i - 1] ?? g) + 0.5 * g + 0.25 * (half[i + 1] ?? g)))
}

/** Q de cada campana según la distancia a sus vecinas: más juntas, más estrechas */
const QS = FREQS.map((f, i) => {
  const lo = FREQS[i - 1]
  const hi = FREQS[i + 1]
  const bw = lo && hi ? Math.log2(hi / lo) / 2 : Math.log2((hi ?? f) / (lo ?? f))
  return Math.round(100 / (2 * Math.sinh((Math.LN2 / 2) * bw))) / 100
})

/** Campanas en `FREQS` cuyo resultado pasa por `target` en cada frecuencia (las campanas vecinas se suman:
 *  se ajustan las ganancias en unas pasadas) */
export function fitBands(target: number[]): Band[] {
  const bands: Band[] = FREQS.map((freq, i) => ({ kind: 'peak', freq, gain: target[i], q: QS[i], on: true }))
  for (let pass = 0; pass < 40; pass++) {
    const err = FREQS.map((f, i) => target[i] - curveDb(bands, f))
    bands.forEach((b, i) => (b.gain = clamp(b.gain + 0.6 * err[i], -24, 24)))
  }
  bands.forEach((b) => (b.gain = round1(b.gain)))
  return bands
}

/** Curva «Mi audición». Las bandas comunes siguen al oído que más necesita en cada frecuencia y el otro oído
 *  solo resta: así el preamp automático (que mira las comunes) sigue evitando que sature. */
export function hearingCurve(t: Thresholds): Curve {
  const left = compensation(t.left)
  const right = compensation(t.right)
  const main = left.map((v, i) => Math.max(v, right[i]))
  const bands = fitBands(main)
  const ear = (c: number[]): Band[] => (c.some((v, i) => v - main[i] <= -0.1) ? fitBands(c.map((v, i) => v - main[i])).filter((b) => Math.abs(b.gain) >= 0.1) : [])
  const ears = { left: ear(left), right: ear(right) }
  return {
    preamp: autoPreamp(bands),
    autoPreamp: true,
    bands,
    preset: null,
    ears: ears.left.length || ears.right.length ? ears : null
  }
}

/** Mayor diferencia entre los umbrales de los dos oídos (dB) */
export function asymmetry(t: Thresholds): number {
  return Math.max(...t.left.map((v, i) => Math.abs(v - t.right[i])))
}
