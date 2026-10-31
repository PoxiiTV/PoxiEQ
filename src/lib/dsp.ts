// Presets de sonoridad de PoxiDSP. Streaming, Fuerte y Noche solo tocan la cadena de volumen (nivelador,
// compresor y limitador) y dejan los realces como estén; Natural lo apaga todo: el sonido tal cual llega.
import type { Dsp } from './api'

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

const PARTS = ['limiter', 'comp', 'leveler', 'bass', 'dialog', 'crossfeed', 'speakers'] as const

export function withSound(d: Dsp, s: Sound): Dsp {
  if (s !== 'natural') return { ...d, ...structuredClone(PRESETS[s]) }
  return Object.fromEntries(PARTS.map((k) => [k, { ...d[k], on: false }])) as unknown as Dsp
}

const same = (a: object, b: object): boolean => Object.entries(a).every(([k, v]) => (b as Record<string, unknown>)[k] === v)

/** Preset de sonoridad que coincide con los ajustes (`null` = personalizado) */
export function soundOf(d: Dsp): Sound | null {
  if (!dspActive(d)) return 'natural'
  return (Object.keys(PRESETS) as (keyof typeof PRESETS)[]).find((s) => (['leveler', 'comp', 'limiter'] as const).every((k) => same(PRESETS[s][k], d[k]))) ?? null
}

/** Crossfeed y altavoces virtuales son dos formas de lo mismo: encender uno apaga el otro */
export function withHeadphones(d: Dsp, part: 'crossfeed' | 'speakers', on: boolean): Dsp {
  const other = part === 'crossfeed' ? 'speakers' : 'crossfeed'
  return { ...d, [part]: { ...d[part], on }, [other]: { ...d[other], on: on ? false : d[other].on } }
}

export function dspActive(d: Dsp): boolean {
  return PARTS.some((k) => d[k].on)
}
