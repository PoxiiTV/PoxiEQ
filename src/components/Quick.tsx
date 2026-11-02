import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { LogicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
import { Leaf, Moon, Radio, Volume2, X, Zap } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { api, type Config, type Quick, type QuickSection } from '../lib/api'
import { soundOf, SOUNDS, withSound, type Sound } from '../lib/dsp'
import { BUILTIN, builtinCurve } from '../lib/eq'
import {
  activeSectionPreset,
  applyPreset,
  applySectionPreset,
  currentCurve,
  currentOutput,
  outputSettings,
  sectionOff,
  sectionPresets,
  setOutputDsp,
  setState,
  updateConfig,
  useStore,
  works
} from '../lib/store'
import { useT } from '../i18n'
import { Logo } from './ui'

export const QUICK_SECTIONS: QuickSection[] = ['eq', 'sound', 'phones']
const SOUND_ICONS: Record<Sound, LucideIcon> = {
  natural: Leaf,
  streaming: Radio,
  loud: Volume2,
  night: Moon
}

/** De serie: los presets de serie del ecualizador y las cuatro sonoridades */
const DEFAULT_BUTTONS = [...BUILTIN.map((b) => `eq/${b.id}`), ...SOUNDS.map((s) => `sound/s:${s}`)]

export const quickButtons = (cfg: Config): string[] => cfg.quick.buttons ?? DEFAULT_BUTTONS

export interface QuickItem {
  /** `<sección>/<id>` (lo que se guarda en `Quick.buttons`) */
  key: string
  name: string
  sub?: string
  icon?: LucideIcon
  on: boolean
  pick: () => void
}

export const setQuick = (patch: Partial<Quick>): void => {
  const cfg = useStore.getState().config
  if (cfg) updateConfig({ quick: { ...cfg.quick, ...patch } }, false)
}

// ——— Tamaño de la ventana ———

const WIDTH = 340
const MAX_HEIGHT = 640
/** Tamaño y sitio de la ventana normal, para volver a él al salir del modo rápido */
const NORMAL_KEY = 'poxieq.normalWindow'
const TAB_KEY = 'poxieq.quickTab'
interface Normal {
  w: number
  h: number
  x: number
  y: number
  max: boolean
}
/** Mientras se sale del modo rápido, la ventanita ya no se ajusta a lo que tiene dentro */
let shrinking = false

/** Encoge la ventana a la ventanita de presets, con su esquina de arriba a la derecha donde estaba la normal */
export async function enterQuick(): Promise<void> {
  const win = getCurrentWindow()
  const k = await win.scaleFactor()
  const max = await win.isMaximized()
  const size = (await win.innerSize()).toLogical(k)
  const pos = (await win.outerPosition()).toLogical(k)
  localStorage.setItem(
    NORMAL_KEY,
    JSON.stringify({
      w: size.width,
      h: size.height,
      x: pos.x,
      y: pos.y,
      max
    } satisfies Normal)
  )
  shrinking = true
  if (max) await win.unmaximize()
  await win.setResizable(false)
  await win.setMinSize(new LogicalSize(WIDTH, 120))
  await win.setPosition(new LogicalPosition(Math.max(pos.x, pos.x + size.width - WIDTH), pos.y))
  setQuick({ on: true })
}

/** Vuelve a la ventana normal, con su tamaño y su sitio de antes */
export async function exitQuick(): Promise<void> {
  shrinking = false
  setQuick({ on: false })
  const win = getCurrentWindow()
  let n: Normal | null = null
  try {
    n = JSON.parse(localStorage.getItem(NORMAL_KEY) ?? 'null') as Normal | null
  } catch {
    n = null
  }
  await win.setResizable(true)
  await win.setMinSize(new LogicalSize(900, 600))
  await win.setSize(new LogicalSize(n?.w ?? 1180, n?.h ?? 740))
  if (n) await win.setPosition(new LogicalPosition(n.x, n.y))
  if (n?.max) await win.maximize()
}

// ——— Botones ———

/** Todos los presets que pueden ser botón, por sección, con cuál suena ahora en la salida */
export function useQuickCatalog(): Record<QuickSection, QuickItem[]> {
  const t = useT()
  const mineEq = useStore((s) => s.config?.presets)
  const eqActive = useStore((s) => currentCurve(s).preset)
  const mineSound = useStore((s) => sectionPresets(s, 'sound'))
  const minePhones = useStore((s) => sectionPresets(s, 'phones'))
  const soundActive = useStore((s) => activeSectionPreset(s, 'sound'))
  const phonesActive = useStore((s) => activeSectionPreset(s, 'phones'))
  const sound = useStore((s) => soundOf(outputSettings(s).dsp))
  const phonesOff = useStore((s) => sectionOff(s, 'phones'))

  return useMemo(() => {
    const eq: QuickItem[] = [
      ...BUILTIN.map((b) => ({
        id: b.id,
        name: t(`presets.builtin.${b.id}`),
        curve: builtinCurve(b.id)
      })),
      ...(mineEq ?? [])
    ].map((p) => ({
      key: `eq/${p.id}`,
      name: p.name,
      on: eqActive === p.id,
      pick: () => applyPreset(p.curve, p.id)
    }))
    const soundItems: QuickItem[] = [
      ...SOUNDS.map((s) => ({
        key: `sound/s:${s}`,
        name: t(`dsp.sounds.${s}.name`),
        sub: t(`dsp.sounds.${s}.meta`),
        icon: SOUND_ICONS[s],
        on: !soundActive && sound === s,
        pick: () => setOutputDsp(withSound(outputSettings().dsp, s))
      })),
      ...mineSound.map((p) => ({
        key: `sound/${p.id}`,
        name: p.name,
        on: soundActive === p.id,
        pick: () => applySectionPreset('sound', p)
      }))
    ]
    const phones: QuickItem[] = [
      {
        key: 'phones/s:off',
        name: t('quick.phonesOff'),
        on: phonesOff,
        pick: () => {
          const d = outputSettings().dsp
          setOutputDsp({
            ...d,
            crossfeed: { ...d.crossfeed, on: false },
            speakers: { ...d.speakers, on: false }
          })
        }
      },
      ...minePhones.map((p) => ({
        key: `phones/${p.id}`,
        name: p.name,
        on: phonesActive === p.id,
        pick: () => applySectionPreset('phones', p)
      }))
    ]
    return { eq, sound: soundItems, phones }
  }, [t, mineEq, eqActive, mineSound, minePhones, soundActive, phonesActive, sound, phonesOff])
}

/** Modo rápido: la ventana encogida, con pestañas por sección y los botones elegidos en Ajustes. Al pulsar uno se
 *  pone en la salida de ahora y la ventana se esconde en la bandeja (si así se quiere). */
export function QuickWindow(): React.JSX.Element {
  const t = useT()
  const quick = useStore((s) => s.config!.quick)
  const chosen = useStore((s) => quickButtons(s.config!))
  const ready = useStore((s) => !!s.engine?.installed && works(currentOutput(s)))
  const catalog = useQuickCatalog()
  const groups = QUICK_SECTIONS.map((sec) => [sec, catalog[sec].filter((i) => chosen.includes(i.key))] as const).filter(([, items]) => items.length > 0)
  const [tab, setTab] = useState<QuickSection>(() => (localStorage.getItem(TAB_KEY) as QuickSection | null) ?? 'eq')
  const current = groups.find(([sec]) => sec === tab) ?? groups[0]

  // Lo de esta ventana va siempre a la salida: si se venía de la página del micrófono, se sale de ella
  const onMic = useStore((s) => s.page === 'mic')
  useEffect(() => {
    if (onMic) setState({ page: 'eq', selected: null })
  }, [onMic])

  // La ventana mide justo lo que tiene dentro (hasta un tope; más allá, los botones se desplazan)
  const inner = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    shrinking = true
    const win = getCurrentWindow()
    void win.setResizable(false)
    void win.setMinSize(new LogicalSize(WIDTH, 120))
    const el = inner.current
    if (!el) return
    const fit = (): void => {
      if (shrinking) void win.setSize(new LogicalSize(WIDTH, Math.min(MAX_HEIGHT, Math.ceil(el.getBoundingClientRect().height))))
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const press = (i: QuickItem): void => {
    i.pick()
    // Un instante para que se vea cuál se ha puesto, y a la bandeja
    if (quick.hide) setTimeout(() => void api.windowHide(), 220)
  }
  const pickTab = (sec: QuickSection): void => {
    setTab(sec)
    localStorage.setItem(TAB_KEY, sec)
  }
  const openSettings = (): void => {
    void exitQuick().then(() => setState({ page: 'settings', settingsTab: 'quick' }))
  }

  return (
    <div className="quick-win">
      <div className="quick-inner" ref={inner}>
        <div className="quick-titlebar" data-tauri-drag-region>
          <Logo size={18} />
          <span className="quick-name" data-tauri-drag-region>
            Poxi<b>EQ</b>
          </span>
          <div className="spacer" data-tauri-drag-region />
          <button className="power no-drag" onClick={() => void exitQuick()} aria-label={t('titlebar.quickOn')}>
            <Zap size={14} />
            <span>{t('titlebar.quick')}</span>
          </button>
          <button className="quick-x no-drag" onClick={() => void api.windowHide()} aria-label={t('quick.hideNow')}>
            <X size={16} />
          </button>
        </div>
        <div className="quick-body">
          {!ready ? (
            <div className="quick-empty">
              <p>{t('quick.notReady')}</p>
              <button className="btn btn-sm btn-primary" onClick={() => void exitQuick()}>
                {t('quick.openFull')}
              </button>
            </div>
          ) : !current ? (
            <div className="quick-empty">
              <p>{t('quick.none')}</p>
              <button className="btn btn-sm btn-primary" onClick={openSettings}>
                {t('quick.choose')}
              </button>
            </div>
          ) : (
            <>
              {groups.length > 1 && (
                <div className="quick-tabs" role="tablist">
                  {groups.map(([sec]) => (
                    <button key={sec} role="tab" aria-selected={sec === current[0]} className={sec === current[0] ? 'on' : ''} onClick={() => pickTab(sec)}>
                      {t(`nav.${sec}`)}
                    </button>
                  ))}
                </div>
              )}
              <div className="quick-grid">
                {current[1].map((i) => {
                  const Icon = i.icon
                  return (
                    <button key={i.key} className={`quick-btn ${i.on ? 'on' : ''}`} aria-pressed={i.on} onClick={() => press(i)}>
                      {Icon && <Icon size={15} className="quick-ico" />}
                      <b>{i.name}</b>
                      {i.sub && <small className="num">{i.sub}</small>}
                    </button>
                  )
                })}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

/** Ajustes → Modo rápido: qué botones salen (pulsar añade o quita; puede no haber ninguno) */
export function QuickSettings(): React.JSX.Element {
  const t = useT()
  const cfg = useStore((s) => s.config!)
  const chosen = quickButtons(cfg)
  const catalog = useQuickCatalog()
  const toggle = (key: string): void =>
    setQuick({
      buttons: chosen.includes(key) ? chosen.filter((k) => k !== key) : [...chosen, key]
    })
  const setSection = (sec: QuickSection, on: boolean): void => {
    const keys = catalog[sec].map((i) => i.key)
    const rest = chosen.filter((k) => !keys.includes(k))
    setQuick({ buttons: on ? [...rest, ...keys] : rest })
  }

  return (
    <div className="quick-set">
      {QUICK_SECTIONS.map((sec) => (
        <div key={sec} className="quick-set-sec">
          <div className="quick-set-head">
            <b>{t(`nav.${sec}`)}</b>
            <span className="spacer" />
            <button className="btn btn-sm" onClick={() => setSection(sec, true)}>
              {t('quick.all')}
            </button>
            <button className="btn btn-sm" onClick={() => setSection(sec, false)}>
              {t('quick.noneBtn')}
            </button>
          </div>
          <div className="quick-chips">
            {catalog[sec].map((i) => {
              const on = chosen.includes(i.key)
              return (
                <button key={i.key} className={`quick-chip ${on ? 'on' : ''}`} aria-pressed={on} onClick={() => toggle(i.key)}>
                  {i.name}
                </button>
              )
            })}
          </div>
        </div>
      ))}
    </div>
  )
}
