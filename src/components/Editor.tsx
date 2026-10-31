import { useEffect, useRef, useState } from 'react'
import { Copy, Plus, Power, RotateCcw, Trash2, TriangleAlert } from 'lucide-react'
import { clamp, fmtDb, hasGain, headroom, MAX_F, MAX_G, MAX_Q, MIN_F, MIN_G, MIN_Q, tenBand, toApo } from '../lib/eq'
import { currentCurve, setCurve, setState, toast, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Select } from './controls'
import { Switch } from './ui'
import { addBand, bandColor, Graph, KINDS, removeBand, updateBand } from './Graph'

export function Editor(): React.JSX.Element {
  const t = useT()
  const curve = useStore((s) => currentCurve(s))
  const bypass = useStore((s) => !!s.config?.bypass)
  const clips = headroom(curve) > 0.05

  return (
    <div className="editor">
      {bypass && (
        <div className="banner">
          <Power size={16} />
          <span>{t('eq.bypassBanner')}</span>
          <div className="spacer" />
          <button className="btn btn-sm btn-primary" onClick={() => updateConfig({ bypass: false })}>
            {t('eq.bypassResume')}
          </button>
        </div>
      )}
      <div className="toolbar">
        <div className="preamp">
          <span className="label">{t('eq.preamp')}</span>
          <input
            type="range"
            min={-30}
            max={12}
            step={0.1}
            value={curve.preamp}
            disabled={curve.autoPreamp}
            aria-label={t('eq.preamp')}
            style={{ '--p': `${((curve.preamp + 30) / 42) * 100}%` } as React.CSSProperties}
            onChange={(e) => setCurve({ ...curve, preamp: +e.target.value, autoPreamp: false }, true)}
          />
          <b className="num">{fmtDb(curve.preamp)} dB</b>
          <label className="auto" data-tip={t('eq.autoTip')}>
            <Switch on={curve.autoPreamp} onChange={(on) => setCurve({ ...curve, autoPreamp: on }, true)} label={t('eq.auto')} />
            {t('eq.auto')}
          </label>
          {clips && (
            <span className="clip" data-tip={t('eq.clip')}>
              <TriangleAlert size={16} />
            </span>
          )}
        </div>
        <div className="spacer" />
        <button className="icon-btn" data-tip={t('eq.reset')} aria-label={t('eq.reset')} onClick={() => setCurve({ ...curve, bands: tenBand() })}>
          <RotateCcw size={16} />
        </button>
        <button
          className="icon-btn"
          data-tip={t('eq.copy')}
          aria-label={t('eq.copy')}
          onClick={() => void navigator.clipboard.writeText(toApo(curve)).then(() => toast(t('eq.copied')))}
        >
          <Copy size={16} />
        </button>
        <button className="btn btn-sm" onClick={() => addBand()}>
          <Plus size={15} /> {t('eq.add')}
        </button>
      </div>
      <Graph />
      <div className="hint-line small muted">{t('eq.hint')}</div>
      <Bands />
    </div>
  )
}

function Bands(): React.JSX.Element {
  const t = useT()
  const bands = useStore((s) => currentCurve(s).bands)
  const selected = useStore((s) => s.selected)
  const strip = useRef<HTMLDivElement>(null)
  // Al elegir un punto en la curva, su tarjeta se pone a la vista
  useEffect(() => {
    if (selected !== null) strip.current?.children[selected]?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [selected])
  return (
    <div className="bands" ref={strip}>
      {bands.map((b, i) => (
        <div
          key={i}
          className={`band ${i === selected ? 'on' : ''} ${b.on ? '' : 'off'}`}
          style={{ '--c': bandColor(i) } as React.CSSProperties}
          onMouseDown={() => setState({ selected: i })}
        >
          <div className="band-head">
            <span className="band-n">{i + 1}</span>
            <Select size="sm" value={b.kind} options={KINDS.map((k) => ({ value: k, label: t(`eq.kinds.${k}`) }))} onChange={(kind) => updateBand(i, { kind })} label={t('eq.kinds.peak')} />
            <button className={`icon-btn ${b.on ? 'on' : ''}`} aria-label={t('eq.on')} aria-pressed={b.on} onClick={() => updateBand(i, { on: !b.on })}>
              <Power size={14} />
            </button>
            <button className="icon-btn" aria-label={t('eq.remove')} onClick={() => removeBand(i)}>
              <Trash2 size={14} />
            </button>
          </div>
          <Num label={t('eq.freq')} unit="Hz" value={b.freq} min={MIN_F} max={MAX_F} step={(v) => v * 2 ** (1 / 12) - v} digits={b.freq < 100 ? 1 : 0} onChange={(freq) => updateBand(i, { freq })} />
          <Num label={t('eq.gain')} unit="dB" value={b.gain} min={MIN_G} max={MAX_G} step={() => 0.5} digits={1} disabled={!hasGain(b.kind)} onChange={(gain) => updateBand(i, { gain })} />
          <Num label={t('eq.q')} unit="" value={b.q} min={MIN_Q} max={MAX_Q} step={(v) => v * 0.1} digits={2} onChange={(q) => updateBand(i, { q })} />
        </div>
      ))}
    </div>
  )
}

/** Número editable: se escribe libre y se aplica con Enter o al salir; las flechas suben y bajan */
function Num({
  label,
  unit,
  value,
  min,
  max,
  step,
  digits,
  disabled,
  onChange
}: {
  label: string
  unit: string
  value: number
  min: number
  max: number
  step: (v: number) => number
  digits: number
  disabled?: boolean
  onChange: (v: number) => void
}): React.JSX.Element {
  const [draft, setDraft] = useState<string | null>(null)
  useEffect(() => setDraft(null), [value])
  const commit = (): void => {
    if (draft === null) return
    const v = parseFloat(draft.replace(',', '.').replace(/k$/i, 'e3'))
    if (Number.isFinite(v)) onChange(+clamp(v, min, max).toFixed(digits))
    setDraft(null)
  }
  return (
    <label className={`num-field ${disabled ? 'disabled' : ''}`}>
      <span>{label}</span>
      <input
        className="num"
        value={draft ?? (disabled ? '—' : value.toFixed(digits))}
        disabled={disabled}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          if (e.key === 'Escape') setDraft(null)
          if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault()
            const d = step(value) * (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 4 : 1)
            onChange(+clamp(value + d, min, max).toFixed(digits))
          }
        }}
      />
      <em>{unit}</em>
    </label>
  )
}
