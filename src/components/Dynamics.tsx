import { ArrowDown, AudioLines, AudioWaveform, DoorClosed, Gauge, Headphones, Leaf, LoaderCircle, Mic, Moon, Radio, ShieldCheck, Sparkles, Speaker, Volume2, Waves } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Dsp } from '../lib/api'
import { runSetup } from '../lib/actions'
import { soundOf, SOUNDS, withHeadphones, withSound, type Sound } from '../lib/dsp'
import { toast, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Knob, Range } from './controls'
import { Switch } from './ui'

const SOUND_ICONS: Record<Sound, LucideIcon> = { natural: Leaf, streaming: Radio, loud: Volume2, night: Moon }

const num = (v: number, digits = 1): string => v.toFixed(digits).replace('-', '−')
const db = (v: number): string => `${v > 0 ? '+' : ''}${num(v)} dB`

/** Cambia una parte de la dinámica de las salidas */
function useDsp(): [Dsp | undefined, <K extends keyof Dsp>(key: K, patch: Partial<Dsp[K]>) => void] {
  const dsp = useStore((s) => s.config?.dsp)
  const set = <K extends keyof Dsp>(key: K, patch: Partial<Dsp[K]>): void => {
    const cur = useStore.getState().config?.dsp
    if (cur) updateConfig({ dsp: { ...cur, [key]: { ...cur[key], ...patch } } })
  }
  return [dsp, set]
}

/** Encabezado de un apartado de página (con número si sigue un orden) */
export function SectionHead({ n, label, children }: { n?: number; label: string; children?: React.ReactNode }): React.JSX.Element {
  return (
    <div className="dyn-head">
      <span className="label">{n ? `${n} · ${label}` : label}</span>
      {children}
    </div>
  )
}

/** Página «Sonido»: sonoridad, la cadena de dinámica en el orden de la señal y los realces. Todo a la vista. */
export function SoundPage(): React.JSX.Element | null {
  const t = useT()
  const [dsp, set] = useDsp()
  if (!dsp) return null
  const sound = soundOf(dsp)

  return (
    <div className="page">
      <Install />
      <section>
        <SectionHead n={1} label={t('dsp.sound')}>
          {sound === null && <span className="chip">{t('dsp.custom')}</span>}
        </SectionHead>
        <div className="sounds">
          {SOUNDS.map((s) => {
            const Icon = SOUND_ICONS[s]
            return (
              <button key={s} className={`sound ${sound === s ? 'on' : ''}`} aria-pressed={sound === s} onClick={() => updateConfig({ dsp: withSound(dsp, s) })}>
                <span className="sound-top">
                  <Icon size={18} />
                  <em className="num">{t(`dsp.sounds.${s}.meta`)}</em>
                </span>
                <b>{t(`dsp.sounds.${s}.name`)}</b>
                <small>{t(`dsp.sounds.${s}.text`)}</small>
              </button>
            )
          })}
        </div>
      </section>

      <section>
        <SectionHead n={2} label={t('dsp.chain')} />
        <small className="studio-hint">{t('dsp.chainHint')}</small>
        <div className="chain">
          <Card icon={AudioLines} title={t('dsp.leveler')} hint={t('dsp.levelerHint')} on={dsp.leveler.on} onToggle={(on) => set('leveler', { on })}>
            <Slider label={t('dsp.target')} value={dsp.leveler.target} min={-36} max={-6} fmt={(v) => `${num(v, 0)} LUFS`} onChange={(target) => set('leveler', { target })} />
            <Slider label={t('dsp.maxGain')} value={dsp.leveler.maxGain} min={0} max={24} fmt={db} onChange={(maxGain) => set('leveler', { maxGain })} />
          </Card>
          <ChainArrow />
          <Card icon={Gauge} title={t('dsp.comp')} hint={t('dsp.compHint')} on={dsp.comp.on} onToggle={(on) => set('comp', { on })}>
            <CompSliders comp={dsp.comp} set={(patch) => set('comp', patch)} />
          </Card>
          <ChainArrow />
          <Card icon={ShieldCheck} title={t('dsp.limiter')} hint={t('dsp.limiterHint')} on={dsp.limiter.on} onToggle={(on) => set('limiter', { on })}>
            <Slider label={t('dsp.ceiling')} value={dsp.limiter.ceiling} min={-12} max={0} step={0.1} fmt={(v) => `${num(v)} dBTP`} onChange={(ceiling) => set('limiter', { ceiling })} />
          </Card>
        </div>
      </section>

      <section>
        <SectionHead n={3} label={t('dsp.extras')} />
        <div className="dyn-cards extras">
          <Card icon={Waves} title={t('dsp.lows')} hint={t('dsp.lowsHint')} on={dsp.bassBoost.on} onToggle={(on) => set('bassBoost', { on })} className="knob-card">
            <Knob label={t('dsp.amount')} value={Math.round(dsp.bassBoost.amount * 100)} fmt={(v) => `${v} %`} onChange={(v) => set('bassBoost', { amount: v / 100 })} />
          </Card>
          <Card icon={AudioWaveform} title={t('dsp.bass')} hint={t('dsp.bassHint')} on={dsp.bass.on} onToggle={(on) => set('bass', { on })}>
            <Slider label={t('dsp.amount')} value={dsp.bass.amount * 100} min={0} max={100} fmt={(v) => `${Math.round(v)} %`} onChange={(v) => set('bass', { amount: v / 100 })} />
            <Slider label={t('dsp.below')} value={dsp.bass.freq} min={40} max={200} step={5} fmt={(v) => `${Math.round(v)} Hz`} onChange={(freq) => set('bass', { freq })} />
          </Card>
          <Card icon={Mic} title={t('dsp.dialog')} hint={t('dsp.dialogHint')} on={dsp.dialog.on} onToggle={(on) => set('dialog', { on })}>
            <Slider label={t('dsp.boost')} value={dsp.dialog.amount} min={0} max={12} step={0.5} fmt={db} onChange={(amount) => set('dialog', { amount })} />
          </Card>
        </div>
      </section>
    </div>
  )
}

function ChainArrow(): React.JSX.Element {
  return (
    <div className="chain-arrow" aria-hidden>
      <ArrowDown size={14} />
    </div>
  )
}

/** Crossfeed y altavoces virtuales (página «Cascos»): uno u otro, encender uno apaga el otro */
export function HeadphoneCards(): React.JSX.Element | null {
  const t = useT()
  const [dsp, set] = useDsp()
  if (!dsp) return null
  return (
    <div className="dyn-cards pair">
      <Card icon={Headphones} title={t('dsp.crossfeed')} hint={t('dsp.crossfeedHint')} on={dsp.crossfeed.on} onToggle={(on) => updateConfig({ dsp: withHeadphones(dsp, 'crossfeed', on) })}>
        <Slider label={t('dsp.intensity')} value={dsp.crossfeed.feed} min={1} max={15} step={0.5} fmt={(v) => `${num(v)} dB`} onChange={(feed) => set('crossfeed', { feed })} />
        <Slider label={t('dsp.cutoff')} value={dsp.crossfeed.freq} min={300} max={2000} step={50} fmt={(v) => `${Math.round(v)} Hz`} onChange={(freq) => set('crossfeed', { freq })} />
      </Card>
      <Card icon={Speaker} title={t('dsp.speakers')} hint={t('dsp.speakersHint')} on={dsp.speakers.on} onToggle={(on) => updateConfig({ dsp: withHeadphones(dsp, 'speakers', on) })}>
        <Slider label={t('dsp.intensity')} value={dsp.speakers.amount * 100} min={0} max={100} fmt={(v) => `${Math.round(v)} %`} onChange={(v) => set('speakers', { amount: v / 100 })} />
      </Card>
    </div>
  )
}

/** Compresor: los mismos controles en las salidas y en el micrófono */
function CompSliders({ comp, set }: { comp: Dsp['comp']; set: (patch: Partial<Dsp['comp']>) => void }): React.JSX.Element {
  const t = useT()
  return (
    <>
      <Slider label={t('dsp.threshold')} value={comp.threshold} min={-60} max={0} fmt={(v) => `${num(v, 0)} dB`} onChange={(threshold) => set({ threshold })} />
      <Slider label={t('dsp.ratio')} value={comp.ratio} min={1} max={20} step={0.1} fmt={(v) => `${num(v)}:1`} onChange={(ratio) => set({ ratio })} />
      <Slider label={t('dsp.attack')} value={comp.attack} min={0.1} max={200} log fmt={(v) => `${num(v, v < 10 ? 1 : 0)} ms`} onChange={(attack) => set({ attack })} />
      <Slider label={t('dsp.release')} value={comp.release} min={10} max={2000} log fmt={(v) => `${Math.round(v)} ms`} onChange={(release) => set({ release })} />
      <Slider label={t('dsp.knee')} value={comp.knee} min={0} max={24} fmt={(v) => `${Math.round(v)} dB`} onChange={(knee) => set({ knee })} />
      <Slider label={t('dsp.makeup')} value={comp.makeup} min={0} max={24} step={0.5} fmt={db} onChange={(makeup) => set({ makeup })} />
    </>
  )
}

/** Puerta de ruido y compresor del micrófono, antes de que lo reciba cualquier app (en ese orden) */
export function MicDynamics(): React.JSX.Element | null {
  const t = useT()
  const mic = useStore((s) => s.config?.mic)
  if (!mic) return null
  const set = <K extends 'gate' | 'comp'>(key: K, patch: Partial<Dsp[K]>): void => updateConfig({ mic: { ...mic, dsp: { ...mic.dsp, [key]: { ...mic.dsp[key], ...patch } } } })
  const g = mic.dsp.gate
  return (
    <div className="chain">
      <Card icon={DoorClosed} title={t('mic.gate')} hint={t('mic.gateHint')} on={g.on} onToggle={(on) => set('gate', { on })}>
        <Slider label={t('dsp.threshold')} value={g.threshold} min={-90} max={0} fmt={(v) => `${num(v, 0)} dBFS`} onChange={(threshold) => set('gate', { threshold })} />
        <Slider label={t('mic.range')} value={g.range} min={0} max={90} fmt={(v) => `${num(-v, 0)} dB`} onChange={(range) => set('gate', { range })} />
        <Slider label={t('dsp.attack')} value={g.attack} min={0.1} max={50} log fmt={(v) => `${num(v, v < 10 ? 1 : 0)} ms`} onChange={(attack) => set('gate', { attack })} />
        <Slider label={t('mic.hold')} value={g.hold} min={0} max={2000} step={10} fmt={(v) => `${Math.round(v)} ms`} onChange={(hold) => set('gate', { hold })} />
        <Slider label={t('dsp.release')} value={g.release} min={5} max={2000} log fmt={(v) => `${Math.round(v)} ms`} onChange={(release) => set('gate', { release })} />
      </Card>
      <ChainArrow />
      <Card icon={Gauge} title={t('dsp.comp')} hint={t('mic.compHint')} on={mic.dsp.comp.on} onToggle={(on) => set('comp', { on })}>
        <CompSliders comp={mic.dsp.comp} set={(patch) => set('comp', patch)} />
      </Card>
    </div>
  )
}

/** Aviso cuando el procesador no está instalado o es de otra versión (un permiso de administrador) */
export function Install(): React.JSX.Element | null {
  const t = useT()
  const state = useStore((s) => s.engine?.dsp)
  const step = useStore((s) => s.setup)
  if (!state || state === 'current') return null
  const old = state === 'old'
  return (
    <div className="banner">
      <Sparkles size={16} />
      <span>{t(old ? 'dsp.update' : 'dsp.install')}</span>
      <div className="spacer" />
      <button className="btn btn-sm btn-primary" disabled={!!step} onClick={() => void runSetup([], []).then((ok) => ok && toast(t('dsp.ready')))}>
        {step && <LoaderCircle size={14} className="spin" />}
        {t(old ? 'dsp.updateBtn' : 'dsp.installBtn')}
      </button>
    </div>
  )
}

function Card({
  icon: Icon,
  title,
  hint,
  on,
  onToggle,
  className = '',
  children
}: {
  icon: LucideIcon
  title: string
  hint: string
  on: boolean
  onToggle: (on: boolean) => void
  className?: string
  children: React.ReactNode
}): React.JSX.Element {
  return (
    <div className={`dyn-card ${className} ${on ? 'on' : ''}`} role="group" aria-label={title}>
      <div className="dyn-card-head">
        <Icon size={16} />
        <b>{title}</b>
        <Switch on={on} onChange={onToggle} label={title} />
      </div>
      <small>{hint}</small>
      <div className="dyn-sliders">{children}</div>
    </div>
  )
}

const LOG_STEPS = 1000

/** Deslizador con etiqueta y valor; `log` reparte la escala en logaritmo (tiempos de ataque/relajación) */
export function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  log,
  fmt,
  onChange
}: {
  label: string
  value: number
  min: number
  max: number
  step?: number
  log?: boolean
  fmt: (v: number) => string
  onChange: (v: number) => void
}): React.JSX.Element {
  const toPos = (v: number): number => (Math.log(v / min) / Math.log(max / min)) * LOG_STEPS
  const fromPos = (p: number): number => {
    const v = min * (max / min) ** (p / LOG_STEPS)
    return +v.toFixed(v < 10 ? 1 : 0)
  }
  return (
    <label className="dyn-slider">
      <span>{label}</span>
      {log ? (
        <Range value={toPos(value)} min={0} max={LOG_STEPS} label={label} onChange={(p) => onChange(fromPos(p))} />
      ) : (
        <Range value={value} min={min} max={max} step={step} label={label} onChange={onChange} />
      )}
      <b className="num">{fmt(value)}</b>
    </label>
  )
}
