// node --test src/lib/dsp.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { DSP_DEFAULT as OFF, equal, onlyParts, partsOn, sameParts, SECTION_PARTS, soundOf, SOUNDS, withHeadphones, withParts, withSound } from './dsp.ts'

test('cada preset de sonoridad se reconoce a sí mismo', () => {
  assert.equal(soundOf(OFF), 'natural')
  assert.equal(partsOn(OFF, SECTION_PARTS.sound), false)
  for (const s of SOUNDS) assert.equal(soundOf(withSound(OFF, s)), s)
  assert.equal(withSound(OFF, 'night').leveler.target, -23)
})

test('los presets no tocan los realces y Natural apaga solo lo de «Sonido»', () => {
  const extras = {
    ...OFF,
    crossfeed: { ...OFF.crossfeed, on: true },
    bass: { ...OFF.bass, on: true, amount: 0.7 },
    dialog: { ...OFF.dialog, on: true },
    bassBoost: { on: true, amount: 0.65 }
  }
  const loud = withSound(extras, 'loud')
  assert.equal(loud.crossfeed.on, true)
  assert.equal(loud.bass.on, true)
  assert.equal(loud.bassBoost.on, true)
  // Solo «Graves» encendido ya es personalizado
  assert.equal(soundOf({ ...OFF, bassBoost: { on: true, amount: 0.3 } }), null)
  assert.equal(soundOf(withSound(extras, 'natural')), 'natural')
  assert.equal(soundOf({ ...OFF, dialog: { ...OFF.dialog, on: true } }), null)
  const natural = withSound(loud, 'natural')
  assert.equal(soundOf(natural), 'natural')
  assert.equal(partsOn(natural, SECTION_PARTS.sound), false)
  // Lo de «Cascos» tiene sus propios presets: Natural no lo toca
  assert.equal(natural.crossfeed.on, true)
  // Apagar no borra los ajustes: al volver a encender cada realce sigue como estaba
  assert.equal(natural.bass.amount, 0.7)
  assert.deepEqual(natural.bassBoost, { on: false, amount: 0.65 })
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
  assert.equal(partsOn(v, SECTION_PARTS.phones), true)
  // Apagar uno no enciende el otro
  const off = withHeadphones(v, 'speakers', false)
  assert.deepEqual([off.crossfeed.on, off.speakers.on], [false, false])
  // Lo de cascos va en otra página: con solo eso encendido, «Sonido» sigue en Natural
  assert.equal(soundOf(v), 'natural')
})

test('un preset de sección pone solo sus partes y se reconoce mientras no se toque', () => {
  const mine = withHeadphones(withSound(OFF, 'night'), 'speakers', true)
  const sound = onlyParts(mine, SECTION_PARTS.sound)
  assert.equal(sound.speakers.on, false, 'lo de Cascos no va en un preset de Sonido')
  // Ponerlo en otra salida con crossfeed: cambia la sonoridad y el crossfeed se queda
  const other = withHeadphones(OFF, 'crossfeed', true)
  const applied = withParts(other, sound, SECTION_PARTS.sound)
  assert.equal(soundOf(applied), 'night')
  assert.equal(applied.crossfeed.on, true)
  assert.ok(sameParts(applied, sound, SECTION_PARTS.sound))
  assert.ok(!sameParts(applied, sound, SECTION_PARTS.phones))
  // Retocado, ya no coincide; vuelto a como estaba (deshacer), otra vez sí
  const touched = { ...applied, comp: { ...applied.comp, ratio: 7 } }
  assert.ok(!sameParts(touched, sound, SECTION_PARTS.sound))
  assert.ok(sameParts({ ...touched, comp: { ...touched.comp, ratio: 6 } }, sound, SECTION_PARTS.sound))
  // La copia no comparte objetos con el preset
  applied.leveler.target = -10
  assert.equal(sound.leveler.target, -23)
})

test('igualdad sin depender del orden de las claves', () => {
  assert.ok(equal({ a: 1, b: [1, { c: 2 }] }, { b: [1, { c: 2 }], a: 1 }))
  assert.ok(!equal({ a: 1 }, { a: 1, b: undefined }))
  assert.ok(!equal([1, 2], { 0: 1, 1: 2 }))
  assert.ok(!equal(null, {}))
})
