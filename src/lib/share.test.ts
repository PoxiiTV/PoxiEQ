// node --test src/lib/share.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { builtinCurve, type Curve } from './eq.ts'
import { decodePreset, encodePreset, MAX_CODE, SHARE_PREFIX } from './share.ts'

const b64 = (o: unknown): string => SHARE_PREFIX + Buffer.from(JSON.stringify(o)).toString('base64url')
const GOOD = { n: 'Mis DT 770', p: -3, a: 0, b: [[0, 1000, 3, 1.41, 1]] }

test('ida y vuelta sin perder nada', () => {
  const curve: Curve = {
    preamp: -6.4,
    autoPreamp: false,
    bands: [
      { kind: 'lowShelf', freq: 105, gain: 5.5, q: 0.7, on: true },
      { kind: 'peak', freq: 2950.5, gain: -2.25, q: 1.8, on: false },
      { kind: 'highPass', freq: 20, gain: 0, q: 0.707, on: true }
    ],
    preset: 'u:abc'
  }
  const code = encodePreset('Noche de pelis 🎬 ñ', curve)
  assert.match(code, /^PXEQ1\.[A-Za-z0-9_-]+$/)
  const back = decodePreset(code)
  assert.deepEqual(back, { name: 'Noche de pelis 🎬 ñ', curve: { ...curve, preset: null } })
  // 10 bandas caben de sobra en un mensaje
  assert.ok(encodePreset('Plano', builtinCurve('b:flat')).length < 400)
})

test('se puede pegar con espacios y saltos de línea', () => {
  const code = encodePreset('A', builtinCurve('b:bass'))
  const messy = `  ${code.slice(0, 20)}\n ${code.slice(20)}  `
  assert.equal(decodePreset(messy)?.curve.bands.length, 10)
})

test('rechaza lo que no es un preset válido', () => {
  assert.ok(decodePreset(b64(GOOD)))
  const bad: unknown[] = [
    { ...GOOD, p: 13 },
    { ...GOOD, p: '0' },
    { ...GOOD, a: true },
    { ...GOOD, n: '' },
    { ...GOOD, n: '\u0000\u0007' },
    { ...GOOD, b: [[5, 1000, 3, 1, 1]] },
    { ...GOOD, b: [[0.5, 1000, 3, 1, 1]] },
    { ...GOOD, b: [[0, 10, 3, 1, 1]] },
    { ...GOOD, b: [[0, 1000, 30, 1, 1]] },
    { ...GOOD, b: [[0, 1000, 3, 0, 1]] },
    { ...GOOD, b: [[0, 1000, 3, 1, 2]] },
    { ...GOOD, b: [[0, 1000, 3, 1]] },
    { ...GOOD, b: [{ k: 0 }] },
    { ...GOOD, b: Array.from({ length: 33 }, () => [0, 1000, 0, 1, 1]) },
    { ...GOOD, extra: 1 },
    [GOOD],
    null
  ]
  for (const o of bad) assert.equal(decodePreset(b64(o)), null, JSON.stringify(o))
  assert.equal(decodePreset(''), null)
  assert.equal(decodePreset('hola'), null)
  assert.equal(decodePreset('PXEQ2.' + b64(GOOD).slice(6)), null)
  assert.equal(decodePreset(SHARE_PREFIX + '!!!'), null)
  assert.equal(decodePreset(SHARE_PREFIX + 'e30'), null, '{} sin campos')
  assert.equal(decodePreset(SHARE_PREFIX + Buffer.from([0xff, 0xfe]).toString('base64url')), null, 'UTF-8 roto')
  assert.equal(decodePreset(SHARE_PREFIX + 'A'.repeat(MAX_CODE)), null, 'demasiado largo')
})

test('el nombre se limpia y se acota', () => {
  const r = decodePreset(b64({ ...GOOD, n: '  Mi\u0000 preset  ' }))
  assert.equal(r?.name, 'Mi preset')
  assert.equal(decodePreset(b64({ ...GOOD, n: 'x'.repeat(60) }))?.name.length, 48)
})
