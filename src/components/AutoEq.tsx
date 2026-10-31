import { useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, LoaderCircle, Search } from 'lucide-react'
import { api, type AutoEqEntry } from '../lib/api'
import { curveDb, fmtDb, logFreqs, parseApo, type Band } from '../lib/eq'
import { currentCurve, savePreset, setCurve, setState, toast, useStore } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Modal } from './ui'

// Quién mide mejor va antes: oratory1990 es la referencia de AutoEq
const RANK: Record<string, number> = { oratory1990: 0, crinacle: 1, Rtings: 2, Innerfidelity: 3 }
const key = (s: string): string => s.toLowerCase().replace(/[^a-z0-9]/g, '')
// El índice se pide una vez por sesión; Rust lo guarda en caché una semana
let cache: AutoEqEntry[] | null = null

const close = (): void => setState({ modal: null })

/** Corrección de AutoEq + graves y agudos a gusto (los mismos shelves que usa AutoEq en su web) */
function withTaste(bands: Band[], bass: number, treble: number): Band[] {
  const extra: Band[] = []
  if (bass) extra.push({ kind: 'lowShelf', freq: 105, gain: bass, q: 0.7, on: true })
  if (treble) extra.push({ kind: 'highShelf', freq: 10000, gain: treble, q: 0.7, on: true })
  return [...bands, ...extra]
}

export function AutoEqModal(): React.JSX.Element | null {
  const open = useStore((s) => s.modal === 'autoeq')
  const [picked, setPicked] = useState<{ entry: AutoEqEntry; bands: Band[]; preamp: number } | null>(null)
  const [manual, setManual] = useState(false)
  useEffect(() => {
    setPicked(null)
    setManual(false)
  }, [open])
  if (!open) return null
  return (
    <Modal onClose={close} wide className="autoeq">
      {picked ? (
        <Detail picked={picked} back={() => setPicked(null)} />
      ) : manual ? (
        <Manual back={() => setManual(false)} />
      ) : (
        <Finder onPick={setPicked} onManual={() => setManual(true)} />
      )}
    </Modal>
  )
}

function Finder({ onPick, onManual }: { onPick: (p: { entry: AutoEqEntry; bands: Band[]; preamp: number }) => void; onManual: () => void }): React.JSX.Element {
  const t = useT()
  const [list, setList] = useState<AutoEqEntry[] | null>(cache)
  const [failed, setFailed] = useState(false)
  const [q, setQ] = useState('')
  const [hl, setHl] = useState(0)
  const [busy, setBusy] = useState<string | null>(null)
  const box = useRef<HTMLDivElement>(null)

  const load = (): void => {
    setFailed(false)
    api
      .autoeqIndex()
      .then((l) => {
        cache = l
        setList(l)
      })
      .catch((e: unknown) => {
        setFailed(true)
        toast(errorText(e), 'err')
      })
  }
  useEffect(() => {
    if (!cache) load()
  }, [])

  const results = useMemo(() => {
    const words = q.split(/\s+/).map(key).filter(Boolean)
    if (!list || !words.length) return []
    return list
      .filter((e) => {
        const k = key(e.name)
        return words.every((w) => k.includes(w))
      })
      .sort((a, b) => (RANK[a.source] ?? 9) - (RANK[b.source] ?? 9) || a.name.length - b.name.length || a.name.localeCompare(b.name))
      .slice(0, 60)
  }, [list, q])
  useEffect(() => setHl(0), [results])

  const pick = async (e: AutoEqEntry): Promise<void> => {
    setBusy(e.path)
    try {
      const r = parseApo(await api.autoeqFetch(e.path))
      if (!r) return toast(t('autoeq.bad'), 'err')
      onPick({ entry: e, ...r })
    } catch (err) {
      toast(errorText(err), 'err')
    } finally {
      setBusy(null)
    }
  }

  return (
    <>
      <h2>{t('autoeq.title')}</h2>
      <p>{t('autoeq.text')}</p>
      <div className="finder-search">
        <Search size={17} />
        <input
          className="input"
          autoFocus
          value={q}
          placeholder={t('autoeq.search')}
          spellCheck={false}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              e.preventDefault()
              const next = Math.max(0, Math.min(results.length - 1, hl + (e.key === 'ArrowDown' ? 1 : -1)))
              setHl(next)
              box.current?.children[next]?.scrollIntoView({ block: 'nearest' })
            }
            if (e.key === 'Enter' && results[hl]) void pick(results[hl])
          }}
        />
      </div>
      <div className="finder-list" ref={box}>
        {!list && !failed && (
          <div className="finder-note">
            <LoaderCircle size={16} className="spin" /> {t('autoeq.loading')}
          </div>
        )}
        {failed && (
          <div className="finder-note">
            <button className="btn btn-sm" onClick={load}>
              {t('common.retry')}
            </button>
          </div>
        )}
        {list && q.trim() && !results.length && <div className="finder-note">{t('autoeq.empty')}</div>}
        {results.map((e, i) => (
          <button key={e.path} className={`finder-item ${i === hl ? 'hl' : ''}`} onMouseEnter={() => setHl(i)} onClick={() => void pick(e)} disabled={!!busy}>
            <span className="txt">
              <b>{e.name}</b>
              <small>
                {t('autoeq.by', { s: e.source })} · {t(`autoeq.forms.${e.form}`)}
              </small>
            </span>
            {busy === e.path ? <LoaderCircle size={16} className="spin" /> : e.source === 'oratory1990' && <span className="chip ok">{t('autoeq.recommended')}</span>}
          </button>
        ))}
      </div>
      <div className="modal-actions">
        <button className="btn btn-ghost btn-sm" onClick={onManual}>
          {t('autoeq.manual')}
        </button>
        <div className="spacer" />
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
      </div>
    </>
  )
}

function Detail({ picked, back }: { picked: { entry: AutoEqEntry; bands: Band[]; preamp: number }; back: () => void }): React.JSX.Element {
  const t = useT()
  const [bass, setBass] = useState(0)
  const [treble, setTreble] = useState(0)
  const [name, setName] = useState(picked.entry.name)
  const bands = useMemo(() => withTaste(picked.bands, bass, treble), [picked, bass, treble])

  const apply = (): void => {
    setCurve({ ...currentCurve(), bands, preamp: picked.preamp })
    if (name.trim()) savePreset(name.trim().slice(0, 48))
    setState({ selected: null })
    toast(t('autoeq.ok', { n: bands.length }))
    close()
  }

  return (
    <>
      <div className="row" style={{ marginBottom: 4 }}>
        <button className="icon-btn" onClick={back} aria-label={t('autoeq.back')}>
          <ArrowLeft size={17} />
        </button>
        <h2 style={{ margin: 0 }}>{picked.entry.name}</h2>
      </div>
      <p className="small muted" style={{ marginLeft: 38 }}>
        {t('autoeq.by', { s: picked.entry.source })} · {t(`autoeq.forms.${picked.entry.form}`)}
      </p>
      <Preview bands={bands} />
      <div className="taste">
        <Taste label={t('autoeq.bass')} value={bass} min={-6} max={12} onChange={setBass} />
        <Taste label={t('autoeq.treble')} value={treble} min={-6} max={6} onChange={setTreble} />
      </div>
      <p className="hint">{t('autoeq.tasteHint')}</p>
      <label className="label" htmlFor="autoeq-name" style={{ marginTop: 14 }}>
        {t('autoeq.name')}
      </label>
      <input id="autoeq-name" className="input" maxLength={48} value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && apply()} />
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" onClick={apply}>
          {t('autoeq.import')}
        </button>
      </div>
    </>
  )
}

function Taste({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange: (v: number) => void }): React.JSX.Element {
  return (
    <label className="taste-row">
      <span>{label}</span>
      <input type="range" min={min} max={max} step={0.5} value={value} style={{ '--p': `${((value - min) / (max - min)) * 100}%` } as React.CSSProperties} onChange={(e) => onChange(+e.target.value)} onDoubleClick={() => onChange(0)} />
      <b className="num">{fmtDb(value)} dB</b>
    </label>
  )
}

const FREQS = logFreqs(140)
const PW = 600
const PH = 150

/** Mini curva para ver la corrección antes de aplicarla */
function Preview({ bands }: { bands: Band[] }): React.JSX.Element {
  const x = (i: number): number => (i / (FREQS.length - 1)) * PW
  const y = (db: number): number => PH / 2 - Math.max(-15, Math.min(15, db)) * (PH / 30)
  const d = FREQS.map((f, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)} ${y(curveDb(bands, f)).toFixed(1)}`).join('')
  const gx = (f: number): number => (Math.log(f / 20) / Math.log(1000)) * PW
  return (
    <svg className="preview" viewBox={`0 0 ${PW} ${PH}`} preserveAspectRatio="none">
      {[100, 1000, 10000].map((f) => (
        <line key={f} x1={gx(f)} x2={gx(f)} y1={0} y2={PH} />
      ))}
      <line className="zero" x1={0} x2={PW} y1={PH / 2} y2={PH / 2} />
      <path d={`${d}L${PW} ${PH / 2}L0 ${PH / 2}Z`} className="fill" />
      <path d={d} className="stroke" />
    </svg>
  )
}

function Manual({ back }: { back: () => void }): React.JSX.Element {
  const t = useT()
  const [text, setText] = useState('')
  const [name, setName] = useState('')
  const [over, setOver] = useState(false)
  const apply = (): void => {
    const r = parseApo(text)
    if (!r) return toast(t('autoeq.bad'), 'err')
    setCurve({ ...currentCurve(), bands: r.bands, preamp: r.preamp })
    if (name.trim()) savePreset(name.trim())
    setState({ selected: null })
    toast(t('autoeq.ok', { n: r.bands.length }))
    close()
  }
  return (
    <>
      <div className="row" style={{ marginBottom: 8 }}>
        <button className="icon-btn" onClick={back} aria-label={t('autoeq.back')}>
          <ArrowLeft size={17} />
        </button>
        <h2 style={{ margin: 0 }}>{t('autoeq.manual')}</h2>
      </div>
      <p>{t('autoeq.manualText')}</p>
      <textarea
        autoFocus
        className={`textarea mono autoeq-text ${over ? 'over' : ''}`}
        value={text}
        placeholder={t('autoeq.ph')}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onDragOver={(e) => {
          e.preventDefault()
          setOver(true)
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault()
          setOver(false)
          const f = e.dataTransfer.files[0]
          if (f && f.size < 256 * 1024) void f.text().then(setText)
        }}
      />
      <label className="label" htmlFor="autoeq-manual-name" style={{ marginTop: 14 }}>
        {t('autoeq.name')}
      </label>
      <input id="autoeq-manual-name" className="input" maxLength={48} value={name} onChange={(e) => setName(e.target.value)} />
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!text.trim()} onClick={apply}>
          {t('autoeq.import')}
        </button>
      </div>
    </>
  )
}
