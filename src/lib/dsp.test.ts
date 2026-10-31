// node --test src/lib/dsp.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Dsp } from './api'
import { dspActive, soundOf, SOUNDS, withSound } from './dsp.ts'

// Lo mismo que `Settings::default()` en Rust
const OFF: Dsp = {
  limiter: { on: false, ceiling: -1 },
  comp: { on: false, threshold: -24, ratio: 3, attack: 10, release: 150, knee: 6, makeup: 0 },
  leveler: { on: false, target: -14, maxGain: 12 },
  bass: { on: false, freq: 100, amount: 0.4 },
  dialog: { on: false, amount: 4 },
  crossfeed: { on: false, freq: 700, feed: 4.5 }
}

test('cada preset de sonoridad se reconoce a sí mismo', () => {
  assert.equal(soundOf(OFF), 'natural')
  assert.equal(dspActive(OFF), false)
  for (const s of SOUNDS) assert.equal(soundOf(withSound(OFF, s)), s)
  assert.equal(withSound(OFF, 'night').leveler.target, -23)
})

test('los presets no tocan los realces y Natural apaga solo el volumen', () => {
  const extras = { ...OFF, crossfeed: { ...OFF.crossfeed, on: true } }
  const loud = withSound(extras, 'loud')
  assert.equal(loud.crossfeed.on, true)
  const natural = withSound(loud, 'natural')
  assert.equal(natural.crossfeed.on, true)
  assert.equal(soundOf(natural), 'natural')
  assert.equal(dspActive(natural), true)
})

test('un ajuste a mano deja el preset en personalizado', () => {
  const s = withSound(OFF, 'streaming')
  assert.equal(soundOf({ ...s, comp: { ...s.comp, ratio: 2.5 } }), null)
})
