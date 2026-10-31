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
}

export interface Config {
  lang: 'es' | 'en' | null
  theme: 'dark' | 'light' | 'system'
  accent: Accent
  closeToTray: boolean
  bypass: boolean
  bypassHotkey: boolean
  analyzer: boolean
  seenVersion: string
  device: string | null
  curves: Record<string, Curve>
  presets: Preset[]
  loudness: Loudness
  meterCollapsed: boolean
  dsp: Dsp
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
  trayLabels: (labels: { open: string; bypass: string; quit: string }) => invoke<void>('tray_labels', { labels }),
  analyzerStart: (device: string) => invoke<void>('analyzer_start', { device }),
  analyzerStop: () => invoke<void>('analyzer_stop'),
  compareSet: (gain: number | null) => invoke<void>('compare_set', { gain }),
  meterReset: () => invoke<void>('meter_reset'),
  appQuit: () => invoke<void>('app_quit')
}
