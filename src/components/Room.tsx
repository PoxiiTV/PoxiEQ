import { useEffect, useMemo, useState } from 'react'
import { CircleCheck, LoaderCircle, Mic, Ruler, Trash2, TriangleAlert, Upload } from 'lucide-react'
import { api, type Preset, type RoomFit, type RoomLevels, type RoomParams } from '../lib/api'
import { autoPreamp, fmtFreq } from '../lib/eq'
import { applyPreset, currentCurve, currentOutput, saveRoomPreset, setState, toast, updateConfig, useStore } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Range, Select } from './controls'
import { Preview } from './AutoEq'
import { SectionHead, Slider } from './Dynamics'
import { Modal } from './ui'
import { ConvolutionCard } from './Convolution'

/** Rango del nivel del barrido (dBFS de pico): el mismo tope que pone Rust (`room::MAX_LEVEL`) */
const MIN_LEVEL = -40
const MAX_LEVEL = -12
/** Lo que llega al micrófono: satura por encima de −1 dBFS y es flojo con menos de 25 dB sobre el ruido */
const CLIP = -1
const MIN_SNR = 25
const RANGES: { id: string; high: number }[] = [
  { id: 'r300', high: 300 },
  { id: 'r500', high: 500 },
  { id: 'full', high: 20000 }
]

const NONE: Preset[] = []

/** Página «Sala»: qué hace la corrección, el botón del asistente y lo último medido en la salida que se edita */
export function RoomPage(): React.JSX.Element {
  const t = useT()
  const output = useStore((s) => currentOutput(s))
  const presets = useStore((s) => s.config?.presets ?? NONE)
  const active = useStore((s) => currentCurve(s).preset)
  const last = output ? presets.findLast((p) => p.room === output.id) : undefined
  return (
    <div className="page">
      <section>
        <SectionHead label={t('room.title')} />
        <div className="dyn-card">
          <div className="dyn-card-head">
            <Ruler size={16} />
            <b>{t('room.cardTitle')}</b>
          </div>
          <small>{t('room.cardText')}</small>
          <small>{t('room.cardWhen')}</small>
          <div className="panel-actions start">
            <button className="btn btn-sm btn-primary" onClick={() => setState({ modal: 'room' })}>
              {t('room.start')}
            </button>
          </div>
        </div>
        <CalCard />
      </section>
      <section>
        <SectionHead label={t('room.last')} />
        {last ? (
          <div className="dyn-card">
            <div className="dyn-card-head">
              <b>{last.name}</b>
              {active === last.id && <span className="chip ok">{t('common.inUse')}</span>}
            </div>
            <small>{t('room.lastText', { n: last.curve.bands.length, d: output?.name ?? '' })}</small>
            <Preview bands={last.curve.bands} />
            {active !== last.id && (
              <div className="panel-actions start">
                <button className="btn btn-sm btn-primary" onClick={() => applyPreset(last.curve, last.id)}>
                  {t('room.apply')}
                </button>
              </div>
            )}
          </div>
        ) : (
          <small className="studio-hint">{t('room.none', { d: output?.name ?? '' })}</small>
        )}
      </section>
      <section>
        <SectionHead label={t('ir.section')} />
        <ConvolutionCard hint={t('ir.roomHint')} />
      </section>
    </div>
  )
}

/** Archivo de calibración del micro de medida: se resta a cada toma */
function CalCard(): React.JSX.Element {
  const t = useT()
  const cal = useStore((s) => s.config?.roomCal ?? null)
  const load = async (): Promise<void> => {
    const path = await api.pickFile('cal')
    if (!path) return
    try {
      updateConfig({ roomCal: await api.roomCalLoad(path) })
      toast(t('room.calLoaded'))
    } catch (e) {
      toast(errorText(e), 'err')
    }
  }
  return (
    <div className="set-row room-cal" role="group" aria-label={t('room.cal')}>
      <div className="txt">
        <b>{t('room.cal')}</b>
        <small>{cal ? cal.name : t('room.calHint')}</small>
      </div>
      <button className="btn btn-sm" onClick={() => void load()}>
        <Upload size={14} />
        {t(cal ? 'ir.replace' : 'room.calLoad')}
      </button>
      {cal && (
        <button className="icon-btn" onClick={() => updateConfig({ roomCal: null })} aria-label={t('ir.remove')} data-tip={t('ir.remove')}>
          <Trash2 size={15} />
        </button>
      )}
    </div>
  )
}

export function RoomModal(): React.JSX.Element | null {
  const open = useStore((s) => s.modal === 'room')
  return open ? <RoomWizard /> : null
}

type Verdict = 'ok' | 'quiet' | 'loud'
const verdict = (l: RoomLevels): Verdict => (l.peak > CLIP ? 'loud' : l.peak - l.noise < MIN_SNR ? 'quiet' : 'ok')

/** Asistente de corrección de sala. Mientras suena un barrido, Rust deja todo en plano y lo restaura al acabar
 *  (también si se cancela o se esconde la ventana). */
function RoomWizard(): React.JSX.Element {
  const t = useT()
  const devices = useStore((s) => s.engine?.devices)
  const outputs = useMemo(() => (devices ?? []).filter((d) => !d.input && !d.unplugged), [devices])
  const inputs = useMemo(() => (devices ?? []).filter((d) => d.input && !d.unplugged), [devices])
  const [output, setOutput] = useState(() => {
    return currentOutput()?.id ?? outputs[0]?.id ?? ''
  })
  const [input, setInput] = useState(() => inputs.find((d) => d.default)?.id ?? inputs[0]?.id ?? '')
  const [level, setLevel] = useState(-24)
  const [busy, setBusy] = useState<'check' | 'measure' | null>(null)
  const [levels, setLevels] = useState<RoomLevels | null>(null)
  const [takes, setTakes] = useState(0)
  const [step, setStep] = useState<'setup' | 'result'>('setup')
  const [params, setParams] = useState<RoomParams>({ smoothing: 1 / 6, slope: 0, high: 500, maxBoost: 3, filters: 8 })
  const [fit, setFit] = useState<RoomFit | null>(null)
  const outName = outputs.find((d) => d.id === output)?.name ?? ''
  const [name, setName] = useState('')

  // Cada vez que se abre empieza de cero; al cerrar se corta lo que esté sonando
  useEffect(() => {
    void api.roomReset()
    return () => void api.roomCancel()
  }, [])

  // El ajuste se rehace al mover cualquier control (sin volver a medir)
  useEffect(() => {
    if (step !== 'result') return
    const id = setTimeout(() => void api.roomFit(params).then(setFit, (e: unknown) => toast(errorText(e), 'err')), 120)
    return () => clearTimeout(id)
  }, [step, params])

  const close = (): void => setState({ modal: null })

  /** Otro altavoz u otro micrófono: lo medido ya no vale */
  const pick = (set: (id: string) => void) => (id: string) => {
    set(id)
    setTakes(0)
    setLevels(null)
    void api.roomReset()
  }

  const run = async (kind: 'check' | 'measure'): Promise<void> => {
    setBusy(kind)
    try {
      const r = kind === 'check' ? await api.roomCheck(output, input, level) : await api.roomMeasure(output, input, level)
      setLevels(r)
      if (kind === 'measure') setTakes(r.takes)
      if (r.odd) toast(t('room.odd'), 'info', { secs: 10 })
    } catch (e) {
      if ((e as { code?: string })?.code !== 'room.cancelled') toast(errorText(e), 'err', { secs: 8 })
    } finally {
      setBusy(null)
    }
  }

  const save = (): void => {
    if (!fit) return
    const n = name.trim() || t('room.defaultName', { d: outName })
    saveRoomPreset(n, { preamp: autoPreamp(fit.bands), autoPreamp: true, bands: fit.bands, preset: null }, output)
    toast(t('room.saved', { n, d: outName }))
    close()
  }

  if (step === 'result')
    return (
      <Modal onClose={close} wide className="room">
        <h2>{t('room.resultTitle')}</h2>
        <RoomGraph fit={fit} high={params.high} />
        <div className="room-legend small">
          <span className="measured">{t('room.measured')}</span>
          <span className="target">{t('room.target')}</span>
          <span className="corrected">{t('room.corrected')}</span>
        </div>
        <div className="room-controls">
          <div className="room-opt" role="group" aria-label={t('room.range')}>
            <span>{t('room.range')}</span>
            <div className="segmented">
              {RANGES.map((r) => (
                <button key={r.id} className={params.high === r.high ? 'on' : ''} onClick={() => setParams({ ...params, high: r.high })}>
                  {t(`room.ranges.${r.id}`)}
                </button>
              ))}
            </div>
          </div>
          <div className="room-opt" role="group" aria-label={t('room.smoothing')}>
            <span>{t('room.smoothing')}</span>
            <div className="segmented">
              {[3, 6].map((n) => (
                <button key={n} className={Math.abs(params.smoothing - 1 / n) < 1e-9 ? 'on' : ''} onClick={() => setParams({ ...params, smoothing: 1 / n })}>
                  1/{n} oct
                </button>
              ))}
            </div>
          </div>
          <Slider label={t('room.slope')} value={params.slope} min={-1.5} max={0} step={0.1} fmt={(v) => `${v.toFixed(1).replace('-', '−')} dB/oct`} onChange={(slope) => setParams({ ...params, slope })} />
          <Slider label={t('room.maxBoost')} value={params.maxBoost} min={0} max={6} step={0.5} fmt={(v) => `+${v.toFixed(1)} dB`} onChange={(maxBoost) => setParams({ ...params, maxBoost })} />
          <Slider label={t('room.filters')} value={params.filters} min={1} max={10} step={1} fmt={(v) => String(v)} onChange={(filters) => setParams({ ...params, filters })} />
        </div>
        {fit && (
          <p className="room-summary small">
            {t('room.summary', {
              n: fit.bands.length,
              c: fit.bands.filter((b) => b.gain < 0).length,
              b: fit.bands.filter((b) => b.gain > 0).length,
              f: fmtFreq(Math.round(fit.low))
            })}
          </p>
        )}
        <label className="room-name">
          <span className="label">{t('room.name')}</span>
          <input className="input" value={name} placeholder={t('room.defaultName', { d: outName })} maxLength={48} spellCheck={false} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={() => setStep('setup')}>
            {t('room.again')}
          </button>
          <div className="spacer" />
          <button className="btn btn-ghost" onClick={close}>
            {t('common.cancel')}
          </button>
          <button className="btn btn-primary" disabled={!fit?.bands.length} onClick={save}>
            {t('room.save', { d: outName })}
          </button>
        </div>
      </Modal>
    )

  const v = levels && verdict(levels)
  const ready = !!output && !!input && !busy
  return (
    <Modal onClose={close} wide className="room">
      <h2>{t('room.title')}</h2>
      <p>{t('room.intro')}</p>
      <ul className="hearing-points">
        <li>{t('room.p1')}</li>
        <li>{t('room.p2')}</li>
        <li>{t('room.p3')}</li>
      </ul>
      {inputs.length === 0 ? (
        <div className="hearing-warn">
          <Mic size={16} />
          <span>{t('room.noMic')}</span>
        </div>
      ) : (
        <div className="room-grid">
          <span className="label">{t('room.output')}</span>
          <Select block value={output} label={t('room.output')} options={outputs.map((d) => ({ value: d.id, label: `${d.name} · ${d.device}` }))} onChange={pick(setOutput)} />
          <span className="label">{t('room.input')}</span>
          <Select block value={input} label={t('room.input')} options={inputs.map((d) => ({ value: d.id, label: `${d.name} · ${d.device}` }))} onChange={pick(setInput)} />
          <span className="label">{t('room.level')}</span>
          <div className="set-range room-level">
            <Range value={level} min={MIN_LEVEL} max={MAX_LEVEL} step={1} label={t('room.level')} onChange={setLevel} />
            <b className="num">{level.toFixed(0).replace('-', '−')} dBFS</b>
          </div>
        </div>
      )}
      <div className="room-status" aria-live="polite">
        {busy ? (
          <span className="room-busy">
            <LoaderCircle size={15} className="spin" />
            {t(busy === 'check' ? 'room.checking' : 'room.measuring')}
          </span>
        ) : levels && v ? (
          <span className={`room-verdict ${v}`}>
            {v === 'ok' ? <CircleCheck size={15} /> : <TriangleAlert size={15} />}
            <span>
              {t(`room.${v}`)}{' '}
              <em className="num">{t('room.levels', { n: levels.noise.toFixed(0).replace('-', '−'), p: levels.peak.toFixed(0).replace('-', '−') })}</em>
            </span>
          </span>
        ) : (
          <span className="muted">{t('room.checkFirst')}</span>
        )}
      </div>
      <div className="room-takes">
        <div>
          <b>{t('room.takes', { n: takes })}</b>
          <small className="muted">{t('room.takesHint')}</small>
        </div>
        <button className="btn btn-sm" disabled={!ready} onClick={() => void run('check')}>
          {t('room.check')}
        </button>
        <button className="btn btn-sm btn-primary" disabled={!ready} onClick={() => void run('measure')}>
          {t('room.measure', { n: takes + 1 })}
        </button>
      </div>
      <div className="modal-actions">
        {busy ? (
          <button className="btn btn-ghost" onClick={() => void api.roomCancel()}>
            {t('room.cancel')}
          </button>
        ) : (
          <button className="btn btn-ghost" onClick={close}>
            {t('common.cancel')}
          </button>
        )}
        <div className="spacer" />
        <button className="btn btn-primary" disabled={!takes || !!busy} onClick={() => setStep('result')}>
          {t('room.result')}
        </button>
      </div>
    </Modal>
  )
}

const W = 640
const H = 230
const PAD = { l: 34, r: 8, t: 10, b: 20 }
const DB = 15
const LINES = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]

/** Medido, objetivo y corregido (20 Hz – 20 kHz, ±15 dB); sombreado lo que se corrige */
function RoomGraph({ fit, high }: { fit: RoomFit | null; high: number }): React.JSX.Element {
  const x = (f: number): number => PAD.l + (Math.log(f / 20) / Math.log(1000)) * (W - PAD.l - PAD.r)
  const y = (db: number): number => PAD.t + ((DB - Math.max(-DB, Math.min(DB, db))) / (2 * DB)) * (H - PAD.t - PAD.b)
  const path = (v: number[]): string => (fit ? fit.freqs.map((f, i) => `${i ? 'L' : 'M'}${x(Math.min(f, 20000)).toFixed(1)} ${y(v[i]).toFixed(1)}`).join('') : '')
  return (
    <svg className="room-graph" viewBox={`0 0 ${W} ${H}`} role="img">
      <rect className="span" x={x(20)} y={PAD.t} width={x(Math.min(high, 20000)) - x(20)} height={H - PAD.t - PAD.b} />
      {LINES.map((f) => (
        <g key={f} className="grid">
          <line x1={x(f)} x2={x(f)} y1={PAD.t} y2={H - PAD.b} />
          <text x={x(f)} y={H - 6} textAnchor="middle">
            {fmtFreq(f)}
          </text>
        </g>
      ))}
      {[-12, -6, 0, 6, 12].map((d) => (
        <g key={d} className={`grid ${d === 0 ? 'zero' : ''}`}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(d)} y2={y(d)} />
          <text x={PAD.l - 6} y={y(d) + 4} textAnchor="end">
            {d > 0 ? `+${d}` : d}
          </text>
        </g>
      ))}
      {fit && (
        <>
          <path d={path(fit.measured)} className="measured" />
          <path d={path(fit.target)} className="target" />
          <path d={path(fit.corrected)} className="corrected" />
        </>
      )}
    </svg>
  )
}
