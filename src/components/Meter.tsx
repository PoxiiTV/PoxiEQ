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
}

const MIN = -60
const TICKS = [0, -6, -12, -18, -24, -36, -48, -60]
const HOLD_MS = 1500
const frac = (db: number): number => Math.min(1, Math.max(0, (db - MIN) / -MIN))
const fmt = (v: number): string => (v <= -119 ? '−∞' : v.toFixed(1).replace('-', '−'))

/** Columna fija de medidores de lo que sale de verdad (después del EQ): pico, RMS, LUFS, true peak y fase */
export function Meter(): React.JSX.Element {
  const t = useT()
  const collapsed = useStore((s) => !!s.config?.meterCollapsed)
  const [lv, setLv] = useState<Levels | null>(null)
  const [clip, setClip] = useState(false)
  const hold = useRef([{ v: MIN, at: 0 }, { v: MIN, at: 0 }])

  useEffect(() => {
    const off = listen<Levels>('levels', (e) => {
      const now = performance.now()
      e.payload.peak.forEach((p, i) => {
        const h = hold.current[i]
        if (p >= h.v || now - h.at > HOLD_MS) hold.current[i] = { v: p, at: now }
      })
      // Saturación: el piloto se queda encendido hasta que lo pulses
      if (e.payload.truePeak > -0.1 || e.payload.peak.some((p) => p > -0.1)) setClip(true)
      setLv(e.payload)
    })
    return () => void off.then((f) => f())
  }, [])

  const reset = (): void => {
    setClip(false)
    hold.current = [{ v: MIN, at: 0 }, { v: MIN, at: 0 }]
    void api.meterReset()
  }

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
