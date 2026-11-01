// Formatos de EQ de otros programas: importar (Equalizer APO/Peace, REW, AutoEq, EasyEffects, GraphicEQ) y
// exportar (ParametricEQ.txt, EasyEffects, REW). Lo que llega de fuera se valida con rangos estrictos: un archivo
// con algo fuera de rango se rechaza entero, no se recorta.
import { hasGain, MAX_BANDS, MAX_F, MAX_G, MAX_Q, MIN_F, MIN_G, MIN_Q, parseApo, toApo, type Band, type Curve, type Kind } from './eq.ts'

export type EqFile =
  | { kind: 'bands'; format: 'apo' | 'easyeffects'; preamp: number; bands: Band[] }
  /** GraphicEQ (Wavelet, AutoEq): una curva de puntos que Rust convierte en filtros */
  | { kind: 'graphic'; points: [number, number][] }

const EE_KINDS: Record<string, Kind> = { Bell: 'peak', 'Lo-shelf': 'lowShelf', 'Hi-shelf': 'highShelf', 'Lo-pass': 'lowPass', 'Hi-pass': 'highPass' }
const inRange = (v: unknown, lo: number, hi: number): v is number => typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi

/** EasyEffects (preset de salida o de entrada con su ecualizador). `null` si no es o algo no vale. */
function readEasyEffects(text: string): EqFile | null {
  let j: unknown
  try {
    j = JSON.parse(text)
  } catch {
    return null
  }
  const root = j as Record<string, Record<string, unknown> | undefined>
  const side = root?.output ?? root?.input
  if (!side || typeof side !== 'object') return null
  const eq = (side['equalizer#0'] ?? side.equalizer) as Record<string, unknown> | undefined
  const left = eq?.left as Record<string, Record<string, unknown>> | undefined
  if (!eq || !left || typeof left !== 'object') return null
  const n = eq['num-bands']
  if (!inRange(n, 1, 64)) return null
  const bands: Band[] = []
  for (let i = 0; i < n; i++) {
    const b = left[`band${i}`]
    if (!b || typeof b !== 'object') return null
    if (b.type === 'Off' || b.mute === true) continue
    const kind = EE_KINDS[String(b.type)]
    if (!kind) return null
    if (!inRange(b.frequency, MIN_F, MAX_F) || !inRange(b.q, MIN_Q, MAX_Q) || (hasGain(kind) && !inRange(b.gain, MIN_G, MAX_G))) return null
    bands.push({ kind, freq: b.frequency, gain: hasGain(kind) ? (b.gain as number) : 0, q: b.q, on: true })
  }
  const pre = (eq['input-gain'] ?? 0) as number
  const post = (eq['output-gain'] ?? 0) as number
  if (!inRange(pre, -36, 36) || !inRange(post, -36, 36) || !bands.length || bands.length > MAX_BANDS) return null
  const preamp = pre + post
  if (!inRange(preamp, -30, 12)) return null
  return { kind: 'bands', format: 'easyeffects', preamp, bands }
}

/** `GraphicEQ: 20 -3.5; 21 -3.4; …` */
function readGraphic(text: string): EqFile | null {
  const m = /GraphicEQ:\s*([^\r\n]+)/i.exec(text)
  if (!m) return null
  const points: [number, number][] = []
  for (const part of m[1].split(';')) {
    const [f, g] = part.trim().split(/\s+/).map(Number)
    if (!inRange(f, 1, 24000) || !inRange(g, -30, 30)) return null
    if (points.length && f <= points[points.length - 1][0]) return null
    points.push([f, g])
  }
  return points.length >= 2 && points.length <= 2000 ? { kind: 'graphic', points } : null
}

/** Reconoce el formato y lo lee. Equalizer APO, Peace, REW y el ParametricEQ.txt de AutoEq comparten líneas
 *  `Filter N: ON PK Fc … Hz Gain … dB Q …`. */
export function readEqFile(text: string): EqFile | null {
  const t = text.trim()
  if (t.startsWith('{')) return readEasyEffects(t)
  if (/GraphicEQ:/i.test(t)) return readGraphic(t)
  const apo = parseApo(t)
  return apo && { kind: 'bands', format: 'apo', ...apo }
}

/** ParametricEQ.txt (Equalizer APO, Peace, Wavelet) */
export const toParametric = (c: Curve): string => `${toApo(c)}\n`

const EE_NAMES: Record<Kind, string> = { peak: 'Bell', lowShelf: 'Lo-shelf', highShelf: 'Hi-shelf', lowPass: 'Lo-pass', highPass: 'Hi-pass' }

/** Preset de salida de EasyEffects con solo el ecualizador */
export function toEasyEffects(c: Curve): string {
  const on = c.bands.filter((b) => b.on)
  const side = Object.fromEntries(
    on.map((b, i) => [
      `band${i}`,
      { frequency: +b.freq.toFixed(1), gain: hasGain(b.kind) ? +b.gain.toFixed(1) : 0, mode: 'RLC (BT)', mute: false, q: +b.q.toFixed(3), slope: 'x1', solo: false, type: EE_NAMES[b.kind] }
    ])
  )
  const eq = { balance: 0, bypass: false, 'input-gain': +c.preamp.toFixed(1), left: side, right: side, mode: 'IIR', 'num-bands': on.length, 'output-gain': 0, 'pitch-left': 0, 'pitch-right': 0, 'split-channels': false }
  return `${JSON.stringify({ output: { blocklist: [], 'equalizer#0': eq, plugins_order: ['equalizer#0'] } }, null, 4)}\n`
}

/** «Filter Settings file» de REW (lo carga en su EQ como «Generic») */
export function toRew(c: Curve): string {
  const tag: Record<Kind, string> = { peak: 'PK', lowShelf: 'LSC', highShelf: 'HSC', lowPass: 'LPQ', highPass: 'HPQ' }
  const lines = ['Filter Settings file', '', 'Room EQ V5.20', `Dated: ${new Date().toISOString().slice(0, 10)}`, '', 'Notes: PoxiEQ', '', 'Equaliser: Generic', `Preamp: ${c.preamp.toFixed(1)} dB`]
  c.bands
    .filter((b) => b.on)
    .forEach((b, i) => {
      const gain = hasGain(b.kind) ? `  Gain ${b.gain.toFixed(1).padStart(5)} dB` : ''
      lines.push(`Filter ${String(i + 1).padStart(2)}: ON  ${tag[b.kind].padEnd(4)} Fc ${b.freq.toFixed(1).padStart(7)} Hz${gain}  Q ${b.q.toFixed(3)}`)
    })
  return `${lines.join('\r\n')}\r\n`
}
