// Compartir presets: un código de texto que se puede pegar en un chat (`PXEQ2.` + base64url de un JSON
// compacto) y el mismo código dentro de un archivo `.poxieq`. El código dice de qué sección es (`s`) y al
// importarlo va a esa sección. Al importar se valida todo y con rangos estrictos: un código fuera de rango no se
// «arregla», se rechaza. Los códigos `PXEQ1.` (los primeros, solo del ecualizador) se siguen leyendo.
import type { Dsp, Listen, Sim, Studio } from './api'
import { DSP_DEFAULT, DSP_RANGES, SECTION_PARTS, type Part } from './dsp.ts'
import { MAX_BANDS, MAX_F, MAX_G, MAX_Q, MIN_F, MIN_G, MIN_Q, type Band, type Curve, type Kind } from './eq.ts'

/** Versión en el prefijo: si el formato cambia, sube y las versiones viejas lo rechazan limpio */
export const SHARE_PREFIX = 'PXEQ2.'
const LEGACY_PREFIX = 'PXEQ1.'
export const MAX_CODE = 8192
export const MAX_FILE = 16 * 1024
export const MAX_NAME = 48

/** Lo que viaja en un código, según su sección */
export type Shared =
  | { section: 'eq'; name: string; curve: Curve }
  | { section: 'sound' | 'phones'; name: string; dsp: Dsp }
  | { section: 'studio'; name: string; studio: Studio }
  | { section: 'mic'; name: string; curve: Curve; dsp: Dsp }

export type ShareSection = Shared['section']

// Orden fijo (forma parte del formato): no depende del orden de los menús
const KIND_CODES: Kind[] = ['peak', 'lowShelf', 'highShelf', 'lowPass', 'highPass']
const LISTENS: Listen[] = ['stereo', 'mono', 'side', 'left', 'right']
const SIMS: Sim[] = ['phone', 'laptop', 'car', 'earbuds', 'tv', 'speaker']
const SECTIONS: ShareSection[] = ['eq', 'sound', 'phones', 'studio', 'mic']

/** Curva: preamp en dB, preamp automático (0/1) y bandas [tipo, Hz, dB, Q, encendida] */
type WireBand = [number, number, number, number, 0 | 1]

const round = (v: number, d: number): number => +v.toFixed(d)

function toBase64Url(text: string): string {
  let bin = ''
  for (const byte of new TextEncoder().encode(text)) bin += String.fromCharCode(byte)
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function fromBase64Url(b64: string): string {
  const bin = atob(b64.replace(/-/g, '+').replace(/_/g, '/'))
  return new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)))
}

export function cleanName(name: string): string {
  return [...name.replace(/\p{Cc}/gu, '').trim()].slice(0, MAX_NAME).join('')
}

const wireCurve = (c: Curve): { p: number; a: 0 | 1; b: WireBand[] } => ({
  p: round(c.preamp, 1),
  a: c.autoPreamp ? 1 : 0,
  b: c.bands.map((b) => [KIND_CODES.indexOf(b.kind), round(b.freq, 1), round(b.gain, 2), round(b.q, 3), b.on ? 1 : 0])
})

/** Solo las partes de la sección, cada una con `on` y sus valores */
const wireDsp = (d: Dsp, parts: readonly Part[]): Record<string, unknown> => Object.fromEntries(parts.map((k) => [k, { ...d[k] }]))

export function encodeShare(p: Shared): string {
  const body: Record<string, unknown> = { s: p.section, n: cleanName(p.name) }
  if (p.section === 'eq' || p.section === 'mic') Object.assign(body, wireCurve(p.curve))
  if (p.section === 'sound' || p.section === 'phones' || p.section === 'mic') body.d = wireDsp(p.dsp, SECTION_PARTS[p.section])
  if (p.section === 'studio') body.d = { ...p.studio }
  return SHARE_PREFIX + toBase64Url(JSON.stringify(body))
}

// ——— Lectura estricta ———

const num = (v: unknown, lo: number, hi: number): v is number => typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi
const bit = (v: unknown): v is 0 | 1 => v === 0 || v === 1
const obj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
/** Exactamente estas claves, ni una más ni una menos */
const keysAre = (o: Record<string, unknown>, keys: readonly string[]): boolean => {
  const k = Object.keys(o)
  return k.length === keys.length && keys.every((x) => k.includes(x))
}

function readCurve(p: unknown, a: unknown, b: unknown): Curve | null {
  if (!num(p, -30, 12) || !bit(a) || !Array.isArray(b) || b.length > MAX_BANDS) return null
  const bands: Band[] = []
  for (const x of b) {
    if (!Array.isArray(x) || x.length !== 5) return null
    const [k, f, g, q, on] = x as unknown[]
    if (!Number.isInteger(k) || !num(k, 0, KIND_CODES.length - 1) || !num(f, MIN_F, MAX_F) || !num(g, MIN_G, MAX_G) || !num(q, MIN_Q, MAX_Q) || !bit(on)) return null
    bands.push({ kind: KIND_CODES[k], freq: f, gain: g, q, on: on === 1 })
  }
  return { preamp: p, autoPreamp: a === 1, bands, preset: null }
}

function readDsp(d: unknown, parts: readonly Part[]): Dsp | null {
  if (!obj(d) || !keysAre(d, parts)) return null
  const out = structuredClone(DSP_DEFAULT)
  for (const part of parts) {
    const v = d[part]
    const ranges = DSP_RANGES[part] as Record<string, [number, number]>
    if (!obj(v) || !keysAre(v, ['on', ...Object.keys(ranges)]) || typeof v.on !== 'boolean') return null
    if (!Object.entries(ranges).every(([k, [lo, hi]]) => num(v[k], lo, hi))) return null
    Object.assign(out[part], v)
  }
  // Crossfeed y altavoces virtuales nunca a la vez (se cruzaría dos veces)
  if (out.crossfeed.on && out.speakers.on) return null
  return out
}

function readStudio(d: unknown): Studio | null {
  if (!obj(d) || !keysAre(d, ['listen', 'flipL', 'flipR', 'swap', 'sim'])) return null
  const { listen, flipL, flipR, swap, sim } = d
  if (!LISTENS.includes(listen as Listen) || typeof flipL !== 'boolean' || typeof flipR !== 'boolean' || typeof swap !== 'boolean') return null
  if (sim !== null && !SIMS.includes(sim as Sim)) return null
  return { listen: listen as Listen, flipL, flipR, swap, sim: sim as Sim | null }
}

/** Lee un código (con espacios o saltos de línea en medio, como al copiarlo de un chat). `null` si no vale. */
export function decodeShare(code: string): Shared | null {
  const clean = code.replace(/\s+/g, '')
  const legacy = clean.startsWith(LEGACY_PREFIX)
  if (clean.length > MAX_CODE || (!legacy && !clean.startsWith(SHARE_PREFIX))) return null
  const body = clean.slice(SHARE_PREFIX.length)
  if (!/^[A-Za-z0-9_-]+$/.test(body)) return null
  let w: unknown
  try {
    w = JSON.parse(fromBase64Url(body))
  } catch {
    return null
  }
  if (!obj(w)) return null
  // Los primeros códigos no llevan sección: son del ecualizador
  const section = legacy ? 'eq' : w.s
  if (!SECTIONS.includes(section as ShareSection)) return null
  const head = legacy ? ['n'] : ['s', 'n']
  const fields: Record<ShareSection, string[]> = { eq: ['p', 'a', 'b'], mic: ['p', 'a', 'b', 'd'], sound: ['d'], phones: ['d'], studio: ['d'] }
  if (!keysAre(w, [...head, ...fields[section as ShareSection]])) return null
  const n = w.n
  if (typeof n !== 'string' || n.length > MAX_NAME * 4) return null
  const name = cleanName(n)
  if (!name) return null
  switch (section as ShareSection) {
    case 'eq': {
      const curve = readCurve(w.p, w.a, w.b)
      return curve && { section: 'eq', name, curve }
    }
    case 'mic': {
      const curve = readCurve(w.p, w.a, w.b)
      const dsp = readDsp(w.d, SECTION_PARTS.mic)
      return curve && dsp && { section: 'mic', name, curve, dsp }
    }
    case 'sound':
    case 'phones': {
      const s = section as 'sound' | 'phones'
      const dsp = readDsp(w.d, SECTION_PARTS[s])
      return dsp && { section: s, name, dsp }
    }
    case 'studio': {
      const studio = readStudio(w.d)
      return studio && { section: 'studio', name, studio }
    }
  }
}
