import { invoke } from '@tauri-apps/api/core'
import type { Band, Curve } from './eq'

export type Accent = 'cyan' | 'blue' | 'emerald' | 'violet' | 'pink' | 'amber' | 'red'

export interface Preset {
  id: string
  name: string
  curve: Curve
  /** Corrección de sala: la salida en la que se midió */
  room?: string | null
}

export interface Loudness {
  on: boolean
  offset: number
  strength: number
}

/** Dinámica y realces (PoxiDSP). Rangos: los de `Settings::sanitize` en `dsp/src/lib.rs` */
/** Latencia de una salida en ms: Windows + driver + aparato (medida) y el lookahead de PoxiDSP */
export interface OutputLatency {
  output: number
  dsp: number
  bluetooth: boolean
}

export interface Dsp {
  limiter: { on: boolean; ceiling: number }
  comp: { on: boolean; threshold: number; ratio: number; attack: number; release: number; knee: number; makeup: number }
  leveler: { on: boolean; target: number; maxGain: number }
  /** «Armónicos» (graves psicoacústicos) */
  bass: { on: boolean; freq: number; amount: number }
  dialog: { on: boolean; amount: number }
  crossfeed: { on: boolean; freq: number; feed: number }
  /** Altavoces virtuales (HRIR): excluye el crossfeed. `amount` 0…1 */
  speakers: { on: boolean; amount: number }
  /** Puerta de ruido (micrófono): dBFS, dB de atenuación y ms */
  gate: { on: boolean; threshold: number; range: number; attack: number; hold: number; release: number }
  /** «Graves» (la rueda): `amount` 0…1, a 0 no procesa */
  bassBoost: { on: boolean; amount: number }
}

/** Micrófono: su propio bypass (su EQ va en `curves` y su puerta/compresor en `perDevice`, con el id de la
 *  entrada) */
export interface Mic {
  /** Sin procesar (aparte del bypass de las salidas) */
  bypass: boolean
}

/** Secciones con presets propios (los del Ecualizador son `Preset`) */
export type Section = 'sound' | 'phones' | 'studio' | 'mic'

/** Preset de una sección: Sonido y Cascos guardan sus partes de `dsp`; Micrófono su `curve` y su puerta/compresor
 *  en `dsp`; Estudio, `studio`. Rust descarta lo que no es de su sección (`SectionPresets::sanitize`). */
export interface SectionPreset {
  id: string
  name: string
  dsp?: Dsp | null
  curve?: Curve | null
  studio?: Studio | null
}

export type SectionPresets = Record<Section, SectionPreset[]>

/** Lo que recuerda cada dispositivo aparte de su curva */
export interface DeviceSettings {
  /** En una salida, Sonido y Cascos; en un micrófono, su puerta y su compresor */
  dsp: Dsp
  /** Último preset elegido en cada sección */
  presets: { sound: string | null; phones: string | null; mic: string | null }
}

/** Atajos globales en el formato del registrador (`control+alt+Numpad0`); `null` = sin atajo */
export interface Hotkeys {
  bypass: string | null
  presets: { keys: string | null; preset: string | null }[]
}

/** Monitorización: qué se escucha. `mono` es el mid, (L + R) / 2 */
export type Listen = 'stereo' | 'mono' | 'side' | 'left' | 'right'
export type Sim = 'phone' | 'laptop' | 'car' | 'earbuds' | 'tv' | 'speaker'

/** Herramientas de estudio: momentáneas, no se guardan (Rust las quita al esconder la ventana o salir) */
export interface Studio {
  listen: Listen
  flipL: boolean
  flipR: boolean
  swap: boolean
  sim: Sim | null
}

/** Cuando `exe` está en primer plano suena `preset` (validado en Rust: `apps::sanitize_rules`) */
export interface AppRule {
  exe: string
  name: string
  preset: string
  on: boolean
}

export interface OpenApp {
  exe: string
  name: string
}

export type AnalyzerMode = 'spectrum' | 'spectrogram'

export interface Config {
  lang: 'es' | 'en' | null
  theme: 'dark' | 'light' | 'system'
  accent: Accent
  closeToTray: boolean
  bypass: boolean
  analyzer: boolean
  analyzerMode: AnalyzerMode
  seenVersion: string
  /** Salida que se edita (`null`: la predeterminada) */
  device: string | null
  /** Micrófono que se edita en su página (`null`: el predeterminado) */
  micDevice: string | null
  /** Al cambiar Windows la salida predeterminada, editar esa (lo hace Rust: evento `devices`) */
  followDefault: boolean
  curves: Record<string, Curve>
  presets: Preset[]
  loudness: Loudness
  meterCollapsed: boolean
  /** Por id de dispositivo; sin entrada, lo de serie (`DSP_DEFAULT`, ningún preset) */
  perDevice: Record<string, DeviceSettings>
  sectionPresets: SectionPresets
  appRules: AppRule[]
  hotkeys: Hotkeys
  mic: Mic
}

/** Lo que Rust necesita para aplicar un preset sin la interfaz (atajos, bandeja, reglas por app) */
export interface PresetEntry {
  id: string
  name: string
  curve: Curve
  /** Preamp automático de la curva */
  autoGain: number
}

export interface TrayLabels {
  open: string
  bypass: string
  quit: string
  presets: string
  eqOn: string
  eqOff: string
  broken: string
  repair: string
}

export interface Device {
  id: string
  name: string
  device: string
  default: boolean
  enabled: boolean
  unplugged: boolean
  /** Entrada (micrófono) */
  input: boolean
  /** PoxiEQ lo activó, pero ya no pasa por el motor: se lo quitó Windows o el driver (`lost`, o falta el motor)
   *  o tiene las «mejoras de audio» desactivadas (`enhancements`). Se arregla activándolo otra vez. */
  broken: 'lost' | 'enhancements' | null
}

export interface EngineStatus {
  installed: boolean
  configDir: string | null
  devices: Device[]
  /** PoxiDSP instalado en la carpeta del motor: falta, de otra versión o el de esta */
  dsp: 'missing' | 'old' | 'current'
}

export interface Check {
  id: string
  preMix: boolean
  postMix: boolean
}

export interface SetupReport {
  error: string | null
  checks: Check[]
}

export interface AutoEqEntry {
  name: string
  source: string
  form: 'over-ear' | 'in-ear' | 'earbud'
  path: string
}

export interface AppInfo {
  version: string
  portable: boolean
  startHidden: boolean
}

export interface AppError {
  code: string
  detail?: string | null
}

/** Niveles en el micrófono durante un barrido (dBFS) y posiciones medidas */
export interface RoomLevels {
  noise: number
  peak: number
  takes: number
}

export interface RoomParams {
  /** Ancho del suavizado en octavas */
  smoothing: number
  /** Caída del objetivo hacia agudos (dB/octava, ≤ 0) */
  slope: number
  /** Hasta dónde se corrige (Hz) */
  high: number
  maxBoost: number
  filters: number
}

export interface RoomFit {
  freqs: number[]
  measured: number[]
  target: number[]
  corrected: number[]
  bands: Band[]
  /** Por debajo no se sube nada (el altavoz ya no llega) */
  low: number
}

export const api = {
  appInfo: () => invoke<AppInfo>('app_info'),
  configGet: () => invoke<Config>('config_get'),
  configSet: (cfg: Config) => invoke<Config>('config_set', { cfg }),
  engineStatus: () => invoke<EngineStatus>('engine_status'),
  engineSetup: (enable: string[], disable: string[]) => invoke<SetupReport>('engine_setup', { enable, disable }),
  engineUninstall: () => invoke<void>('engine_uninstall'),
  autoeqIndex: (force = false) => invoke<AutoEqEntry[]>('autoeq_index', { force }),
  autoeqFetch: (path: string) => invoke<string>('autoeq_fetch', { path }),
  autostartGet: () => invoke<boolean>('autostart_get'),
  autostartSet: (on: boolean) => invoke<void>('autostart_set', { on }),
  trayLabels: (labels: TrayLabels) => invoke<void>('tray_labels', { labels }),
  presetsSync: (presets: PresetEntry[]) => invoke<void>('presets_sync', { presets }),
  appsList: () => invoke<OpenApp[]>('apps_list'),
  presetExport: (name: string, code: string) => invoke<string>('preset_export', { name, code }),
  analyzerStart: (device: string) => invoke<void>('analyzer_start', { device }),
  analyzerStop: () => invoke<void>('analyzer_stop'),
  compareSet: (gain: number | null) => invoke<void>('compare_set', { gain }),
  studioSet: (studio: Studio) => invoke<void>('studio_set', { studio }),
  hearingSet: (on: boolean) => invoke<void>('hearing_set', { on }),
  meterReset: () => invoke<void>('meter_reset'),
  outputLatency: (device: string, dsp: Dsp) => invoke<OutputLatency | null>('output_latency', { device, dsp }),
  appQuit: () => invoke<void>('app_quit'),
  hotkeyCheck: (keys: string) => invoke<string>('hotkey_check', { keys }),
  hotkeysPause: (paused: boolean) => invoke<void>('hotkeys_pause', { paused }),
  hotkeysFailed: () => invoke<string[]>('hotkeys_failed'),
  roomCheck: (output: string, input: string, level: number) => invoke<RoomLevels>('room_check', { output, input, level }),
  roomMeasure: (output: string, input: string, level: number) => invoke<RoomLevels>('room_measure', { output, input, level }),
  roomReset: () => invoke<void>('room_reset'),
  roomCancel: () => invoke<void>('room_cancel'),
  roomFit: (params: RoomParams) => invoke<RoomFit>('room_fit', { params })
}
