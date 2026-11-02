import { create } from 'zustand'
import { api, type AppInfo, type Config, type Device, type DeviceSettings, type Dsp, type EngineStatus, type PluginSlot, type Preset, type Section, type SectionPreset, type Studio } from './api'
import { DSP_DEFAULT, equal, onlyParts, partsOn, sameParts, SECTION_PARTS, withParts } from './dsp'
import { autoPreamp, BUILTIN, builtinCurve, hasGain, type Curve } from './eq'
import { Grouper, History } from './history'
import { cleanName, encodeShare, type Shared, type ShareSection } from './share'
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
export type Page = 'eq' | 'sound' | 'phones' | 'studio' | 'room' | 'mic' | 'plugins' | 'apps' | 'settings'

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
  modal: 'savePreset' | 'import' | 'hearing' | 'room' | 'diagnose' | null
  /** Sección del preset que se guarda (ventana de guardar) */
  saveFor: ShareSection | 'plugins'
  /** Preset que se está compartiendo, con su código (ventana de compartir abierta) */
  sharing: { name: string; code: string } | null
  /** Último preset de Estudio elegido: momentáneo como la propia escucha (se va con ella) */
  studioPreset: string | null
  /** Página Plugins: la cadena de la salida o la del micrófono */
  pluginsFor: 'output' | 'mic'
  /** Ventanas de edición de plugins abiertas (`dispositivo/hueco`) */
  editing: string[]
  /** Código que llega a la ventana de importar (archivo arrastrado) */
  importCode: string
  /** Hay algo que deshacer / rehacer */
  history: { undo: boolean; redo: boolean }
  /** Atajos que Windows no ha dejado registrar (los tiene otro programa) */
  hotkeysFailed: string[]
  /** Pestaña de Ajustes a la vista */
  settingsTab: SettingsTab
  toasts: Toast[]
}

export type SettingsTab = 'general' | 'keys' | 'quick' | 'engine' | 'about'

export const STUDIO_OFF: Studio = { listen: 'stereo', flipL: false, flipR: false, swap: false, sim: null, tilt: 0 }

export const useStore = create<State>(() => ({
  app: null,
  config: null,
  engine: null,
  setup: null,
  selected: null,
  page: 'eq',
  studio: STUDIO_OFF,
  modal: null,
  saveFor: 'eq',
  sharing: null,
  studioPreset: null,
  pluginsFor: 'output',
  editing: [],
  importCode: '',
  history: { undo: false, redo: false },
  hotkeysFailed: [],
  settingsTab: 'general',
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
  const touches = (patch.curves && patch.curves !== cur.curves) || (patch.perDevice && patch.perDevice !== cur.perDevice) || (patch.mic && patch.mic !== cur.mic)
  if (record && touches) {
    history.record({ curves: cur.curves, perDevice: cur.perDevice, mic: cur.mic }, grouper.next(performance.now()))
    syncHistory()
  }
  const next = { ...cur, ...patch }
  setState({ config: next })
  pending = next
  void flush()
}

// ——— Herramientas de estudio ———

export const studioActive = (s: Studio): boolean => s.listen !== 'stereo' || s.flipL || s.flipR || s.swap || s.sim !== null || s.tilt !== 0

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

// ——— Deshacer / rehacer (curvas, ajustes de cada dispositivo y micrófono) ———

type Snapshot = Pick<Config, 'curves' | 'perDevice' | 'mic'>
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
  const snap = history[dir]({ curves: cfg.curves, perDevice: cfg.perDevice, mic: cfg.mic })
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

/** El elegido si está conectado y, si no, el predeterminado (o el primero). El manos libres de unos cascos
 *  Bluetooth cuenta como su estéreo (es el que se ajusta). */
function pick(s: Pick<State, 'config' | 'engine'>, input: boolean, id: string | null | undefined): Device | null {
  const all = s.engine?.devices.filter((d) => d.input === input && !d.unplugged) ?? []
  const list = all.filter(listed)
  const main = (d: Device | undefined): Device | undefined => (d?.handsFree ? list.find((x) => x.id === d.pair) : d)
  return main(all.find((d) => d.id === id)) ?? main(all.find((d) => d.default)) ?? list[0] ?? null
}

/** Sale en las listas: todo menos el manos libres de unos cascos Bluetooth (va con su estéreo) */
export const listed = (d: Device): boolean => !d.handsFree

/** Salida que se edita (la de la barra de título) */
export const currentOutput = (s: Pick<State, 'config' | 'engine'> = getState()): Device | null => pick(s, false, s.config?.device)

/** Micrófono que se edita (el de su página) */
export const currentMic = (s: Pick<State, 'config' | 'engine'> = getState()): Device | null => pick(s, true, s.config?.micDevice)

/** Se está editando el micrófono: en su página */
export const editingMic = (s: Pick<State, 'page'> = getState()): boolean => s.page === 'mic'

/** Pasa de verdad por el motor: activado y sin nada que lo haya roto */
export const works = (d: Device | null | undefined): boolean => !!d?.enabled && !d.broken

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

// ——— Ajustes de cada dispositivo: su PoxiDSP y el último preset de cada sección ———

const NO_SETTINGS: DeviceSettings = Object.freeze({ dsp: DSP_DEFAULT, presets: { sound: null, phones: null, mic: null, plugins: null }, plugins: [], ir: null, balance: 0 })

/** Lo que recuerda un dispositivo; sin nada guardado, lo de serie (Natural, sin crossfeed, ningún preset) */
export function settingsOf(s: Pick<State, 'config'>, id: string | null | undefined): DeviceSettings {
  return (id && s.config?.perDevice[id]) || NO_SETTINGS
}

/** Ajustes de la salida que se edita (Sonido y Cascos) */
export const outputSettings = (s: Pick<State, 'config' | 'engine'> = getState()): DeviceSettings => settingsOf(s, currentOutput(s)?.id)

/** Ajustes del micrófono que se edita (puerta y compresor) */
export const micSettings = (s: Pick<State, 'config' | 'engine'> = getState()): DeviceSettings => settingsOf(s, currentMic(s)?.id)

function setDsp(id: string | undefined, dsp: Dsp): void {
  const cfg = getState().config
  if (id && cfg) updateConfig({ perDevice: { ...cfg.perDevice, [id]: { ...settingsOf(getState(), id), dsp } } })
}

/** Cambia el PoxiDSP de la salida que se edita */
export const setOutputDsp = (dsp: Dsp): void => setDsp(currentOutput()?.id, dsp)

/** Cambia la puerta/compresor del micrófono que se edita */
export const setMicDsp = (dsp: Dsp): void => setDsp(currentMic()?.id, dsp)

// ——— Plugins del usuario ———

/** Dispositivo cuya cadena se ve en la página Plugins */
export const pluginsDevice = (s: Pick<State, 'config' | 'engine' | 'pluginsFor'> = getState()): Device | null =>
  s.pluginsFor === 'mic' ? currentMic(s) : currentOutput(s)

export const pluginsSettings = (s: Pick<State, 'config' | 'engine' | 'pluginsFor'> = getState()): DeviceSettings => settingsOf(s, pluginsDevice(s)?.id)

/** Cambia la cadena de plugins del dispositivo de la página Plugins */
export function setPlugins(plugins: PluginSlot[]): void {
  const s = getState()
  const id = pluginsDevice(s)?.id
  if (id && s.config) updateConfig({ perDevice: { ...s.config.perDevice, [id]: { ...settingsOf(s, id), plugins } } })
}

// ——— Presets de Sonido, Cascos, Estudio y Micrófono ———

/** Como mucho por sección (lo mismo que `MAX_SECTION_PRESETS` en Rust) */
export const MAX_SECTION_PRESETS = 100

const NO_PRESETS: SectionPreset[] = []

export const sectionPresets = (s: Pick<State, 'config'>, section: Section): SectionPreset[] => s.config?.sectionPresets[section] ?? NO_PRESETS

const micCurve = (s: State): Curve => {
  const id = currentMic(s)?.id
  return (id && s.config?.curves[id]) || FLAT
}

const sameCurve = (a: Curve, b: Curve): boolean => a.preamp === b.preamp && a.autoPreamp === b.autoPreamp && equal(a.bands, b.bands)

/** Lo que suena ahora en una sección coincide con un preset */
function matches(s: State, section: Section, p: SectionPreset): boolean {
  switch (section) {
    case 'sound':
    case 'phones':
      return !!p.dsp && sameParts(outputSettings(s).dsp, p.dsp, SECTION_PARTS[section])
    case 'mic':
      return !!p.dsp && !!p.curve && sameParts(micSettings(s).dsp, p.dsp, SECTION_PARTS.mic) && sameCurve(micCurve(s), p.curve)
    case 'studio':
      return !!p.studio && equal(s.studio, p.studio)
    case 'plugins':
      return !!p.plugins && equal(chainOf(pluginsSettings(s).plugins), chainOf(p.plugins))
  }
}

/** Lo que define una cadena: qué plugins, en qué orden, encendidos o no y con qué estado */
const chainOf = (list: PluginSlot[]): unknown[] => list.map((p) => [p.path, p.cid, p.on, p.state])

/** Preset de la sección que suena: el último elegido mientras no se retoque (después, «Personalizado») */
export function activeSectionPreset(s: State, section: Section): string | null {
  const id =
    section === 'studio' ? s.studioPreset : section === 'mic' ? micSettings(s).presets.mic : section === 'plugins' ? pluginsSettings(s).presets.plugins : outputSettings(s).presets[section]
  const p = id ? sectionPresets(s, section).find((x) => x.id === id) : undefined
  return p && matches(s, section, p) ? p.id : null
}

/** La sección no hace nada: sin preset no es «Personalizado» sino «Ninguno» */
export function sectionOff(s: State, section: Section): boolean {
  switch (section) {
    case 'sound':
    case 'phones':
      return !partsOn(outputSettings(s).dsp, SECTION_PARTS[section])
    case 'studio':
      return !studioActive(s.studio)
    case 'mic': {
      const c = micCurve(s)
      return !partsOn(micSettings(s).dsp, SECTION_PARTS.mic) && c.preamp === 0 && c.bands.every((b) => !b.on || (hasGain(b.kind) && b.gain === 0))
    }
    case 'plugins':
      return pluginsSettings(s).plugins.length === 0
  }
}

/** Lo que guarda el preset de una sección: solo lo suyo, con los valores de ahora */
function snapshot(s: State, section: Section): Omit<SectionPreset, 'id' | 'name'> {
  switch (section) {
    case 'sound':
    case 'phones':
      return { dsp: onlyParts(outputSettings(s).dsp, SECTION_PARTS[section]) }
    case 'mic': {
      const c = micCurve(s)
      return { dsp: onlyParts(micSettings(s).dsp, SECTION_PARTS.mic), curve: { preamp: c.preamp, autoPreamp: c.autoPreamp, bands: structuredClone(c.bands), preset: null } }
    }
    case 'studio':
      return { studio: { ...s.studio } }
    case 'plugins':
      return { plugins: structuredClone(pluginsSettings(s).plugins) }
  }
}

/** Dispositivo al que se aplica una sección: el micrófono que se edita o la salida */
const targetOf = (s: State, section: Section): string | undefined => (section === 'mic' || (section === 'plugins' && s.pluginsFor === 'mic') ? currentMic(s) : currentOutput(s))?.id

/** Apunta `id` como último preset de la sección en su dispositivo (sin paso de deshacer: no cambia el sonido) */
function markPick(section: Section, id: string): void {
  const s = getState()
  if (section === 'studio') return setState({ studioPreset: id })
  const dev = targetOf(s, section)
  if (!dev || !s.config) return
  const cur = settingsOf(s, dev)
  updateConfig({ perDevice: { ...s.config.perDevice, [dev]: { ...cur, presets: { ...cur.presets, [section]: id } } } }, false)
}

function addSectionPreset(section: Section, preset: SectionPreset): boolean {
  const cfg = getState().config
  if (!cfg) return false
  const list = cfg.sectionPresets[section]
  if (list.length >= MAX_SECTION_PRESETS) {
    toast(t('presets.full', { n: MAX_SECTION_PRESETS }), 'err')
    return false
  }
  updateConfig({ sectionPresets: { ...cfg.sectionPresets, [section]: [...list, preset] } })
  return true
}

/** Guarda lo de ahora como preset de la sección y lo deja marcado */
export function saveSectionPreset(section: Section, name: string): boolean {
  const preset: SectionPreset = { id: newPresetId(), name, ...snapshot(getState(), section) }
  if (!addSectionPreset(section, preset)) return false
  markPick(section, preset.id)
  return true
}

/** Pone un preset de su sección: Sonido y Cascos en la salida que se edita, Micrófono en el micro que se edita y
 *  Estudio en la escucha (momentánea). Solo cambia lo de esa sección. */
export function applySectionPreset(section: Section, p: SectionPreset): void {
  const s = getState()
  if (!s.config) return
  if (section === 'studio') {
    if (p.studio) {
      setStudio(p.studio)
      setState({ studioPreset: p.id })
    }
    return
  }
  const dev = targetOf(s, section)
  if (!dev) return
  const cur = settingsOf(s, dev)
  if (section === 'plugins') {
    if (p.plugins) updateConfig({ perDevice: { ...s.config.perDevice, [dev]: { ...cur, plugins: structuredClone(p.plugins), presets: { ...cur.presets, plugins: p.id } } } })
    return
  }
  if (!p.dsp) return
  const next: DeviceSettings = { ...cur, dsp: withParts(cur.dsp, p.dsp, SECTION_PARTS[section]), presets: { ...cur.presets, [section]: p.id } }
  const patch: Partial<Config> = { perDevice: { ...s.config.perDevice, [dev]: next } }
  if (section === 'mic' && p.curve) {
    setState({ selected: null })
    patch.curves = { ...s.config.curves, [dev]: { ...structuredClone(p.curve), preset: null } }
  }
  updateConfig(patch)
}

/** Borra un preset de una sección, con un aviso para deshacerlo */
export function deleteSectionPreset(section: Section, id: string): void {
  const cfg = getState().config
  if (!cfg) return
  const before = cfg.sectionPresets[section]
  const put = (list: SectionPreset[]): void => {
    const c = getState().config
    if (c) updateConfig({ sectionPresets: { ...c.sectionPresets, [section]: list } })
  }
  put(before.filter((p) => p.id !== id))
  toast(t('presets.deleted'), 'info', { secs: 6, action: { label: t('presets.undo'), run: () => put(before) } })
}

export function renameSectionPreset(section: Section, id: string, name: string): void {
  const cfg = getState().config
  const clean = cleanName(name)
  if (cfg && clean) updateConfig({ sectionPresets: { ...cfg.sectionPresets, [section]: cfg.sectionPresets[section].map((p) => (p.id === id ? { ...p, name: clean } : p)) } })
}

/** Lo que viaja al compartir un preset de una sección */
function sharedOf(section: Section, p: SectionPreset): Shared | null {
  if ((section === 'sound' || section === 'phones') && p.dsp) return { section, name: p.name, dsp: p.dsp }
  if (section === 'mic' && p.dsp && p.curve) return { section, name: p.name, curve: p.curve, dsp: p.dsp }
  if (section === 'studio' && p.studio) return { section, name: p.name, studio: p.studio }
  return null
}

/** Abre la ventana de compartir con el código del preset */
export function sharePreset(section: ShareSection, p: Preset | SectionPreset): void {
  const shared = section === 'eq' ? (p as Preset).curve && { section, name: p.name, curve: (p as Preset).curve } : sharedOf(section, p as SectionPreset)
  if (shared) setState({ sharing: { name: p.name, code: encodeShare(shared) } })
}

/** Preset que llega compartido: va a «Mis presets» de su sección, se pone y se enseña esa sección */
export function importShared(sh: Shared, name: string): boolean {
  if (sh.section === 'eq') {
    go('eq')
    importPreset(name, sh.curve)
    return true
  }
  const fields: Omit<SectionPreset, 'id' | 'name'> =
    sh.section === 'studio' ? { studio: sh.studio } : sh.section === 'mic' ? { curve: sh.curve, dsp: sh.dsp } : { dsp: sh.dsp }
  const preset: SectionPreset = { id: newPresetId(), name, ...fields }
  if (!addSectionPreset(sh.section, preset)) return false
  go(sh.section)
  applySectionPreset(sh.section, preset)
  return true
}

export async function refreshEngine(): Promise<EngineStatus> {
  const engine = await api.engineStatus()
  setState({ engine })
  return engine
}
