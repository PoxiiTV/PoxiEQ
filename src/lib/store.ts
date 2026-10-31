import { create } from 'zustand'
import { api, type AppInfo, type Config, type Device, type EngineStatus, type Preset } from './api'
import { autoPreamp, builtinCurve, type Curve } from './eq'
import { errorText } from '../i18n'

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
  settingsOpen: boolean
  modal: 'autoeq' | 'savePreset' | null
  toasts: Toast[]
}

export const useStore = create<State>(() => ({
  app: null,
  config: null,
  engine: null,
  setup: null,
  selected: null,
  settingsOpen: false,
  modal: null,
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

/** Arrastrando la curva llegan decenas de cambios por segundo: solo se envía el último pendiente */
export function updateConfig(patch: Partial<Config>): void {
  const cur = getState().config
  if (!cur) return
  const next = { ...cur, ...patch }
  setState({ config: next })
  pending = next
  void flush()
}

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

export function savePreset(name: string): void {
  const cfg = getState().config
  if (!cfg) return
  const id = `u:${Date.now().toString(36)}`
  const curve = { ...currentCurve(), preset: id }
  const presets: Preset[] = [...cfg.presets, { id, name, curve }]
  const dev = currentDevice()?.id
  updateConfig({ presets, curves: dev ? { ...cfg.curves, [dev]: curve } : cfg.curves })
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
