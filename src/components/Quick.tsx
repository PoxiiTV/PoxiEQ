import { useEffect, useMemo, useState } from 'react'
import { AudioLines, Eye, EyeOff, Headphones, Leaf, Moon, Pencil, Radio, SlidersHorizontal, Volume2 } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { api, type Quick, type QuickSection } from '../lib/api'
import { soundOf, SOUNDS, withSound, type Sound } from '../lib/dsp'
import { BUILTIN, builtinCurve } from '../lib/eq'
import {
  activeSectionPreset,
  applyPreset,
  applySectionPreset,
  currentCurve,
  outputSettings,
  sectionOff,
  sectionPresets,
  setOutputDsp,
  setState,
  updateConfig,
  useStore
} from '../lib/store'
import { useT } from '../i18n'
import { Switch } from './ui'

const SECTIONS: QuickSection[] = ['eq', 'sound', 'phones']
const SECTION_ICONS: Record<QuickSection, LucideIcon> = { eq: SlidersHorizontal, sound: AudioLines, phones: Headphones }
const SOUND_ICONS: Record<Sound, LucideIcon> = { natural: Leaf, streaming: Radio, loud: Volume2, night: Moon }

interface Item {
  /** `<sección>/<id>` (lo que se guarda en `Quick.hidden`) */
  key: string
  name: string
  sub?: string
  icon?: LucideIcon
  on: boolean
  pick: () => void
}

const setQuick = (patch: Partial<Quick>): void => {
  const cfg = useStore.getState().config
  if (cfg) updateConfig({ quick: { ...cfg.quick, ...patch } }, false)
}

/** Modo rápido: botones grandes con los presets de las secciones elegidas, para la salida de la barra de título.
 *  Al elegir uno, la ventana se esconde en la bandeja (si así se quiere). */
export function QuickPage(): React.JSX.Element {
  const t = useT()
  const quick = useStore((s) => s.config!.quick)
  const [editing, setEditing] = useState(false)
  // Lo de esta vista va siempre a la salida: si se venía de la página del micrófono, se sale de ella
  const onMic = useStore((s) => s.page === 'mic')
  useEffect(() => {
    if (onMic) setState({ page: 'eq', selected: null })
  }, [onMic])

  const toggleSection = (sec: QuickSection): void => {
    const on = quick.sections.includes(sec)
    setQuick({ sections: on ? quick.sections.filter((x) => x !== sec) : SECTIONS.filter((x) => x === sec || quick.sections.includes(x)) })
  }

  return (
    <div className="page quick">
      <div className="quick-bar">
        <div className="quick-chips" role="group" aria-label={t('quick.show')}>
          {SECTIONS.map((sec) => {
            const Icon = SECTION_ICONS[sec]
            const on = quick.sections.includes(sec)
            return (
              <button key={sec} className={`quick-chip ${on ? 'on' : ''}`} aria-pressed={on} onClick={() => toggleSection(sec)}>
                <Icon size={14} />
                {t(`nav.${sec}`)}
              </button>
            )
          })}
        </div>
        <div className="spacer" />
        <label className="quick-opt">
          <span>{t('quick.hide')}</span>
          <Switch on={quick.hide} onChange={(hide) => setQuick({ hide })} label={t('quick.hide')} />
        </label>
        <button className={`btn btn-sm ${editing ? 'btn-primary' : ''}`} aria-pressed={editing} onClick={() => setEditing(!editing)}>
          <Pencil size={14} />
          {t(editing ? 'common.done' : 'quick.edit')}
        </button>
      </div>
      {editing && <small className="studio-hint">{t('quick.editHint')}</small>}
      {quick.sections.length === 0 && <p className="quick-empty">{t('quick.none')}</p>}
      {quick.sections.map((sec) => (
        <Group key={sec} section={sec} quick={quick} editing={editing} />
      ))}
    </div>
  )
}

function Group({ section, quick, editing }: { section: QuickSection; quick: Quick; editing: boolean }): React.JSX.Element | null {
  const t = useT()
  const items = useItems(section)
  const shown = editing ? items : items.filter((i) => !quick.hidden.includes(i.key))
  const Icon = SECTION_ICONS[section]

  const press = (i: Item): void => {
    if (editing) {
      setQuick({ hidden: quick.hidden.includes(i.key) ? quick.hidden.filter((k) => k !== i.key) : [...quick.hidden, i.key] })
      return
    }
    i.pick()
    // Un instante para que se vea cuál se ha puesto, y a la bandeja
    if (quick.hide) setTimeout(() => void api.windowHide(), 220)
  }

  return (
    <section>
      <div className="dyn-head">
        <span className="label quick-label">
          <Icon size={14} />
          {t(`nav.${section}`)}
        </span>
      </div>
      {shown.length === 0 ? (
        <p className="quick-empty">{t(items.length ? 'quick.allHidden' : `quick.empty.${section}`)}</p>
      ) : (
        <div className="quick-grid">
          {shown.map((i) => {
            const hidden = quick.hidden.includes(i.key)
            const ItemIcon = i.icon
            return (
              <button key={i.key} className={`quick-btn ${i.on ? 'on' : ''} ${editing && hidden ? 'off' : ''}`} aria-pressed={editing ? !hidden : i.on} onClick={() => press(i)}>
                {editing ? hidden ? <EyeOff size={16} className="quick-eye" /> : <Eye size={16} className="quick-eye" /> : ItemIcon && <ItemIcon size={18} className="quick-eye" />}
                <b>{i.name}</b>
                {i.sub && <small className="num">{i.sub}</small>}
              </button>
            )
          })}
        </div>
      )}
    </section>
  )
}

/** Los presets de una sección, de serie y propios, con cuál suena ahora en la salida */
function useItems(section: QuickSection): Item[] {
  const t = useT()
  const mineEq = useStore((s) => s.config?.presets)
  const eqActive = useStore((s) => currentCurve(s).preset)
  const mineSec = useStore((s) => sectionPresets(s, section === 'eq' ? 'sound' : section))
  const secActive = useStore((s) => (section === 'eq' ? null : activeSectionPreset(s, section)))
  const sound = useStore((s) => soundOf(outputSettings(s).dsp))
  const phonesOff = useStore((s) => sectionOff(s, 'phones'))

  return useMemo(() => {
    if (section === 'eq') {
      const builtin = BUILTIN.map((b) => ({ id: b.id, name: t(`presets.builtin.${b.id}`), curve: builtinCurve(b.id) }))
      return [...builtin, ...(mineEq ?? [])].map((p) => ({ key: `eq/${p.id}`, name: p.name, on: eqActive === p.id, pick: () => applyPreset(p.curve, p.id) }))
    }
    const mine: Item[] = mineSec.map((p) => ({ key: `${section}/${p.id}`, name: p.name, on: secActive === p.id, pick: () => applySectionPreset(section, p) }))
    if (section === 'sound') {
      const builtin: Item[] = SOUNDS.map((s) => ({
        key: `sound/s:${s}`,
        name: t(`dsp.sounds.${s}.name`),
        sub: t(`dsp.sounds.${s}.meta`),
        icon: SOUND_ICONS[s],
        on: !secActive && sound === s,
        pick: () => setOutputDsp(withSound(outputSettings().dsp, s))
      }))
      return [...builtin, ...mine]
    }
    const off: Item = {
      key: 'phones/s:off',
      name: t('quick.phonesOff'),
      on: phonesOff,
      pick: () => {
        const d = outputSettings().dsp
        setOutputDsp({ ...d, crossfeed: { ...d.crossfeed, on: false }, speakers: { ...d.speakers, on: false } })
      }
    }
    return [off, ...mine]
  }, [section, t, mineEq, eqActive, mineSec, secActive, sound, phonesOff])
}
