// node --test src/lib/eq.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { autoPreamp, bandDb, builtinCurve, headroom, parseApo, toApo, type Band } from './eq.ts'

const pk = (gain: number): Band => ({ kind: 'peak', freq: 1000, gain, q: 1.41, on: true })
const near = (a: number, b: number, eps = 0.05): void => assert.ok(Math.abs(a - b) < eps, `${a} ≈ ${b}`)

test('un peaking da su ganancia en su frecuencia y nada lejos de ella', () => {
  near(bandDb(pk(6), 1000), 6)
  near(bandDb(pk(-9), 1000), -9)
  near(bandDb(pk(6), 20), 0, 0.1)
})

test('shelves y pasos', () => {
  near(bandDb({ kind: 'lowShelf', freq: 100, gain: 6, q: 0.707, on: true }, 20), 6, 0.3)
  near(bandDb({ kind: 'highShelf', freq: 8000, gain: -4, q: 0.707, on: true }, 20000), -4, 0.3)
  near(bandDb({ kind: 'lowPass', freq: 1000, gain: 0, q: 0.707, on: true }, 1000), -3, 0.1)
  assert.ok(bandDb({ kind: 'highPass', freq: 100, gain: 0, q: 0.707, on: true }, 20) < -20)
})

test('preamp automático: el pico queda en 0 dB', () => {
  const c = builtinCurve('b:bass')
  assert.ok(c.preamp < 0)
  assert.ok(headroom(c) <= 0.001)
  assert.equal(autoPreamp([pk(-6)]), 0)
})

test('AutoEq ida y vuelta', () => {
  const text = `Preamp: -6.4 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
Filter 2: ON PK Fc 3000 Hz Gain -2.1 dB Q 1.80
Filter 3: ON HPQ Fc 20 Hz Q 0.70
Filter 4: OFF PK Fc 50 Hz Gain 3 dB Q 1
basura`
  const r = parseApo(text)
  assert.ok(r)
  assert.equal(r.preamp, -6.4)
  assert.equal(r.bands.length, 3)
  assert.deepEqual(r.bands[0], { kind: 'lowShelf', freq: 105, gain: 5.5, q: 0.7, on: true })
  assert.equal(r.bands[2].gain, 0)
  const back = parseApo(toApo({ preamp: r.preamp, autoPreamp: false, bands: r.bands, preset: null }))
  assert.deepEqual(back, r)
  assert.equal(parseApo('hola'), null)
})
