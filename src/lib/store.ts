import { create } from 'zustand'
import { api, type AppInfo, type Config, type Device, type EngineStatus, type Preset } from './api'
import { autoPreamp, BUILTIN, builtinCurve, type Curve } from './eq'
import { Grouper, History } from './history'
import { errorText, t } from '../i18n'

export interface Toast {
  id: number
  kind: 'ok' | 'err' | 'info'
  text: string
  secs?: number
  action?: { label: string; run: () => void }
}

/** Paso de la activación del motor que se enseña mientras dura */
export type SetupStep = 'download' | 'install' | 'check' | null

interface State {
  app: AppInfo | null
  config: Config | null
  engine: EngineStatus | null
  setup: SetupStep
  /** Banda elegida en la curva (índice) */
  selected: number | null
  /** Pestaña del editor: curva o dinámica */
  tab: 'eq' | 'dsp'
  settingsOpen: boolean
  modal: 'autoeq' | 'savePreset' | 'import' | null
  /** Preset que se está compartiendo (ventana de compartir abierta) */
  sharing: Preset | null
  /** Código que llega a la ventana de importar (archivo arrastrado) */
  importCode: string
  /** Hay algo que deshacer / rehacer */
  history: { undo: boolean; redo: boolean }
  toasts: Toast[]
}

export const useStore = create<State>(() => ({
  app: null,
  config: null,
  engine: null,
  setup: null,
  selected: null,
  tab: 'eq',
  settingsOpen: false,
  modal: null,
  sharing: null,
  importCode: '',
  history: { undo: false, redo: false },
  toasts: []
}))

export const getState = useStore.getState
export const setState = useStore.setState

// ——— Avisos ———

let toastId = 0
export function toast(text: string, kind: Toast['kind'] = 'ok', extra: Partial<Toast> = {}): void {
  const id = ++toastId
  const secs = extra.secs ?? (kind === 'err' ? 6 : 3)
  setState((s) => ({ toasts: [...s.toasts.slice(-2), { id, kind, text, secs, ...extra }] }))
  setTimeout(() => dismiss(id), secs * 1000)
}

export function dismiss(id: number): void {
  setState((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) }))
}

// ——— Configuración: se pinta al momento y se guarda (y aplica al sonido) en segundo plano ———

let pending: Config | null = null
let saving = false

async function flush(): Promise<void> {
  if (saving) return
  saving = true
  try {
    while (pending) {
      const cfg = pending
      pending = null
      await api.configSet(cfg).catch((e: unknown) => toast(errorText(e), 'err'))
    }
  } finally {
    saving = false
  }
}

/** Arrastrando la curva llegan decenas de cambios por segundo: solo se envía el último pendiente.
 *  Los cambios de curva y de dinámica quedan en el historial (salvo `record = false`: deshacer/rehacer). */
export function updateConfig(patch: Partial<Config>, record = true): void {
  const cur = getState().config
  if (!cur) return
  const touches = (patch.curves && patch.curves !== cur.curves) || (patch.dsp && patch.dsp !== cur.dsp)
  if (record && touches) {
    history.record({ curves: cur.curves, dsp: cur.dsp }, grouper.next(performance.now()))
    syncHistory()
  }
  const next = { ...cur, ...patch }
  setState({ config: next })
  pending = next
  void flush()
}

// ——— Deshacer / rehacer (curvas y dinámica) ———

type Snapshot = Pick<Config, 'curves' | 'dsp'>
const history = new History<Snapshot>()
/** Agrupa los cambios por gesto: lo alimentan los eventos de puntero de toda la ventana (`main.tsx`) */
export const grouper = new Grouper()

function syncHistory(): void {
  const h = getState().history
  if (h.undo !== history.canUndo || h.redo !== history.canRedo) setState({ history: { undo: history.canUndo, redo: history.canRedo } })
}

function travel(dir: 'undo' | 'redo'): void {
  const cfg = getState().config
  if (!cfg) return
  const snap = history[dir]({ curves: cfg.curves, dsp: cfg.dsp })
  if (!snap) return
  const bands = currentCurve({ config: { ...cfg, ...snap }, engine: getState().engine }).bands.length
  const sel = getState().selected
  if (sel !== null && sel >= bands) setState({ selected: null })
  updateConfig(snap, false)
  syncHistory()
}

export const undo = (): void => travel('undo')
export const redo = (): void => travel('redo')

// ——— Dispositivo y curva actuales ———

export function currentDevice(s: Pick<State, 'config' | 'engine'> = getState()): Device | null {
  const list = s.engine?.devices.filter((d) => !d.unplugged) ?? []
  return list.find((d) => d.id === s.config?.device) ?? list.find((d) => d.default) ?? list[0] ?? null
}

// Una sola instancia: los selectores de zustand necesitan devolver la misma referencia si nada cambia
const FLAT: Curve = Object.freeze(builtinCurve('b:flat'))

export function currentCurve(s: Pick<State, 'config' | 'engine'> = getState()): Curve {
  const id = currentDevice(s)?.id
  return (id && s.config?.curves[id]) || FLAT
}

/** Cambia la curva del dispositivo actual. Con «auto», el preamp se recalcula solo. */
export function setCurve(next: Curve, keepPreset = false): void {
  const s = getState()
  const id = currentDevice(s)?.id
  if (!id || !s.config) return
  const curve = { ...next, preset: keepPreset ? next.preset : null }
  if (curve.autoPreamp) curve.preamp = autoPreamp(curve.bands)
  updateConfig({ curves: { ...s.config.curves, [id]: curve } })
}

export function applyPreset(curve: Curve, id: string): void {
  const auto = currentCurve().autoPreamp
  setState({ selected: null })
  setCurve({ ...structuredClone(curve), autoPreamp: auto || curve.autoPreamp, preset: id }, true)
}

const newPresetId = (): string => `u:${Date.now().toString(36)}`

export function savePreset(name: string): void {
  const cfg = getState().config
  if (!cfg) return
  const id = newPresetId()
  const curve = { ...currentCurve(), preset: id }
  const presets: Preset[] = [...cfg.presets, { id, name, curve }]
  const dev = currentDevice()?.id
  updateConfig({ presets, curves: dev ? { ...cfg.curves, [dev]: curve } : cfg.curves })
}

/** Preset que llega compartido: se añade a «Mis presets» y se pone */
export function importPreset(name: string, curve: Curve): void {
  const cfg = getState().config
  if (!cfg) return
  const id = newPresetId()
  updateConfig({ presets: [...cfg.presets, { id, name, curve: { ...curve, preset: id } }] })
  applyPreset(curve, id)
}

/** Todos los presets, de serie (traducidos) y propios, en el orden de la barra lateral */
export function allPresets(cfg: Config | null = getState().config): Preset[] {
  return [...BUILTIN.map((b) => ({ id: b.id, name: t(`presets.builtin.${b.id}`), curve: builtinCurve(b.id) })), ...(cfg?.presets ?? [])]
}

export function deletePreset(id: string): void {
  const cfg = getState().config
  if (cfg) updateConfig({ presets: cfg.presets.filter((p) => p.id !== id) })
}

export async function refreshEngine(): Promise<EngineStatus> {
  const engine = await api.engineStatus()
  setState({ engine })
  return engine
}
