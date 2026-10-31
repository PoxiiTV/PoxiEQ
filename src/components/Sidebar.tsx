import { AudioWaveform, Headphones, House, Mic, Settings, SlidersVertical, Volume2 } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { SOUND_PARTS } from '../lib/dsp'
import { go, studioActive, useStore, type Page } from '../lib/store'
import { useT } from '../i18n'

const PAGES: [Page, LucideIcon][] = [
  ['eq', SlidersVertical],
  ['sound', Volume2],
  ['phones', Headphones],
  ['studio', AudioWaveform],
  ['room', House],
  ['mic', Mic]
]

/** Punto junto a la sección cuando algo suyo está encendido: ámbar si es momentáneo y cambia lo que oyes */
type Dot = 'on' | 'warn' | null

function useDots(): Record<Page, Dot> {
  const sound = useStore((s) => !!s.config && SOUND_PARTS.some((k) => s.config!.dsp[k].on))
  const phones = useStore((s) => !!s.config && (s.config.dsp.crossfeed.on || s.config.dsp.speakers.on))
  const studio = useStore((s) => studioActive(s.studio))
  const mic = useStore((s) => {
    const m = s.config?.mic
    return !!m && !m.bypass && (m.dsp.gate.on || m.dsp.comp.on)
  })
  return { eq: null, sound: sound ? 'on' : null, phones: phones ? 'on' : null, studio: studio ? 'warn' : null, room: null, mic: mic ? 'on' : null, settings: null }
}

/** Menú principal: una sección por tarea y Ajustes abajo del todo */
export function Sidebar(): React.JSX.Element {
  const t = useT()
  const page = useStore((s) => s.page)
  const dots = useDots()
  const item = (p: Page, Icon: LucideIcon): React.JSX.Element => (
    <button key={p} className={`nav-item ${page === p ? 'active' : ''}`} aria-current={page === p ? 'page' : undefined} onClick={() => go(p)}>
      <Icon size={17} />
      <span className="name">{t(`nav.${p}`)}</span>
      {dots[p] && <i className={`nav-dot ${dots[p]}`} role="img" aria-label={t(dots[p] === 'warn' ? 'nav.warn' : 'nav.on')} />}
    </button>
  )
  return (
    <nav className="sidebar" aria-label={t('nav.label')}>
      {PAGES.map(([p, Icon]) => item(p, Icon))}
      <div className="spacer" />
      {item('settings', Settings)}
    </nav>
  )
}
