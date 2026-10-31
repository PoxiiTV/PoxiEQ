import { useEffect, useRef, useState } from 'react'
import { Ear, Minus, Plus, TriangleAlert } from 'lucide-react'
import { api } from '../lib/api'
import { clamp, fmtFreq } from '../lib/eq'
import { ASYMMETRY, asymmetry, EARS, FREQS, hearingCurve, MAX_LEVEL, MIN_LEVEL, ORDER, START_LEVEL, type Ear as EarSide, type Thresholds } from '../lib/hearing'
import { applyPreset, getState, importPreset, setState, STUDIO_OFF, toast, updateConfig, useStore } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Range } from './controls'
import { Modal } from './ui'
import { Preview } from './AutoEq'

/** Tono de prueba: pitidos (más fáciles de distinguir que un tono fijo) en un solo oído. El nivel nunca pasa de
 *  `MAX_LEVEL` y los cambios van con rampas cortas para que no haya clics. */
class Tone {
  private ctx = new AudioContext()
  private osc = this.ctx.createOscillator()
  private env = this.ctx.createGain()
  private gain = this.ctx.createGain()
  private merger = this.ctx.createChannelMerger(2)
  private timer: number

  constructor() {
    this.env.gain.value = 0
    this.gain.gain.value = 0
    this.osc.connect(this.env).connect(this.gain)
    this.merger.connect(this.ctx.destination)
    this.osc.start()
    let on = false
    this.timer = window.setInterval(() => {
      on = !on
      this.env.gain.setTargetAtTime(on ? 1 : 0, this.ctx.currentTime, 0.012)
    }, 320)
  }

  set(freq: number, ear: EarSide, db: number): void {
    const now = this.ctx.currentTime
    this.osc.frequency.setValueAtTime(freq, now)
    this.gain.disconnect()
    this.gain.connect(this.merger, 0, ear === 'left' ? 0 : 1)
    this.gain.gain.setTargetAtTime(10 ** (clamp(db, MIN_LEVEL, MAX_LEVEL) / 20), now, 0.03)
  }

  stop(): void {
    window.clearInterval(this.timer)
    void this.ctx.close()
  }
}

const STEPS = EARS.length * ORDER.length
const fresh = (): Thresholds => ({ left: FREQS.map(() => MAX_LEVEL), right: FREQS.map(() => MAX_LEVEL) })

export function HearingModal(): React.JSX.Element | null {
  const open = useStore((s) => s.modal === 'hearing')
  return open ? <HearingTest /> : null
}

/** Asistente del test de audición. Mientras dura, el EQ y la dinámica van en plano (lo hace Rust con
 *  `hearing_set`) y al cerrar, por lo que sea, vuelve todo como estaba. */
function HearingTest(): React.JSX.Element {
  const t = useT()
  const [step, setStep] = useState<'intro' | 'test' | 'result'>('intro')
  const [pos, setPos] = useState(0)
  const [level, setLevel] = useState(START_LEVEL)
  const [thr, setThr] = useState(fresh)
  const tone = useRef<Tone | null>(null)
  /** El sonido está en plano por el test (hay que devolverlo al salir) */
  const flat = useRef(false)
  const ear = EARS[Math.floor(pos / ORDER.length)]
  const idx = ORDER[pos % ORDER.length]

  /** Silencio y el sonido de siempre */
  const finish = (): void => {
    tone.current?.stop()
    tone.current = null
    if (flat.current) void api.hearingSet(false)
    flat.current = false
  }
  // Al desmontar (cerrar, cancelar, esconder la ventana…) no se queda nada sonando ni en plano
  useEffect(() => finish, [])
  useEffect(() => {
    if (step === 'test') tone.current?.set(FREQS[idx], ear, level)
  }, [step, idx, ear, level])

  const close = (): void => setState({ modal: null })

  const start = async (): Promise<void> => {
    // El audio se crea ya en el clic (en silencio): después de esperar a Rust podría no dejar arrancarlo
    tone.current ??= new Tone()
    try {
      await api.hearingSet(true)
      flat.current = true
    } catch (e) {
      finish()
      return toast(errorText(e), 'err')
    }
    setState({ studio: STUDIO_OFF })
    setThr(fresh())
    setPos(0)
    setLevel(START_LEVEL)
    setStep('test')
  }

  const next = (heard: boolean): void => {
    const value = heard ? level : MAX_LEVEL
    const done = { ...thr, [ear]: thr[ear].map((v, i) => (i === idx ? value : v)) }
    setThr(done)
    if (pos + 1 < STEPS) {
      setPos(pos + 1)
      setLevel(START_LEVEL)
      return
    }
    finish()
    setStep('result')
  }

  const save = (): void => {
    const name = t('hearing.preset')
    const curve = hearingCurve(thr)
    const cfg = getState().config
    const old = cfg?.presets.find((p) => p.name === name)
    // Repetir el test sustituye «Mi audición» en vez de amontonar copias
    if (cfg && old) {
      updateConfig({ presets: cfg.presets.map((p) => (p.id === old.id ? { ...p, curve: { ...curve, preset: old.id } } : p)) })
      applyPreset(curve, old.id)
    } else importPreset(name, curve)
    toast(t('hearing.saved'))
    close()
  }

  if (step === 'intro')
    return (
      <Modal onClose={close} className="hearing">
        <h2>{t('hearing.title')}</h2>
        <p>{t('hearing.intro')}</p>
        <ul className="hearing-points">
          <li>{t('hearing.p1')}</li>
          <li>{t('hearing.p2')}</li>
          <li>{t('hearing.p3')}</li>
        </ul>
        <div className="hearing-warn">
          <TriangleAlert size={16} />
          <span>{t('hearing.medical')}</span>
        </div>
        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={close}>
            {t('common.cancel')}
          </button>
          <button className="btn btn-primary" autoFocus onClick={() => void start()}>
            {t('hearing.begin')}
          </button>
        </div>
      </Modal>
    )

  if (step === 'test') {
    const nudge = (d: number): void => setLevel((v) => clamp(v + d, MIN_LEVEL, MAX_LEVEL))
    return (
      <Modal onClose={close} className="hearing">
        <div className="hearing-top">
          <span className="chip">
            <Ear size={13} /> {t(`hearing.${ear}`)}
          </span>
          <span className="muted small num">
            {pos + 1} / {STEPS}
          </span>
        </div>
        <div className="hearing-bar" aria-hidden>
          <i style={{ width: `${(pos / STEPS) * 100}%` }} />
        </div>
        <div className="hearing-freq num">
          {fmtFreq(FREQS[idx])}
          <small>Hz</small>
        </div>
        <p className="hearing-ask">{t('hearing.ask')}</p>
        <div
          className="hearing-level"
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              next(true)
            }
          }}
        >
          <button className="icon-btn" aria-label={t('hearing.down')} onClick={() => nudge(-1)}>
            <Minus size={16} />
          </button>
          <Range value={level} min={MIN_LEVEL} max={MAX_LEVEL} step={1} label={t('hearing.level')} onChange={setLevel} />
          <button className="icon-btn" aria-label={t('hearing.up')} onClick={() => nudge(1)}>
            <Plus size={16} />
          </button>
          <b className="num">{level.toFixed(0).replace('-', '−')} dBFS</b>
        </div>
        <small className="muted hearing-keys">{t('hearing.keys')}</small>
        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={close}>
            {t('common.cancel')}
          </button>
          <div className="spacer" />
          <button className="btn btn-ghost" onClick={() => next(false)}>
            {t('hearing.notHeard')}
          </button>
          <button className="btn btn-primary" onClick={() => next(true)}>
            {t('hearing.heard')}
          </button>
        </div>
      </Modal>
    )
  }

  const curve = hearingCurve(thr)
  const uneven = asymmetry(thr) >= ASYMMETRY
  return (
    <Modal onClose={close} wide className="hearing">
      <h2>{t('hearing.resultTitle')}</h2>
      <p>{t('hearing.resultText')}</p>
      <div className="hearing-curves">
        {EARS.map((e) => (
          <div key={e}>
            <span className="label">{t(`hearing.${e}`)}</span>
            <Preview bands={[...curve.bands, ...(curve.ears?.[e] ?? [])]} />
          </div>
        ))}
      </div>
      {uneven && (
        <div className="hearing-warn">
          <TriangleAlert size={16} />
          <span>{t('hearing.uneven')}</span>
        </div>
      )}
      <small className="muted">{t('hearing.medical')}</small>
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.close')}
        </button>
        <div className="spacer" />
        <button className="btn btn-ghost" onClick={() => void start()}>
          {t('hearing.again')}
        </button>
        <button className="btn btn-primary" autoFocus onClick={save}>
          {t('hearing.save')}
        </button>
      </div>
    </Modal>
  )
}
