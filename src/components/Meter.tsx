import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { ChevronLeft, ChevronRight, RotateCcw } from 'lucide-react'
import { api } from '../lib/api'
import { updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'

interface Levels {
  peak: [number, number]
  rms: [number, number]
  momentary: number
  shortTerm: number
  integrated: number
  truePeak: number
  truePeakMax: number
  correlation: number
  /** Reducción de ganancia de la dinámica (dB, 0 si no actúa) */
  grComp: number
  grLimit: number
}

const MIN = -60
const TICKS = [0, -6, -12, -18, -24, -36, -48, -60]
const HOLD_MS = 1500
/** Escala de la barra de reducción (de arriba abajo) y cuánto se queda a la vista después de actuar */
const GR_MAX = 24
const GR_HOLD_MS = 2000
const frac = (db: number): number => Math.min(1, Math.max(0, (db - MIN) / -MIN))
const fmt = (v: number): string => (v <= -119 ? '−∞' : v.toFixed(1).replace('-', '−'))

/** Columna fija de medidores de lo que sale de verdad (después del EQ): pico, RMS, LUFS, true peak, fase y,
 *  cuando la dinámica actúa, su reducción de ganancia */
export function Meter(): React.JSX.Element {
  const t = useT()
  const collapsed = useStore((s) => !!s.config?.meterCollapsed)
  const [lv, setLv] = useState<Levels | null>(null)
  const [clip, setClip] = useState(false)
  const hold = useRef([{ v: MIN, at: 0 }, { v: MIN, at: 0 }])
  const grSeen = useRef(-GR_HOLD_MS)

  useEffect(() => {
    const off = listen<Levels>('levels', (e) => {
      const now = performance.now()
      e.payload.peak.forEach((p, i) => {
        const h = hold.current[i]
        if (p >= h.v || now - h.at > HOLD_MS) hold.current[i] = { v: p, at: now }
      })
      // Saturación: el piloto se queda encendido hasta que lo pulses
      if (e.payload.truePeak > -0.1 || e.payload.peak.some((p) => p > -0.1)) setClip(true)
      if (e.payload.grComp + e.payload.grLimit > 0.05) grSeen.current = now
      setLv(e.payload)
    })
    return () => void off.then((f) => f())
  }, [])

  const reset = (): void => {
    setClip(false)
    hold.current = [{ v: MIN, at: 0 }, { v: MIN, at: 0 }]
    void api.meterReset()
  }

  const gr = (lv?.grComp ?? 0) + (lv?.grLimit ?? 0)
  const showGr = performance.now() - grSeen.current < GR_HOLD_MS
  const grTip = t('meter.grTip', { c: fmt(-(lv?.grComp ?? 0)), l: fmt(-(lv?.grLimit ?? 0)) })

  const bars = (
    <div className="meter-bars">
      <button className={`clip-led ${clip ? 'on' : ''}`} onClick={reset} aria-label={t('meter.clip')} data-tip={t('meter.clip')} />
      <div className="meter-scale">
        {TICKS.map((d) => (
          <span key={d} style={{ bottom: `${frac(d) * 100}%` }}>
            {d}
          </span>
        ))}
      </div>
      {[0, 1].map((i) => (
        <div key={i} className="meter-bar">
          <i className="rms" style={{ clipPath: `inset(${(1 - frac(lv?.rms[i] ?? MIN)) * 100}% 0 0 0 round 4px)` }} />
          <i className="pk" style={{ bottom: `${frac(lv?.peak[i] ?? MIN) * 100}%` }} />
          <i className="hold" style={{ bottom: `${frac(hold.current[i].v) * 100}%` }} />
          <em>{i ? 'R' : 'L'}</em>
        </div>
      ))}
      {showGr && (
        <div className="meter-bar gr" data-tip={grTip}>
          <i style={{ height: `${Math.min(1, gr / GR_MAX) * 100}%` }} />
          <em>GR</em>
        </div>
      )}
    </div>
  )

  if (collapsed) {
    return (
      <aside className="meter collapsed">
        <button className="icon-btn" onClick={() => updateConfig({ meterCollapsed: false })} aria-label={t('meter.expand')} data-tip={t('meter.expand')}>
          <ChevronLeft size={15} />
        </button>
        {bars}
      </aside>
    )
  }

  const tpBad = (lv?.truePeakMax ?? MIN) > -1
  return (
    <aside className="meter">
      <div className="meter-head">
        <span className="label">{t('meter.title')}</span>
        <button className="icon-btn" onClick={() => updateConfig({ meterCollapsed: true })} aria-label={t('meter.collapse')}>
          <ChevronRight size={15} />
        </button>
      </div>
      {bars}
      <div className="meter-read">
        <div className="big num">
          {fmt(lv?.momentary ?? MIN * 2)}
          <small>LUFS M</small>
        </div>
        <div className="line num" data-tip={t('meter.shortTip')}>
          <span>S</span>
          <b>{fmt(lv?.shortTerm ?? MIN * 2)}</b>
        </div>
        <div className="line num" data-tip={t('meter.intTip')}>
          <span>I</span>
          <b>{fmt(lv?.integrated ?? MIN * 2)}</b>
        </div>
        <div className={`line num ${tpBad ? 'bad' : ''}`} data-tip={t('meter.tpTip')}>
          <span>TP</span>
          <b>{fmt(lv?.truePeakMax ?? MIN * 2)}</b>
        </div>
        {showGr && (
          <div className="line num gr" data-tip={grTip}>
            <span>GR</span>
            <b>{fmt(-gr)}</b>
          </div>
        )}
        <div className="corr" data-tip={t('meter.corrTip')}>
          <div className="corr-track">
            <i style={{ left: `${(((lv?.correlation ?? 1) + 1) / 2) * 100}%` }} className={(lv?.correlation ?? 1) < 0 ? 'neg' : ''} />
          </div>
          <div className="corr-scale">
            <span>−1</span>
            <span>0</span>
            <span>+1</span>
          </div>
        </div>
        <button className="btn btn-sm btn-ghost" onClick={reset}>
          <RotateCcw size={13} /> {t('meter.reset')}
        </button>
      </div>
    </aside>
  )
}
