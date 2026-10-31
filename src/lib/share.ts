// Compartir presets: un código de texto que se puede pegar en un chat (`PXEQ1.` + base64url de un JSON
// compacto) y el mismo código dentro de un archivo `.poxieq`. Al importar se valida todo y con rangos
// estrictos: un código fuera de rango no se «arregla», se rechaza.
import { MAX_BANDS, MAX_F, MAX_G, MAX_Q, MIN_F, MIN_G, MIN_Q, type Band, type Curve, type Kind } from './eq.ts'

/** Versión en el prefijo: si el formato cambia, `PXEQ2.` y las versiones viejas lo rechazan limpio */
export const SHARE_PREFIX = 'PXEQ1.'
export const MAX_CODE = 8192
export const MAX_FILE = 16 * 1024
export const MAX_NAME = 48

// Orden fijo (forma parte del formato): no depende del orden de los menús
const KIND_CODES: Kind[] = ['peak', 'lowShelf', 'highShelf', 'lowPass', 'highPass']

interface Wire {
  /** nombre */
  n: string
  /** preamp en dB */
  p: number
  /** preamp automático (0/1) */
  a: 0 | 1
  /** bandas: [tipo, Hz, dB, Q, encendida] */
  b: [number, number, number, number, 0 | 1][]
}

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

export function encodePreset(name: string, c: Curve): string {
  const wire: Wire = {
    n: cleanName(name),
    p: round(c.preamp, 1),
    a: c.autoPreamp ? 1 : 0,
    b: c.bands.map((b) => [KIND_CODES.indexOf(b.kind), round(b.freq, 1), round(b.gain, 2), round(b.q, 3), b.on ? 1 : 0])
  }
  return SHARE_PREFIX + toBase64Url(JSON.stringify(wire))
}

const num = (v: unknown, lo: number, hi: number): v is number => typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi
const bit = (v: unknown): v is 0 | 1 => v === 0 || v === 1

/** Lee un código (con espacios o saltos de línea en medio, como al copiarlo de un chat). `null` si no vale. */
export function decodePreset(code: string): { name: string; curve: Curve } | null {
  const clean = code.replace(/\s+/g, '')
  if (clean.length > MAX_CODE || !clean.startsWith(SHARE_PREFIX)) return null
  const body = clean.slice(SHARE_PREFIX.length)
  if (!/^[A-Za-z0-9_-]+$/.test(body)) return null
  let w: unknown
  try {
    w = JSON.parse(fromBase64Url(body))
  } catch {
    return null
  }
  if (!w || typeof w !== 'object' || Array.isArray(w)) return null
  const { n, p, a, b } = w as Record<string, unknown>
  if (Object.keys(w).some((k) => !['n', 'p', 'a', 'b'].includes(k))) return null
  if (typeof n !== 'string' || !num(p, -30, 12) || !bit(a) || !Array.isArray(b) || b.length > MAX_BANDS) return null
  const name = cleanName(n)
  if (!name || n.length > MAX_NAME * 4) return null
  const bands: Band[] = []
  for (const x of b) {
    if (!Array.isArray(x) || x.length !== 5) return null
    const [k, f, g, q, on] = x as unknown[]
    if (!Number.isInteger(k) || !num(k, 0, KIND_CODES.length - 1) || !num(f, MIN_F, MAX_F) || !num(g, MIN_G, MAX_G) || !num(q, MIN_Q, MAX_Q) || !bit(on)) return null
    bands.push({ kind: KIND_CODES[k], freq: f, gain: g, q, on: on === 1 })
  }
  return { name, curve: { preamp: p, autoPreamp: a === 1, bands, preset: null } }
}
