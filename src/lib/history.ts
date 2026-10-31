// Historial de deshacer/rehacer. Guarda el estado de ANTES de cada cambio; los cambios de un mismo grupo
// (un arrastre entero, una ráfaga de rueda o de flechas) cuentan como un solo paso.

export class History<T> {
  private past: T[] = []
  private future: T[] = []
  private group: number | null = null
  private readonly limit: number

  constructor(limit = 100) {
    this.limit = limit
  }

  /** `prev`: el estado antes del cambio. `group`: mismo número que el cambio anterior = mismo paso. */
  record(prev: T, group: number | null = null): void {
    const merge = group !== null && group === this.group && this.past.length > 0
    this.group = group
    if (!merge) {
      this.past.push(prev)
      if (this.past.length > this.limit) this.past.shift()
    }
    this.future = []
  }

  /** Devuelve el estado al que volver (o `undefined`); `current` pasa a poder rehacerse */
  undo(current: T): T | undefined {
    const prev = this.past.pop()
    if (prev === undefined) return undefined
    this.future.push(current)
    this.group = null
    return prev
  }

  redo(current: T): T | undefined {
    const next = this.future.pop()
    if (next === undefined) return undefined
    this.past.push(current)
    this.group = null
    return next
  }

  get canUndo(): boolean {
    return this.past.length > 0
  }

  get canRedo(): boolean {
    return this.future.length > 0
  }
}

/**
 * Decide a qué paso pertenece un cambio: mientras hay un botón del ratón pulsado, todo es del mismo gesto;
 * sin ratón (teclado, rueda), los cambios seguidos con menos de `gap` ms entre sí forman una ráfaga.
 */
export class Grouper {
  private gesture = 0
  private held = false
  private burst = 0
  private last = -Infinity
  private readonly gap: number

  constructor(gap = 400) {
    this.gap = gap
  }

  down(): void {
    this.held = true
    this.gesture++
  }

  up(): void {
    this.held = false
  }

  /** Número de grupo del cambio que ocurre en `now` (ms) */
  next(now: number): number {
    const quick = now - this.last < this.gap
    this.last = now
    if (this.held) return this.gesture * 2
    if (!quick) this.burst++
    return -this.burst * 2 - 1
  }
}
