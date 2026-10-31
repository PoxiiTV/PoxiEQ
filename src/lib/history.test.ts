// node --test src/lib/history.test.ts
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Grouper, History } from './history.ts'

test('deshacer y rehacer en orden', () => {
  const h = new History<number>()
  let v = 0
  const set = (n: number): void => {
    h.record(v)
    v = n
  }
  set(1)
  set(2)
  set(3)
  assert.equal(h.canRedo, false)
  v = h.undo(v)!
  v = h.undo(v)!
  assert.equal(v, 1)
  v = h.redo(v)!
  assert.equal(v, 2)
  // Un cambio nuevo tira lo que se podía rehacer
  set(9)
  assert.equal(h.canRedo, false)
  assert.equal(h.redo(v), undefined)
  assert.equal((v = h.undo(v)!), 2)
  assert.equal((v = h.undo(v)!), 1)
  assert.equal((v = h.undo(v)!), 0)
  assert.equal(h.canUndo, false)
  assert.equal(h.undo(v), undefined)
})

test('un arrastre entero es un solo paso', () => {
  const h = new History<number>()
  const g = new Grouper()
  let v = 0
  const set = (n: number, at: number): void => {
    h.record(v, g.next(at))
    v = n
  }
  g.down()
  for (let i = 1; i <= 50; i++) set(i, i * 16)
  g.up()
  // Aunque haya una pausa larga dentro del mismo arrastre, sigue siendo un paso
  g.down()
  set(100, 2000)
  set(101, 9000)
  g.up()
  assert.equal(h.undo(v), 50)
  assert.equal(h.undo(50), 0)
  assert.equal(h.canUndo, false)
})

test('ráfagas de teclado o rueda: juntas si van seguidas, separadas tras una pausa', () => {
  const h = new History<number>()
  const g = new Grouper(400)
  let v = 0
  const set = (n: number, at: number): void => {
    h.record(v, g.next(at))
    v = n
  }
  set(1, 0)
  set(2, 100)
  set(3, 300)
  set(4, 1000)
  // Un clic suelto justo después de una ráfaga es un paso aparte
  g.down()
  set(5, 1100)
  g.up()
  set(6, 1200)
  assert.equal((v = h.undo(v)!), 5)
  assert.equal((v = h.undo(v)!), 4)
  assert.equal((v = h.undo(v)!), 3)
  assert.equal((v = h.undo(v)!), 0)
})

test('tras deshacer, el siguiente cambio no se funde con el anterior', () => {
  const h = new History<number>()
  let v = 0
  h.record(v, 7)
  v = 1
  v = h.undo(v)!
  h.record(v, 7)
  v = 2
  assert.equal(h.undo(v), 0)
})

test('límite de pasos', () => {
  const h = new History<number>(3)
  for (let i = 0; i < 10; i++) h.record(i)
  assert.equal(h.undo(10), 9)
  assert.equal(h.undo(9), 8)
  assert.equal(h.undo(8), 7)
  assert.equal(h.undo(7), undefined)
})
