// node --test src/lib/share.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { DSP_DEFAULT, onlyParts, SECTION_PARTS } from './dsp.ts'
import { builtinCurve, type Curve } from './eq.ts'
import { decodeShare, encodeShare, MAX_CODE, SHARE_PREFIX, type Shared } from './share.ts'

const code = (prefix: string, o: unknown): string => prefix + Buffer.from(JSON.stringify(o)).toString('base64url')
const v1 = (o: unknown): string => code('PXEQ1.', o)
const v2 = (o: unknown): string => code(SHARE_PREFIX, o)
const GOOD = { n: 'Mis DT 770', p: -3, a: 0, b: [[0, 1000, 3, 1.41, 1]] }

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

const night = {
  ...DSP_DEFAULT,
  leveler: { on: true, target: -23, maxGain: 18 },
  comp: { on: true, threshold: -32, ratio: 6, attack: 2, release: 300, knee: 10, makeup: 0 },
  bassBoost: { on: true, amount: 0.65 },
  crossfeed: { on: true, freq: 700, feed: 6 }
}

test('ida y vuelta en cada sección, sin perder nada y solo con lo suyo', () => {
  const cases: Shared[] = [
    { section: 'eq', name: 'Noche de pelis 🎬 ñ', curve },
    { section: 'sound', name: 'Noche', dsp: night },
    { section: 'phones', name: 'Cruce', dsp: night },
    { section: 'studio', name: 'Mono', studio: { listen: 'mono', flipL: true, flipR: false, swap: false, sim: 'phone' } },
    { section: 'mic', name: 'Voz', curve, dsp: { ...DSP_DEFAULT, gate: { on: true, threshold: -45, range: 20, attack: 1, hold: 200, release: 150 }, leveler: night.leveler } }
  ]
  for (const c of cases) {
    const text = encodeShare(c)
    assert.match(text, /^PXEQ2\.[A-Za-z0-9_-]+$/)
    const back = decodeShare(text)
    assert.ok(back, c.section)
    assert.equal(back.section, c.section)
    assert.equal(back.name, c.name)
    if ('curve' in c && 'curve' in back) assert.deepEqual(back.curve, { ...c.curve, preset: null })
    if ('studio' in c && 'studio' in back) assert.deepEqual(back.studio, c.studio)
    // Del PoxiDSP solo viaja lo de la sección: el resto llega apagado y de serie
    if ('dsp' in c && 'dsp' in back && c.section !== 'eq' && c.section !== 'studio') assert.deepEqual(back.dsp, onlyParts(c.dsp, SECTION_PARTS[c.section]))
  }
  assert.equal((decodeShare(encodeShare(cases[1])) as { dsp: typeof night }).dsp.crossfeed.on, false)
  assert.equal((decodeShare(encodeShare(cases[2])) as { dsp: typeof night }).dsp.leveler.on, false)
  // 10 bandas caben de sobra en un mensaje
  assert.ok(encodeShare({ section: 'eq', name: 'Plano', curve: builtinCurve('b:flat') }).length < 400)
})

test('los códigos de ecualizador de antes (PXEQ1) siguen valiendo', () => {
  assert.deepEqual(decodeShare(v1(GOOD)), { section: 'eq', name: 'Mis DT 770', curve: { preamp: -3, autoPreamp: false, bands: [{ kind: 'peak', freq: 1000, gain: 3, q: 1.41, on: true }], preset: null } })
  // Pero con sección no (en la versión 1 no existía) y en la 2 la sección es obligatoria
  assert.equal(decodeShare(v1({ ...GOOD, s: 'eq' })), null)
  assert.equal(decodeShare(v2(GOOD)), null)
  assert.ok(decodeShare(v2({ s: 'eq', ...GOOD })))
})

test('se puede pegar con espacios y saltos de línea', () => {
  const text = encodeShare({ section: 'eq', name: 'A', curve: builtinCurve('b:bass') })
  const messy = `  ${text.slice(0, 20)}\n ${text.slice(20)}  `
  const back = decodeShare(messy)
  assert.equal(back?.section === 'eq' && back.curve.bands.length, 10)
})

test('rechaza lo que no es un preset válido', () => {
  const badEq: unknown[] = [
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
  for (const o of badEq) {
    assert.equal(decodeShare(v1(o)), null, JSON.stringify(o))
    if (o && typeof o === 'object' && !Array.isArray(o)) assert.equal(decodeShare(v2({ s: 'eq', ...o })), null, JSON.stringify(o))
  }
  const sound = JSON.parse(JSON.stringify({ s: 'sound', n: 'S', d: Object.fromEntries(SECTION_PARTS.sound.map((k) => [k, DSP_DEFAULT[k]])) }))
  assert.ok(decodeShare(v2(sound)))
  const with_ = (patch: (o: typeof sound) => void): string => {
    const o = structuredClone(sound)
    patch(o)
    return v2(o)
  }
  const badSections = [
    with_((o) => (o.s = 'loud')),
    with_((o) => (o.s = 'phones')),
    with_((o) => delete o.d.limiter),
    with_((o) => (o.d.crossfeed = DSP_DEFAULT.crossfeed)),
    with_((o) => (o.d.comp.ratio = 25)),
    with_((o) => (o.d.comp.ratio = '3')),
    with_((o) => (o.d.leveler.on = 1)),
    with_((o) => (o.d.leveler.extra = 1)),
    with_((o) => delete o.d.bassBoost.amount),
    with_((o) => (o.d = [])),
    with_((o) => (o.p = 0)),
    v2({ s: 'phones', n: 'X', d: { crossfeed: { ...DSP_DEFAULT.crossfeed, on: true }, speakers: { ...DSP_DEFAULT.speakers, on: true } } }),
    v2({ s: 'studio', n: 'X', d: { listen: 'surround', flipL: false, flipR: false, swap: false, sim: null } }),
    v2({ s: 'studio', n: 'X', d: { listen: 'mono', flipL: 0, flipR: false, swap: false, sim: null } }),
    v2({ s: 'studio', n: 'X', d: { listen: 'mono', flipL: false, flipR: false, swap: false, sim: 'cinema' } }),
    v2({ s: 'studio', n: 'X', d: { listen: 'mono', flipL: false, flipR: false, swap: false } }),
    v2({ s: 'mic', ...GOOD }),
    v2({ s: 'mic', ...GOOD, d: { gate: DSP_DEFAULT.gate } })
  ]
  for (const c of badSections) assert.equal(decodeShare(c), null, c)
  assert.equal(decodeShare(''), null)
  assert.equal(decodeShare('hola'), null)
  assert.equal(decodeShare('PXEQ3.' + v2(sound).slice(6)), null)
  assert.equal(decodeShare(SHARE_PREFIX + '!!!'), null)
  assert.equal(decodeShare(SHARE_PREFIX + 'e30'), null, '{} sin campos')
  assert.equal(decodeShare(SHARE_PREFIX + Buffer.from([0xff, 0xfe]).toString('base64url')), null, 'UTF-8 roto')
  assert.equal(decodeShare(SHARE_PREFIX + 'A'.repeat(MAX_CODE)), null, 'demasiado largo')
})

test('el nombre se limpia y se acota', () => {
  assert.equal(decodeShare(v1({ ...GOOD, n: '  Mi\u0000 preset  ' }))?.name, 'Mi preset')
  assert.equal(decodeShare(v2({ s: 'eq', ...GOOD, n: 'x'.repeat(60) }))?.name.length, 48)
})
