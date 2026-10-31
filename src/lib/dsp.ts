// Presets de sonoridad de PoxiDSP: solo tocan la cadena de volumen (nivelador, compresor y limitador);
// graves, diálogos y crossfeed van aparte y se quedan como estén.
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

export function withSound(d: Dsp, s: Sound): Dsp {
  if (s !== 'natural') return { ...d, ...structuredClone(PRESETS[s]) }
  return { ...d, leveler: { ...d.leveler, on: false }, comp: { ...d.comp, on: false }, limiter: { ...d.limiter, on: false } }
}

const same = (a: object, b: object): boolean => Object.entries(a).every(([k, v]) => (b as Record<string, unknown>)[k] === v)

/** Preset de sonoridad que coincide con los ajustes (`null` = personalizado) */
export function soundOf(d: Dsp): Sound | null {
  if (!d.leveler.on && !d.comp.on && !d.limiter.on) return 'natural'
  return (Object.keys(PRESETS) as (keyof typeof PRESETS)[]).find((s) => (['leveler', 'comp', 'limiter'] as const).every((k) => same(PRESETS[s][k], d[k]))) ?? null
}

export const dspActive = (d: Dsp): boolean => [d.limiter, d.comp, d.leveler, d.bass, d.dialog, d.crossfeed].some((p) => p.on)
