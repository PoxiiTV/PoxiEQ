import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readEqFile, toEasyEffects, toParametric, toRew } from './formats.ts'
import type { Curve } from './eq.ts'

const curve: Curve = {
  preamp: -4.5,
  autoPreamp: false,
  preset: null,
  bands: [
    { kind: 'lowShelf', freq: 105, gain: 4, q: 0.7, on: true },
    { kind: 'peak', freq: 3150, gain: -2.5, q: 2.1, on: true },
    { kind: 'highPass', freq: 25, gain: 0, q: 0.707, on: true },
    { kind: 'peak', freq: 8000, gain: 9, q: 1, on: false }
  ]
}
const on = curve.bands.filter((b) => b.on)

test('exportar e importar da lo mismo en los tres formatos', () => {
  for (const text of [toParametric(curve), toRew(curve), toEasyEffects(curve)]) {
    const r = readEqFile(text)
    assert.ok(r && r.kind === 'bands', text)
    assert.equal(r.preamp, -4.5)
    assert.deepEqual(r.bands, on)
  }
})

test('GraphicEQ se lee como puntos', () => {
  const r = readEqFile('GraphicEQ: 20 -3.5; 21 -3.4; 1000 0; 20000 -6')
  assert.deepEqual(r, { kind: 'graphic', points: [[20, -3.5], [21, -3.4], [1000, 0], [20000, -6]] })
  assert.equal(readEqFile('GraphicEQ: 20 -3.5; 19 0'), null, 'frecuencias desordenadas')
  assert.equal(readEqFile('GraphicEQ: 20 -99; 40 0'), null, 'fuera de rango')
})

test('EasyEffects fuera de rango se rechaza entero', () => {
  const ee = JSON.parse(toEasyEffects(curve))
  ee.output['equalizer#0'].left.band1.gain = 60
  assert.equal(readEqFile(JSON.stringify(ee)), null)
  ee.output['equalizer#0'].left.band1.gain = -2
  ee.output['equalizer#0'].left.band1.type = 'Raro'
  assert.equal(readEqFile(JSON.stringify(ee)), null)
  assert.equal(readEqFile('{"nada": 1}'), null)
  assert.equal(readEqFile('hola'), null)
})
