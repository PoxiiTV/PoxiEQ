// Matemática del ecualizador: respuesta de cada filtro (biquads RBJ, los mismos que usa Equalizer APO),
// presets de serie e importación de AutoEq.

export type Kind = 'peak' | 'lowShelf' | 'highShelf' | 'lowPass' | 'highPass'

export interface Band {
  kind: Kind
  freq: number
  gain: number
  q: number
  on: boolean
}

/** Bandas solo para un oído (después de las comunes): las crea el test de audición */
export interface Ears {
  left: Band[]
  right: Band[]
}

export interface Curve {
  preamp: number
  autoPreamp: boolean
  bands: Band[]
  preset: string | null
  /** Ajuste por oído; no viaja al compartir (es de quien hizo el test) */
  ears?: Ears | null
}

export const MIN_F = 20
export const MAX_F = 20000
export const MIN_G = -24
export const MAX_G = 24
export const MIN_Q = 0.1
export const MAX_Q = 20
export const MAX_BANDS = 32
const FS = 48000

export const hasGain = (k: Kind): boolean => k !== 'lowPass' && k !== 'highPass'
export const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v))

/** Coeficientes normalizados [b0, b1, b2, a1, a2] */
function coeffs(b: Band): number[] {
  const A = 10 ** (b.gain / 40)
  const w0 = (2 * Math.PI * b.freq) / FS
  const cos = Math.cos(w0)
  const alpha = Math.sin(w0) / (2 * b.q)
  const sq = 2 * Math.sqrt(A) * alpha
  let c: number[]
  switch (b.kind) {
    case 'peak':
      c = [1 + alpha * A, -2 * cos, 1 - alpha * A, 1 + alpha / A, -2 * cos, 1 - alpha / A]
      break
    case 'lowShelf':
      c = [
        A * (A + 1 - (A - 1) * cos + sq),
        2 * A * (A - 1 - (A + 1) * cos),
        A * (A + 1 - (A - 1) * cos - sq),
        A + 1 + (A - 1) * cos + sq,
        -2 * (A - 1 + (A + 1) * cos),
        A + 1 + (A - 1) * cos - sq
      ]
      break
    case 'highShelf':
      c = [
        A * (A + 1 + (A - 1) * cos + sq),
        -2 * A * (A - 1 + (A + 1) * cos),
        A * (A + 1 + (A - 1) * cos - sq),
        A + 1 - (A - 1) * cos + sq,
        2 * (A - 1 - (A + 1) * cos),
        A + 1 - (A - 1) * cos - sq
      ]
      break
    case 'lowPass':
      c = [(1 - cos) / 2, 1 - cos, (1 - cos) / 2, 1 + alpha, -2 * cos, 1 - alpha]
      break
    case 'highPass':
      c = [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2, 1 + alpha, -2 * cos, 1 - alpha]
      break
  }
  const a0 = c[3]
  return [c[0] / a0, c[1] / a0, c[2] / a0, c[4] / a0, c[5] / a0]
}

/** Ganancia en dB de una banda a la frecuencia `f` */
export function bandDb(b: Band, f: number): number {
  const [b0, b1, b2, a1, a2] = coeffs(b)
  const w = (2 * Math.PI * f) / FS
  const c1 = Math.cos(w)
  const s1 = Math.sin(w)
  const c2 = Math.cos(2 * w)
  const s2 = Math.sin(2 * w)
  const nr = b0 + b1 * c1 + b2 * c2
  const ni = -(b1 * s1 + b2 * s2)
  const dr = 1 + a1 * c1 + a2 * c2
  const di = -(a1 * s1 + a2 * s2)
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di))
}

export function curveDb(bands: Band[], f: number): number {
  let db = 0
  for (const b of bands) if (b.on) db += bandDb(b, f)
  return db
}

/** Frecuencias repartidas en escala logarítmica para pintar y medir la curva */
export function logFreqs(n: number): number[] {
  return Array.from({ length: n }, (_, i) => MIN_F * (MAX_F / MIN_F) ** (i / (n - 1)))
}

const GRID = logFreqs(400)

/** Preamp que evita saturar: baja justo lo que sube el pico de la curva */
export function autoPreamp(bands: Band[]): number {
  let peak = 0
  for (const f of GRID) peak = Math.max(peak, curveDb(bands, f))
  return 0 - Math.ceil(peak * 10) / 10
}

/** Pico de la curva con el preamp aplicado (> 0: puede saturar) */
export function headroom(c: Curve): number {
  let peak = -Infinity
  for (const f of GRID) peak = Math.max(peak, curveDb(c.bands, f))
  return peak + c.preamp
}

export function fmtFreq(f: number): string {
  if (f >= 1000) return `${+(f / 1000).toFixed(f >= 10000 ? 1 : 2)}k`
  return `${Math.round(f)}`
}

export const fmtDb = (g: number): string => `${g > 0 ? '+' : ''}${g.toFixed(1)}`

// ——— Presets de serie: las 10 bandas clásicas con distintas ganancias ———

export const TEN = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]

export function tenBand(gains: number[] = TEN.map(() => 0)): Band[] {
  return TEN.map((freq, i) => ({ kind: 'peak', freq, gain: gains[i] ?? 0, q: 1.41, on: true }))
}

export const BUILTIN: { id: string; gains: number[] }[] = [
  { id: 'b:flat', gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { id: 'b:bass', gains: [6, 5, 3.5, 1.5, 0, 0, 0, 0, 0, 0] },
  { id: 'b:voice', gains: [-3, -2, -1, 0, 1, 2.5, 3.5, 3, 1, 0] },
  { id: 'b:footsteps', gains: [-4, -3, -1.5, 0, 0, 1.5, 3.5, 5, 3, 1] },
  { id: 'b:cinema', gains: [4, 3, 1, 0, -1, 0, 1, 2, 2, 1] },
  { id: 'b:music', gains: [4, 3, 1.5, 0, -1, -1, 0, 1.5, 3, 3.5] },
  { id: 'b:night', gains: [-5, -3.5, -1.5, 0, 0, 0, 0, -1, -2, -3] }
]

export function builtinCurve(id: string, autoPre = true): Curve {
  const bands = tenBand(BUILTIN.find((p) => p.id === id)?.gains)
  return { preamp: autoPre ? autoPreamp(bands) : 0, autoPreamp: autoPre, bands, preset: id }
}

// ——— Importar / exportar (formato de Equalizer APO, el que da AutoEq como «ParametricEQ.txt») ———

const KINDS: Record<string, Kind> = {
  PK: 'peak',
  PEQ: 'peak',
  LSC: 'lowShelf',
  LS: 'lowShelf',
  HSC: 'highShelf',
  HS: 'highShelf',
  LPQ: 'lowPass',
  LP: 'lowPass',
  HPQ: 'highPass',
  HP: 'highPass'
}

/** Lee texto de AutoEq / Equalizer APO. `null` si no hay ni un filtro reconocible. */
export function parseApo(text: string): { preamp: number; bands: Band[] } | null {
  let preamp = 0
  const bands: Band[] = []
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    const pre = /^Preamp:\s*(-?[\d.]+)\s*dB/i.exec(line)
    if (pre) {
      preamp = +pre[1]
      continue
    }
    const m = /^Filter\s*\d*:\s*ON\s+([A-Z]+)\b(.*)$/i.exec(line)
    if (!m) continue
    const kind = KINDS[m[1].toUpperCase()]
    const fc = /Fc\s+([\d.]+)\s*Hz/i.exec(m[2])
    if (!kind || !fc) continue
    const gain = /Gain\s+(-?[\d.]+)\s*dB/i.exec(m[2])
    const q = /\bQ\s+([\d.]+)/i.exec(m[2])
    const def = kind === 'peak' ? 1 : 0.707
    bands.push({
      kind,
      freq: clamp(+fc[1], MIN_F, MAX_F),
      gain: hasGain(kind) ? clamp(+(gain?.[1] ?? 0), MIN_G, MAX_G) : 0,
      q: clamp(+(q?.[1] ?? def), MIN_Q, MAX_Q),
      on: true
    })
    if (bands.length === MAX_BANDS) break
  }
  return bands.length ? { preamp: clamp(preamp, -30, 12), bands } : null
}

export function toApo(c: Curve): string {
  const tag: Record<Kind, string> = { peak: 'PK', lowShelf: 'LSC', highShelf: 'HSC', lowPass: 'LPQ', highPass: 'HPQ' }
  const lines = [`Preamp: ${c.preamp.toFixed(1)} dB`]
  c.bands
    .filter((b) => b.on)
    .forEach((b, i) => {
      const gain = hasGain(b.kind) ? ` Gain ${b.gain.toFixed(1)} dB` : ''
      lines.push(`Filter ${i + 1}: ON ${tag[b.kind]} Fc ${+b.freq.toFixed(1)} Hz${gain} Q ${+b.q.toFixed(3)}`)
    })
  return lines.join('\n')
}
