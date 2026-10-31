import { useEffect, useMemo, useRef, useState } from 'react'
import { Check, Power, Trash2 } from 'lucide-react'
import { bandDb, clamp, curveDb, fmtDb, fmtFreq, hasGain, logFreqs, MAX_BANDS, MAX_F, MAX_G, MIN_F, MIN_G, type Band, type Kind } from '../lib/eq'
import { currentCurve, setCurve, setState, toast, useStore } from '../lib/store'
import { t as tr, useT } from '../i18n'
import { Analyzer } from './Analyzer'

const PAD = { l: 42, r: 18, t: 14, b: 26 }
const FREQ_LINES = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]
export const KINDS: Kind[] = ['peak', 'lowShelf', 'highShelf', 'lowPass', 'highPass']
const LOG_SPAN = Math.log(MAX_F / MIN_F)

/** Un color por banda, repartidos por la rueda para que dos vecinas nunca se parezcan */
export const bandColor = (i: number): string => `hsl(${(186 + i * 137.5) % 360} 80% 62%)`

export function updateBand(i: number, patch: Partial<Band>): void {
  const c = currentCurve()
  setCurve({ ...c, bands: c.bands.map((b, j) => (j === i ? { ...b, ...patch } : b)) })
}

export function removeBand(i: number): void {
  const c = currentCurve()
  setCurve({ ...c, bands: c.bands.filter((_, j) => j !== i) })
  setState({ selected: null })
}

export function addBand(freq = 1000, gain = 0): void {
  const c = currentCurve()
  if (c.bands.length >= MAX_BANDS) return toast(tr('eq.full', { n: MAX_BANDS }), 'info')
  const bands = [...c.bands, { kind: 'peak' as const, freq, gain, q: 1.41, on: true }]
  setCurve({ ...c, bands })
  setState({ selected: bands.length - 1 })
}

const roundFreq = (f: number): number => (f < 100 ? Math.round(f * 10) / 10 : f < 1000 ? Math.round(f) : Math.round(f / 10) * 10)
const roundGain = (g: number): number => Math.round(g * 10) / 10

export function Graph(): React.JSX.Element {
  const t = useT()
  const curve = useStore((s) => currentCurve(s))
  const selected = useStore((s) => s.selected)
  const bypass = useStore((s) => !!s.config?.bypass)
  const analyzer = useStore((s) => !!s.config?.analyzer)
  const mode = useStore((s) => s.config?.analyzerMode ?? 'spectrum')
  const box = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ w: 800, h: 360 })
  const [drag, setDrag] = useState<number | null>(null)
  const [hover, setHover] = useState<number | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; i: number } | null>(null)

  useEffect(() => {
    const el = box.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const W = Math.max(10, size.w - PAD.l - PAD.r)
  const H = Math.max(10, size.h - PAD.t - PAD.b)
  // Rango vertical: ±12 dB, y más si la curva lo pide (en pasos de 6)
  const range = useMemo(() => {
    let m = 9
    for (const b of curve.bands) if (b.on && hasGain(b.kind)) m = Math.max(m, Math.abs(b.gain))
    return Math.min(MAX_G, Math.ceil((m + 3) / 6) * 6)
  }, [curve.bands])

  const x = (f: number): number => PAD.l + (Math.log(f / MIN_F) / LOG_SPAN) * W
  const fx = (px: number): number => MIN_F * Math.exp(((px - PAD.l) / W) * LOG_SPAN)
  const y = (db: number): number => PAD.t + ((range - clamp(db, -range - 6, range + 6)) / (2 * range)) * H
  const dy = (py: number): number => range - ((py - PAD.t) / H) * 2 * range

  const freqs = useMemo(() => logFreqs(Math.round(Math.max(120, W / 2.5))), [W])
  const path = (fn: (f: number) => number): string => freqs.map((f, i) => `${i ? 'L' : 'M'}${x(f).toFixed(1)} ${y(fn(f)).toFixed(1)}`).join('')
  const total = path((f) => curveDb(curve.bands, f))
  const area = `${total}L${x(MAX_F)} ${y(0)}L${x(MIN_F)} ${y(0)}Z`
  const sel = selected !== null ? curve.bands[selected] : undefined

  const local = (e: React.PointerEvent | React.MouseEvent): { px: number; py: number } => {
    const r = box.current!.getBoundingClientRect()
    return { px: e.clientX - r.left, py: e.clientY - r.top }
  }

  const onMove = (e: React.PointerEvent): void => {
    if (drag === null) return
    const { px, py } = local(e)
    const b = currentCurve().bands[drag]
    if (!b) return
    updateBand(drag, {
      freq: roundFreq(clamp(fx(px), MIN_F, MAX_F)),
      ...(hasGain(b.kind) ? { gain: roundGain(clamp(dy(py), MIN_G, MAX_G)) } : {})
    })
  }

  const onWheel = (e: React.WheelEvent): void => {
    const i = hover ?? selected
    const b = i !== null ? curve.bands[i] : undefined
    if (i === null || !b) return
    const q = clamp(b.q * (e.deltaY < 0 ? 1.12 : 1 / 1.12), 0.1, 20)
    updateBand(i, { q: Math.round(q * 100) / 100 })
  }

  const onKey = (e: React.KeyboardEvent): void => {
    if (selected === null || !sel) return
    const step = e.shiftKey ? 4 : 1
    const moves: Record<string, () => Partial<Band>> = {
      ArrowLeft: () => ({ freq: roundFreq(clamp(sel.freq / 2 ** (step / 12), MIN_F, MAX_F)) }),
      ArrowRight: () => ({ freq: roundFreq(clamp(sel.freq * 2 ** (step / 12), MIN_F, MAX_F)) }),
      ArrowUp: () => ({ gain: roundGain(clamp(sel.gain + 0.5 * step, MIN_G, MAX_G)) }),
      ArrowDown: () => ({ gain: roundGain(clamp(sel.gain - 0.5 * step, MIN_G, MAX_G)) })
    }
    if (moves[e.key]) {
      e.preventDefault()
      updateBand(selected, moves[e.key]())
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      removeBand(selected)
    } else if (e.key === 'Escape') {
      setState({ selected: null })
    }
  }

  const tipBand = drag ?? hover
  const tip = tipBand !== null ? curve.bands[tipBand] : undefined

  return (
    <div
      ref={box}
      className={`graph ${bypass ? 'bypassed' : ''} ${drag !== null ? 'dragging' : ''}`}
      tabIndex={0}
      onKeyDown={onKey}
      onWheel={onWheel}
      onPointerMove={onMove}
      onPointerUp={() => setDrag(null)}
      onPointerDown={(e) => {
        if (e.button === 0 && e.target === e.currentTarget.querySelector('svg')) setState({ selected: null })
      }}
      onDoubleClick={(e) => {
        if ((e.target as Element).closest('.pt')) return
        const { px, py } = local(e)
        addBand(roundFreq(clamp(fx(px), MIN_F, MAX_F)), roundGain(clamp(dy(py), MIN_G, MAX_G)))
      }}
    >
      {analyzer && !bypass && <Analyzer key={mode} mode={mode} x={x} top={PAD.t} height={H} width={size.w} fullHeight={size.h} />}
      <svg width={size.w} height={size.h}>
        <defs>
          <linearGradient id="fill" x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="0.5" stopColor="var(--accent)" stopOpacity="0.04" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0.28" />
          </linearGradient>
        </defs>
        {FREQ_LINES.map((f) => (
          <g key={f} className="grid">
            <line x1={x(f)} x2={x(f)} y1={PAD.t} y2={PAD.t + H} />
            <text x={x(f)} y={size.h - 8} textAnchor="middle">
              {fmtFreq(f)}
            </text>
          </g>
        ))}
        {Array.from({ length: (2 * range) / 6 + 1 }, (_, k) => range - k * 6).map((db) => (
          <g key={db} className={`grid ${db === 0 ? 'zero' : ''}`}>
            <line x1={PAD.l} x2={PAD.l + W} y1={y(db)} y2={y(db)} />
            <text x={PAD.l - 8} y={y(db) + 4} textAnchor="end">
              {db > 0 ? `+${db}` : db}
            </text>
          </g>
        ))}
        <path d={area} fill="url(#fill)" className="area" />
        {sel && sel.on && <path d={path((f) => bandDb(sel, f))} className="band-curve" style={{ stroke: bandColor(selected!) }} />}
        <path d={total} className="curve" />
        {curve.bands.map((b, i) => (
          <g
            key={i}
            className={`pt ${i === selected ? 'on' : ''} ${b.on ? '' : 'off'}`}
            transform={`translate(${x(b.freq)} ${y(hasGain(b.kind) ? b.gain : 0)})`}
            style={{ '--c': bandColor(i) } as React.CSSProperties}
            onPointerEnter={() => setHover(i)}
            onPointerLeave={() => setHover((h) => (h === i ? null : h))}
            onPointerDown={(e) => {
              if (e.button !== 0) return
              e.stopPropagation()
              ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
              setState({ selected: i })
              setDrag(i)
            }}
            onContextMenu={(e) => {
              e.preventDefault()
              setState({ selected: i })
              const r = box.current!.getBoundingClientRect()
              setMenu({ x: Math.min(e.clientX - r.left, size.w - 200), y: Math.min(e.clientY - r.top, size.h - 250), i })
            }}
          >
            <circle r={11} className="halo" />
            <circle r={9} className="dot" />
            <text y={3.5} textAnchor="middle">
              {i + 1}
            </text>
          </g>
        ))}
      </svg>
      {tip && (
        <div className="graph-tip num" style={{ left: x(tip.freq), top: y(hasGain(tip.kind) ? tip.gain : 0) }}>
          {fmtFreq(tip.freq)} Hz{hasGain(tip.kind) && <> · {fmtDb(tip.gain)} dB</>} · Q {tip.q.toFixed(2)}
        </div>
      )}
      {!curve.bands.length && <div className="graph-empty">{t('eq.hint')}</div>}
      {menu && <PointMenu {...menu} close={() => setMenu(null)} />}
    </div>
  )
}

function PointMenu({ x, y, i, close }: { x: number; y: number; i: number; close: () => void }): React.JSX.Element | null {
  const t = useT()
  const band = useStore((s) => currentCurve(s).bands[i])
  useEffect(() => {
    const onDown = (e: MouseEvent): void => {
      if (!(e.target as HTMLElement).closest('.ctx')) close()
    }
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey)
    window.addEventListener('blur', close)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('blur', close)
    }
  }, [close])
  if (!band) return null
  const run = (fn: () => void) => () => {
    fn()
    close()
  }
  return (
    <div className="dd-menu ctx" style={{ position: 'absolute', left: x, top: y, right: 'auto', minWidth: 190, transformOrigin: 'top left' }} role="menu">
      {KINDS.map((k) => (
        <button key={k} role="menuitemradio" aria-checked={band.kind === k} className={band.kind === k ? 'on' : ''} onClick={run(() => updateBand(i, { kind: k }))}>
          {t(`eq.kinds.${k}`)}
          {band.kind === k && <Check size={14} />}
        </button>
      ))}
      <div className="ctx-sep" />
      <button role="menuitem" onClick={run(() => updateBand(i, { on: !band.on }))}>
        <span className="row" style={{ gap: 10 }}>
          <Power size={15} />
          {t('eq.on')}
        </span>
      </button>
      <button role="menuitem" className="danger-text" onClick={run(() => removeBand(i))}>
        <span className="row" style={{ gap: 10 }}>
          <Trash2 size={15} />
          {t('eq.remove')}
        </span>
      </button>
    </div>
  )
}
