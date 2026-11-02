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
  /** Micrófono: supresión de ruido (RNNoise); `amount` 0…1 = mezcla con la original */
  denoise: { on: boolean; amount: number }
}

/** Micrófono: su propio bypass (su EQ va en `curves` y su puerta/compresor en `perDevice`, con el id de la
 *  entrada) */
export interface Mic {
  /** Sin procesar (aparte del bypass de las salidas) */
  bypass: boolean
}

/** Secciones con presets propios (los del Ecualizador son `Preset`) */
export type Section = 'sound' | 'phones' | 'studio' | 'mic' | 'plugins'

export type PluginFormat = 'vst2' | 'vst3'

/** Un plugin del usuario en la cadena de un dispositivo (lo aloja PoxiDSP en el audio de Windows) */
export interface PluginSlot {
  /** Único en la cadena */
  id: string
  format: PluginFormat
  path: string
  /** Clase dentro de un VST3 (hex); en VST2, vacío */
  cid: string
  name: string
  vendor: string
  on: boolean
  /** Estado del plugin en base64 (vacío: el inicial) */
  state: string
  /** Muestras de retardo a 48 kHz */
  latency: number
}

/** Un plugin encontrado al escanear */
export type FoundPlugin = Pick<PluginSlot, 'format' | 'path' | 'cid' | 'name' | 'vendor' | 'latency'>

export interface ScanResult {
  plugins: FoundPlugin[]
  failed: { path: string; error: string }[]
}

/** Preset de una sección: Sonido y Cascos guardan sus partes de `dsp`; Micrófono su `curve` y su puerta/compresor
 *  en `dsp`; Estudio, `studio`. Rust descarta lo que no es de su sección (`SectionPresets::sanitize`). */
export interface SectionPreset {
  id: string
  name: string
  dsp?: Dsp | null
  curve?: Curve | null
  studio?: Studio | null
  plugins?: PluginSlot[] | null
}

export type SectionPresets = Record<Section, SectionPreset[]>

/** Respuesta al impulso de una salida (convolución) */
export interface Ir {
  file: string
  name: string
  on: boolean
  channels: number
  rate: number
  seconds: number
  /** ms hasta el pico */
  latency: number
  /** dB del primer canal, de 20 Hz a 20 kHz en escala logarítmica */
  response: number[]
}

/** Lo que recuerda cada dispositivo aparte de su curva */
export interface DeviceSettings {
  /** En una salida, Sonido y Cascos; en un micrófono, su puerta y su compresor */
  dsp: Dsp
  /** Último preset elegido en cada sección */
  presets: { sound: string | null; phones: string | null; mic: string | null; plugins: string | null }
  /** Plugins del usuario, en orden (después de la curva y antes de PoxiDSP) */
  plugins: PluginSlot[]
  /** Respuesta al impulso, justo después de la curva */
  ir: Ir | null
  /** Balance en dB (±12): positivo baja el izquierdo, negativo el derecho */
  balance: number
}

/** Atajos globales en el formato del registrador (`control+alt+Numpad0`); `null` = sin atajo */
export interface Hotkeys {
  bypass: string | null
  presets: { keys: string | null; preset: string | null }[]
  mixerGame: string | null
  mixerChat: string | null
}

/** Monitorización: qué se escucha. `mono` es el mid, (L + R) / 2 */
export type Listen = 'stereo' | 'mono' | 'side' | 'left' | 'right'
export type Sim = 'phone' | 'laptop' | 'car' | 'earbuds' | 'tv' | 'speaker' | 'tablet' | 'soundbar' | 'monitors' | 'club'

/** Herramientas de estudio: momentáneas, no se guardan (Rust las quita al esconder la ventana o salir) */
export interface Studio {
  listen: Listen
  flipL: boolean
  flipR: boolean
  swap: boolean
  sim: Sim | null
  /** Inclinación del espectro en dB (pivote en 1 kHz), −6…6 */
  tilt: number
}

/** Cuando `exe` está en primer plano suena `preset` (validado en Rust: `apps::sanitize_rules`) */
export interface AppRule {
  exe: string
  name: string
  /** Vacío: la regla solo cambia la salida */
  preset: string
  on: boolean
  /** Salida por la que suena la app (GUID); `null` = la predeterminada */
  output: string | null
}

/** Juego y voz */
export interface Voice {
  duck: boolean
  duckDb: number
  apps: string[]
  /** −1 = todo juego, 0 = igual, 1 = todo chat */
  mixer: number
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
  /** Carpetas de plugins que añade el usuario (aparte de las estándar) */
  pluginFolders: string[]
  voice: Voice
  roomCal: RoomCal | null
  /** Activar solos los dispositivos nuevos */
  autoEnable: boolean
  quick: Quick
  appRules: AppRule[]
  hotkeys: Hotkeys
  mic: Mic
}

export type QuickSection = 'eq' | 'sound' | 'phones'

/** Modo rápido: la ventana es solo botones grandes de presets */
export interface Quick {
  on: boolean
  /** Secciones que se ven, en su orden */
  sections: QuickSection[]
  /** Presets escondidos (`<sección>/<id>`) */
  hidden: string[]
  /** Esconder la ventana (a la bandeja) al elegir uno */
  hide: boolean
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
  pluginsOff: string
  game: string
  chat: string
  mini: string
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
  /** Cascos Bluetooth: la otra mitad (del estéreo, su manos libres; del manos libres, su estéreo) */
  pair: string | null
  /** Manos libres de unos cascos Bluetooth: va con su estéreo (misma curva) y no sale aparte */
  handsFree: boolean
}

export interface EngineStatus {
  installed: boolean
  configDir: string | null
  devices: Device[]
  /** PoxiDSP instalado en la carpeta del motor: falta, de otra versión o el de esta */
  dsp: 'missing' | 'old' | 'current'
}

/** «¿Por qué no suena?»: lo que Rust ve sin permisos ni sonido */
export interface Diagnosis {
  spatial: boolean
  exclusive: boolean
  muted: boolean
  vendor: string[]
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
  /** La toma no se parece a las de antes */
  odd: boolean
}

/** Calibración del micro de medida (dB en la rejilla de la sala) */
export interface RoomCal {
  name: string
  db: number[]
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
  engineDiagnose: (device: string) => invoke<Diagnosis>('engine_diagnose', { device }),
  engineCheck: (device: string) => invoke<Check>('engine_check', { device }),
  soundSettings: () => invoke<void>('sound_settings'),
  autoEnableSet: (on: boolean) => invoke<void>('auto_enable_set', { on }),
  miniSet: (on: boolean) => invoke<void>('mini_set', { on }),
  pickFile: (kind: 'wav' | 'cal' | 'eq' | 'plugin') => invoke<string | null>('pick_file', { kind }),
  irLoad: (path: string) => invoke<Ir>('ir_load', { path }),
  textRead: (path: string) => invoke<string>('text_read', { path }),
  textExport: (name: string, ext: 'txt' | 'json', content: string) => invoke<string>('text_export', { name, ext, content }),
  graphicFit: (points: [number, number][]) => invoke<Band[]>('graphic_fit', { points }),
  pluginsScan: (force: boolean) => invoke<ScanResult>('plugins_scan', { force }),
  pluginsCached: () => invoke<ScanResult>('plugins_cached'),
  pluginScanFile: (path: string) => invoke<FoundPlugin[]>('plugin_scan_file', { path }),
  pluginEdit: (device: string, id: string) => invoke<void>('plugin_edit', { device, id }),
  autoeqIndex: (force = false) => invoke<AutoEqEntry[]>('autoeq_index', { force }),
  autoeqFetch: (path: string) => invoke<string>('autoeq_fetch', { path }),
  autoeqFit: (path: string, target: string | null, like: string | null) => invoke<Band[]>('autoeq_fit', { req: { path, target, like } }),
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
  windowHide: () => invoke<void>('window_hide'),
  hotkeyCheck: (keys: string) => invoke<string>('hotkey_check', { keys }),
  hotkeysPause: (paused: boolean) => invoke<void>('hotkeys_pause', { paused }),
  hotkeysFailed: () => invoke<string[]>('hotkeys_failed'),
  roomCheck: (output: string, input: string, level: number) => invoke<RoomLevels>('room_check', { output, input, level }),
  roomMeasure: (output: string, input: string, level: number) => invoke<RoomLevels>('room_measure', { output, input, level }),
  roomReset: () => invoke<void>('room_reset'),
  roomCalLoad: (path: string) => invoke<RoomCal>('room_cal_load', { path }),
  roomCancel: () => invoke<void>('room_cancel'),
  roomFit: (params: RoomParams) => invoke<RoomFit>('room_fit', { params })
}
