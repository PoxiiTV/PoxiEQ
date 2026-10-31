import { test } from 'node:test'
import assert from 'node:assert/strict'
import { comboFromEvent, keyLabel } from './hotkeys.ts'

const key = (code: string, mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods
})

test('combinación de una pulsación', () => {
  assert.equal(comboFromEvent(key('Numpad0', { ctrlKey: true, altKey: true })), 'control+alt+Numpad0')
  // Mismo orden de modificadores que la forma canónica de Rust
  assert.equal(comboFromEvent(key('KeyM', { ctrlKey: true, shiftKey: true, altKey: true, metaKey: true })), 'shift+control+alt+super+KeyM')
  assert.equal(comboFromEvent(key('F9')), 'F9')
  // Solo modificadores (también AltGr, que llega como AltRight): aún no hay combinación
  for (const c of ['ControlLeft', 'AltRight', 'ShiftLeft', 'MetaLeft', '']) assert.equal(comboFromEvent(key(c, { ctrlKey: true })), null)
})

test('texto de un atajo', () => {
  assert.equal(keyLabel('control+alt+Numpad0'), 'Ctrl+Alt+Num 0')
  assert.equal(keyLabel('shift+control+KeyM'), 'Shift+Ctrl+M')
  assert.equal(keyLabel('super+NumpadAdd'), 'Win+Num +')
  assert.equal(keyLabel('control+Digit2'), 'Ctrl+2')
  assert.equal(keyLabel('alt+F10'), 'Alt+F10')
  assert.equal(keyLabel('control+alt+ArrowUp'), 'Ctrl+Alt+↑')
})
