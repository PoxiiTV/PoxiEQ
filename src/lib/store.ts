import { create } from 'zustand'
import { api, type AppInfo, type Config, type Device, type EngineStatus, type Preset, type Studio } from './api'
import { autoPreamp, BUILTIN, builtinCurve, type Curve } from './eq'
import { Grouper, History } from './history'
import { cleanName } from './share'
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

/** Secciones del menú lateral */
export type Page = 'eq' | 'sound' | 'phones' | 'studio' | 'room' | 'mic' | 'settings'

interface State {
  app: AppInfo | null
  config: Config | null
  engine: EngineStatus | null
  setup: SetupStep
  /** Banda elegida en la curva (índice) */
  selected: number | null
  /** Sección a la vista. En «Micrófono» se edita el micrófono; en el resto, la salida */
  page: Page
  /** Herramientas de estudio puestas ahora (momentáneas, no van a la configuración) */
  studio: Studio
  modal: 'savePreset' | 'import' | 'hearing' | 'room' | null
  /** Preset que se está compartiendo (ventana de compartir abierta) */
  sharing: Preset | null
  /** Código que llega a la ventana de importar (archivo arrastrado) */
  importCode: string
  /** Hay algo que deshacer / rehacer */
  history: { undo: boolean; redo: boolean }
  /** Atajos que Windows no ha dejado registrar (los tiene otro programa) */
  hotkeysFailed: string[]
  toasts: Toast[]
}

export const STUDIO_OFF: Studio = { listen: 'stereo', flipL: false, flipR: false, swap: false, sim: null }

export const useStore = create<State>(() => ({
  app: null,
  config: null,
  engine: null,
  setup: null,
  selected: null,
  page: 'eq',
  studio: STUDIO_OFF,
  modal: null,
  sharing: null,
  importCode: '',
  history: { undo: false, redo: false },
  hotkeysFailed: [],
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
  const touches = (patch.curves && patch.curves !== cur.curves) || (patch.dsp && patch.dsp !== cur.dsp) || (patch.mic && patch.mic !== cur.mic)
  if (record && touches) {
    history.record({ curves: cur.curves, dsp: cur.dsp, mic: cur.mic }, grouper.next(performance.now()))
    syncHistory()
  }
  const next = { ...cur, ...patch }
  setState({ config: next })
  pending = next
  void flush()
}

// ——— Herramientas de estudio ———

export const studioActive = (s: Studio): boolean => s.listen !== 'stereo' || s.flipL || s.flipR || s.swap || s.sim !== null

/** Se pinta al momento y suena en segundo plano; si Rust lo rechaza, vuelve a lo de antes */
export function setStudio(patch: Partial<Studio>): void {
  const before = getState().studio
  const studio = { ...before, ...patch }
  setState({ studio })
  api.studioSet(studio).catch((e: unknown) => {
    setState({ studio: before })
    toast(errorText(e), 'err')
  })
}

// ——— Deshacer / rehacer (curvas, dinámica y micrófono) ———

type Snapshot = Pick<Config, 'curves' | 'dsp' | 'mic'>
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
  const snap = history[dir]({ curves: cfg.curves, dsp: cfg.dsp, mic: cfg.mic })
  if (!snap) return
  const bands = currentCurve({ ...getState(), config: { ...cfg, ...snap } }).bands.length
  const sel = getState().selected
  if (sel !== null && sel >= bands) setState({ selected: null })
  updateConfig(snap, false)
  syncHistory()
}

export const undo = (): void => travel('undo')
export const redo = (): void => travel('redo')

// ——— Sección, dispositivo y curva actuales ———

export function go(page: Page): void {
  setState({ page, selected: null })
}

type Sel = Pick<State, 'config' | 'engine' | 'page'>

/** El elegido si está conectado y, si no, el predeterminado (o el primero) */
function pick(s: Pick<State, 'config' | 'engine'>, input: boolean, id: string | null | undefined): Device | null {
  const list = s.engine?.devices.filter((d) => d.input === input && !d.unplugged) ?? []
  return list.find((d) => d.id === id) ?? list.find((d) => d.default) ?? list[0] ?? null
}

/** Salida que se edita (la de la barra de título) */
export const currentOutput = (s: Pick<State, 'config' | 'engine'> = getState()): Device | null => pick(s, false, s.config?.device)

/** Micrófono que se edita (el de su página) */
export const currentMic = (s: Pick<State, 'config' | 'engine'> = getState()): Device | null => pick(s, true, s.config?.micDevice)

/** Se está editando el micrófono: en su página */
export const editingMic = (s: Pick<State, 'page'> = getState()): boolean => s.page === 'mic'

/** Lo que se edita: el micrófono en su página y, en el resto, la salida */
export const currentDevice = (s: Sel = getState()): Device | null => (editingMic(s) ? currentMic(s) : currentOutput(s))

// Una sola instancia: los selectores de zustand necesitan devolver la misma referencia si nada cambia
const FLAT: Curve = Object.freeze(builtinCurve('b:flat'))

export function currentCurve(s: Sel = getState()): Curve {
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

/** Corrección de sala medida en una salida: se añade a «Mis presets» (marcada con la salida) y se pone en ella */
export function saveRoomPreset(name: string, curve: Curve, device: string): void {
  const cfg = getState().config
  if (!cfg) return
  const id = newPresetId()
  const c = { ...curve, preset: id }
  updateConfig({ presets: [...cfg.presets, { id, name, curve: c, room: device }], curves: { ...cfg.curves, [device]: c } })
}

/** Preset que llega compartido: se añade a «Mis presets» y se pone */
export function importPreset(name: string, curve: Curve): void {
  const cfg = getState().config
  if (!cfg) return
  const id = newPresetId()
  updateConfig({ presets: [...cfg.presets, { id, name, curve: { ...curve, preset: id } }] })
  applyPreset(curve, id)
}

/** Todos los presets, de serie (traducidos) y propios, en el orden del menú de presets */
export function allPresets(cfg: Config | null = getState().config): Preset[] {
  return [...BUILTIN.map((b) => ({ id: b.id, name: t(`presets.builtin.${b.id}`), curve: builtinCurve(b.id) })), ...(cfg?.presets ?? [])]
}

/** Borra uno de «Mis presets», con un aviso para deshacerlo */
export function deletePreset(id: string): void {
  const cfg = getState().config
  if (!cfg) return
  const before = cfg.presets
  updateConfig({ presets: before.filter((p) => p.id !== id) })
  toast(t('presets.deleted'), 'info', { secs: 6, action: { label: t('presets.undo'), run: () => updateConfig({ presets: before }) } })
}

export function renamePreset(id: string, name: string): void {
  const cfg = getState().config
  const clean = cleanName(name)
  if (cfg && clean) updateConfig({ presets: cfg.presets.map((p) => (p.id === id ? { ...p, name: clean } : p)) })
}

export async function refreshEngine(): Promise<EngineStatus> {
  const engine = await api.engineStatus()
  setState({ engine })
  return engine
}
