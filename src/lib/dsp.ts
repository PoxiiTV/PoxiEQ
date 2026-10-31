// Presets de sonoridad de PoxiDSP. Streaming, Fuerte y Noche solo tocan la cadena de volumen (nivelador,
// compresor y limitador) y dejan los realces como estén; Natural apaga todo lo de «Sonido»: el sonido tal cual
// llega (lo de «Cascos» tiene sus propios presets y no lo toca).
import type { Dsp, Section } from './api'

export type Sound = 'natural' | 'streaming' | 'loud' | 'night'
export const SOUNDS: Sound[] = ['natural', 'streaming', 'loud', 'night']

type Chain = Pick<Dsp, 'leveler' | 'comp' | 'limiter'>

// Umbrales pensados para el nivel al que deja el nivelador (el compresor va detrás de él)
const PRESETS: Record<Exclude<Sound, 'natural'>, Chain> = {
  streaming: {
    leveler: { on: true, target: -14, maxGain: 12 },
    comp: { on: true, threshold: -20, ratio: 2, attack: 20, release: 250, knee: 10, makeup: 0 },
    limiter: { on: true, ceiling: -1 }
  },
  loud: {
    leveler: { on: true, target: -9, maxGain: 15 },
    comp: { on: true, threshold: -16, ratio: 3, attack: 10, release: 150, knee: 8, makeup: 0 },
    limiter: { on: true, ceiling: -1 }
  },
  // Compresión fuerte y rápida: las explosiones bajan y los diálogos se quedan a la vista
  night: {
    leveler: { on: true, target: -23, maxGain: 18 },
    comp: { on: true, threshold: -32, ratio: 6, attack: 2, release: 300, knee: 10, makeup: 0 },
    limiter: { on: true, ceiling: -1 }
  }
}

/** Lo mismo que `Settings::default()` en `dsp/src/lib.rs`: todo apagado (un dispositivo nuevo empieza así) */
export const DSP_DEFAULT: Dsp = {
  limiter: { on: false, ceiling: -1 },
  comp: { on: false, threshold: -24, ratio: 3, attack: 10, release: 150, knee: 6, makeup: 0 },
  leveler: { on: false, target: -14, maxGain: 12 },
  bass: { on: false, freq: 100, amount: 0.4 },
  dialog: { on: false, amount: 4 },
  crossfeed: { on: false, freq: 700, feed: 4.5 },
  speakers: { on: false, amount: 1 },
  gate: { on: false, threshold: -50, range: 30, attack: 1, hold: 150, release: 200 },
  bassBoost: { on: false, amount: 0.5 }
}

/** Rangos de cada valor, los de `Settings::sanitize` (al importar, lo que se sale se rechaza) */
export const DSP_RANGES: { [K in keyof Dsp]: Record<Exclude<keyof Dsp[K], 'on'>, [number, number]> } = {
  limiter: { ceiling: [-12, 0] },
  comp: { threshold: [-60, 0], ratio: [1, 20], attack: [0.1, 200], release: [10, 2000], knee: [0, 24], makeup: [0, 24] },
  leveler: { target: [-36, -6], maxGain: [0, 24] },
  bass: { freq: [40, 200], amount: [0, 1] },
  dialog: { amount: [0, 12] },
  crossfeed: { freq: [300, 2000], feed: [1, 15] },
  speakers: { amount: [0, 1] },
  gate: { threshold: [-90, 0], range: [0, 90], attack: [0.1, 50], hold: [0, 2000], release: [5, 2000] },
  bassBoost: { amount: [0, 1] }
}

export type Part = keyof Dsp

/** Lo que se ajusta en la página «Sonido»: la cadena de volumen y los realces */
export const SOUND_PARTS = ['leveler', 'comp', 'limiter', 'bassBoost', 'bass', 'dialog'] as const satisfies readonly Part[]

/** Partes de PoxiDSP que guarda el preset de cada sección (Estudio no usa PoxiDSP) */
export const SECTION_PARTS: Record<Exclude<Section, 'studio'>, readonly Part[]> = {
  sound: SOUND_PARTS,
  phones: ['crossfeed', 'speakers'],
  mic: ['gate', 'comp']
}

export function withSound(d: Dsp, s: Sound): Dsp {
  if (s !== 'natural') return { ...d, ...structuredClone(PRESETS[s]) }
  return { ...d, ...Object.fromEntries(SOUND_PARTS.map((k) => [k, { ...d[k], on: false }])) }
}

/** Igualdad de valores (objetos planos, listas y primitivos), sin depender del orden de las claves */
export function equal(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (!a || !b || typeof a !== 'object' || typeof b !== 'object' || Array.isArray(a) !== Array.isArray(b)) return false
  const ka = Object.keys(a)
  const kb = Object.keys(b)
  return ka.length === kb.length && ka.every((k) => equal((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]))
}

const same = (a: object, b: object): boolean => Object.entries(a).every(([k, v]) => (b as Record<string, unknown>)[k] === v)

/** Preset de sonoridad que coincide con los ajustes de la página «Sonido» (`null` = personalizado) */
export function soundOf(d: Dsp): Sound | null {
  if (!SOUND_PARTS.some((k) => d[k].on)) return 'natural'
  return (Object.keys(PRESETS) as (keyof typeof PRESETS)[]).find((s) => (['leveler', 'comp', 'limiter'] as const).every((k) => same(PRESETS[s][k], d[k]))) ?? null
}

/** `d` con las partes `parts` copiadas de `from` */
export function withParts(d: Dsp, from: Dsp, parts: readonly Part[]): Dsp {
  return { ...d, ...Object.fromEntries(parts.map((k) => [k, structuredClone(from[k])])) }
}

/** Solo las partes `parts` de `d`; el resto, por defecto (apagado) */
export const onlyParts = (d: Dsp, parts: readonly Part[]): Dsp => withParts(structuredClone(DSP_DEFAULT), d, parts)

export const sameParts = (a: Dsp, b: Dsp, parts: readonly Part[]): boolean => parts.every((k) => equal(a[k], b[k]))

export const partsOn = (d: Dsp, parts: readonly Part[]): boolean => parts.some((k) => d[k].on)

/** Crossfeed y altavoces virtuales son dos formas de lo mismo: encender uno apaga el otro */
export function withHeadphones(d: Dsp, part: 'crossfeed' | 'speakers', on: boolean): Dsp {
  const other = part === 'crossfeed' ? 'speakers' : 'crossfeed'
  return { ...d, [part]: { ...d[part], on }, [other]: { ...d[other], on: on ? false : d[other].on } }
}
