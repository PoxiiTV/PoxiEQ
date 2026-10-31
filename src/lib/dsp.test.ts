// node --test src/lib/dsp.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Dsp } from './api'
import { dspActive, soundOf, SOUNDS, withHeadphones, withSound } from './dsp.ts'

// Lo mismo que `Settings::default()` en Rust
const OFF: Dsp = {
  limiter: { on: false, ceiling: -1 },
  comp: { on: false, threshold: -24, ratio: 3, attack: 10, release: 150, knee: 6, makeup: 0 },
  leveler: { on: false, target: -14, maxGain: 12 },
  bass: { on: false, freq: 100, amount: 0.4 },
  dialog: { on: false, amount: 4 },
  crossfeed: { on: false, freq: 700, feed: 4.5 },
  speakers: { on: false, amount: 1 }
}

test('cada preset de sonoridad se reconoce a sí mismo', () => {
  assert.equal(soundOf(OFF), 'natural')
  assert.equal(dspActive(OFF), false)
  for (const s of SOUNDS) assert.equal(soundOf(withSound(OFF, s)), s)
  assert.equal(withSound(OFF, 'night').leveler.target, -23)
})

test('los presets no tocan los realces y Natural lo apaga todo', () => {
  const extras = { ...OFF, crossfeed: { ...OFF.crossfeed, on: true }, bass: { ...OFF.bass, on: true, amount: 0.7 }, dialog: { ...OFF.dialog, on: true } }
  const loud = withSound(extras, 'loud')
  assert.equal(loud.crossfeed.on, true)
  assert.equal(loud.bass.on, true)
  // Con realces encendidos y la cadena de volumen apagada ya no es «Natural»
  assert.equal(soundOf(withSound(extras, 'natural')), 'natural')
  assert.equal(soundOf({ ...OFF, dialog: { ...OFF.dialog, on: true } }), null)
  const natural = withSound(loud, 'natural')
  assert.equal(soundOf(natural), 'natural')
  assert.equal(dspActive(natural), false)
  // Apagar no borra los ajustes: al volver a encender cada realce sigue como estaba
  assert.equal(natural.bass.amount, 0.7)
  assert.equal(natural.leveler.target, -9)
})

test('un ajuste a mano deja el preset en personalizado', () => {
  const s = withSound(OFF, 'streaming')
  assert.equal(soundOf({ ...s, comp: { ...s.comp, ratio: 2.5 } }), null)
})

test('crossfeed y altavoces virtuales nunca a la vez', () => {
  const x = withHeadphones(OFF, 'crossfeed', true)
  assert.equal(x.crossfeed.on, true)
  const v = withHeadphones(x, 'speakers', true)
  assert.deepEqual([v.crossfeed.on, v.speakers.on], [false, true])
  assert.equal(dspActive(v), true)
  // Apagar uno no enciende el otro
  const off = withHeadphones(v, 'speakers', false)
  assert.deepEqual([off.crossfeed.on, off.speakers.on], [false, false])
  assert.equal(withSound(v, 'natural').speakers.on, false)
})
