import { invoke } from '@tauri-apps/api/core'
import type { Curve } from './eq'

export type Accent = 'cyan' | 'blue' | 'emerald' | 'violet' | 'pink' | 'amber' | 'red'

export interface Preset {
  id: string
  name: string
  curve: Curve
}

export interface Loudness {
  on: boolean
  offset: number
  strength: number
}

/** Dinámica y realces (PoxiDSP). Rangos: los de `Settings::sanitize` en `dsp/src/lib.rs` */
export interface Dsp {
  limiter: { on: boolean; ceiling: number }
  comp: { on: boolean; threshold: number; ratio: number; attack: number; release: number; knee: number; makeup: number }
  leveler: { on: boolean; target: number; maxGain: number }
  bass: { on: boolean; freq: number; amount: number }
  dialog: { on: boolean; amount: number }
  crossfeed: { on: boolean; freq: number; feed: number }
  /** Altavoces virtuales (HRIR): excluye el crossfeed. `amount` 0…1 */
  speakers: { on: boolean; amount: number }
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
  bypassHotkey: boolean
  analyzer: boolean
  analyzerMode: AnalyzerMode
  seenVersion: string
  device: string | null
  curves: Record<string, Curve>
  presets: Preset[]
  loudness: Loudness
  meterCollapsed: boolean
  dsp: Dsp
  appRules: AppRule[]
  /** Ctrl+Alt+1…9: id del preset de cada atajo */
  presetKeys: (string | null)[]
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
}

export interface Device {
  id: string
  name: string
  device: string
  default: boolean
  enabled: boolean
  unplugged: boolean
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
  appQuit: () => invoke<void>('app_quit')
}
