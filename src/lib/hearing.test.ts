// node --test src/lib/hearing.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { curveDb, logFreqs } from './eq.ts'
import { asymmetry, compensation, fitBands, FREQS, hearingCurve, MAX_BOOST, MAX_CUT, NORMAL, ORDER } from './hearing.ts'

/** Umbrales de un oído normal con el 1 kHz en `at` dBFS, más una pérdida por frecuencia */
const ear = (at: number, loss: number[] = FREQS.map(() => 0)): number[] => NORMAL.map((n, i) => at + n + loss[i])

test('un oído normal da una curva plana', () => {
  assert.ok(compensation(ear(-80)).every((g) => g === 0))
  const c = hearingCurve({ left: ear(-80), right: ear(-72) })
  assert.ok(c.bands.every((b) => b.gain === 0))
  assert.equal(c.ears, null, 'que un oído oiga todo 8 dB más alto no cambia la forma')
  assert.equal(c.preamp, 0)
})

test('media compensación, con límites y suave', () => {
  // 10 dB de pérdida a 4 kHz: unos +5 dB repartidos con sus vecinas
  const loss = FREQS.map((f) => (f === 4000 ? 10 : 0))
  const g = compensation(ear(-80, loss))
  const i = FREQS.indexOf(4000)
  assert.equal(g[i], 2.5)
  assert.equal(g[i - 1], 1.3)
  assert.equal(g[i + 1], 1.3)
  // Pérdidas enormes no pasan de los límites
  const big = compensation(ear(-80, FREQS.map((f) => (f >= 8000 ? 60 : f === 250 ? -60 : 0))))
  assert.ok(big.every((v) => v <= MAX_BOOST && v >= MAX_CUT))
  assert.equal(Math.max(...big), MAX_BOOST)
})

test('las campanas pasan por los puntos pedidos', () => {
  const target = [1, 2, 0, -1, 3, 4, 2, 5, 6, 6]
  const bands = fitBands(target)
  FREQS.forEach((f, i) => assert.ok(Math.abs(curveDb(bands, f) - target[i]) < 0.3, `${f} Hz: ${curveDb(bands, f).toFixed(2)} en vez de ${target[i]}`))
})

test('cada oído lo suyo y el preamp evita saturar', () => {
  const left = ear(-80, FREQS.map((f) => (f >= 6000 ? 12 : 0)))
  const right = ear(-80)
  const c = hearingCurve({ left, right })
  assert.ok(c.ears)
  // Las comunes siguen al izquierdo (el que más necesita); el derecho resta en agudos
  assert.ok(curveDb(c.bands, 8000) > 4)
  assert.equal(c.ears.left.length, 0)
  assert.ok(Math.abs(curveDb([...c.bands, ...c.ears.right], 8000)) < 0.5, 'el derecho queda casi plano')
  // Ningún oído pasa del pico de las comunes, que es lo que mira el preamp automático
  const peak = (bands: typeof c.bands): number => Math.max(...logFreqs(400).map((f) => curveDb(bands, f)))
  assert.ok(peak([...c.bands, ...c.ears.right]) <= peak(c.bands) + 0.3)
  assert.ok(c.preamp + peak(c.bands) <= 0.05)
  assert.equal(asymmetry({ left, right }), 12)
})

test('se prueba cada frecuencia una vez, empezando por 1 kHz', () => {
  assert.equal(FREQS[ORDER[0]], 1000)
  assert.deepEqual([...ORDER].sort((a, b) => a - b), FREQS.map((_, i) => i))
})
