import { useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, LoaderCircle, Search } from 'lucide-react'
import { api, type AutoEqEntry } from '../lib/api'
import { autoPreamp, curveDb, fmtDb, logFreqs, parseApo, type Band } from '../lib/eq'
import { currentCurve, savePreset, setCurve, setState, toast } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Select } from './controls'

// Quién mide mejor va antes: oratory1990 es la referencia de AutoEq
const RANK: Record<string, number> = { oratory1990: 0, crinacle: 1, Rtings: 2, Innerfidelity: 3 }
const key = (s: string): string => s.toLowerCase().replace(/[^a-z0-9]/g, '')
// El índice se pide una vez por sesión; Rust lo guarda en caché una semana
let cache: AutoEqEntry[] | null = null

/** Corrección de AutoEq + graves y agudos a gusto (los mismos shelves que usa AutoEq en su web) */
function withTaste(bands: Band[], bass: number, treble: number): Band[] {
  const extra: Band[] = []
  if (bass) extra.push({ kind: 'lowShelf', freq: 105, gain: bass, q: 0.7, on: true })
  if (treble) extra.push({ kind: 'highShelf', freq: 10000, gain: treble, q: 0.7, on: true })
  return [...bands, ...extra]
}

type Picked = { entry: AutoEqEntry; bands: Band[]; preamp: number }

/** Objetivos que se pueden elegir (los mismos que `autoeq::TARGET_FILES`); `autoeq` = el suyo, `like` = otros cascos */
const TARGETS = ['autoeq', 'Harman over-ear 2018', 'Harman in-ear 2019', 'Diffuse field 5128', 'Diffuse field ISO 11904-1', 'AutoEq in-ear', 'like'] as const
type Target = (typeof TARGETS)[number]

/** Banco de medida: fuente y equipo (las dos primeras partes de la ruta del índice) */
const rig = (path: string): string => path.split('/').slice(0, 2).join('/')

/** Buscador de AutoEq dentro de la página «Cascos»: buscar, ver la corrección y aplicarla (o pegar el archivo) */
export function AutoEqPanel(): React.JSX.Element {
  const [picked, setPicked] = useState<Picked | null>(null)
  const [manual, setManual] = useState(false)
  const reset = (): void => {
    setPicked(null)
    setManual(false)
  }
  return (
    <div className="dyn-card autoeq-panel">
      {picked ? <Detail picked={picked} back={reset} /> : manual ? <Manual back={reset} /> : <Finder onPick={setPicked} onManual={() => setManual(true)} />}
    </div>
  )
}

function Finder({ onPick, onManual }: { onPick: (p: Picked) => void; onManual: () => void }): React.JSX.Element {
  const t = useT()
  const [list, setList] = useState<AutoEqEntry[] | null>(cache)
  const [failed, setFailed] = useState(false)
  const [q, setQ] = useState('')
  const [hl, setHl] = useState(0)
  const [busy, setBusy] = useState<string | null>(null)
  const box = useRef<HTMLDivElement>(null)
  const requested = useRef(false)

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
  // La lista se pide al ir a buscar, no solo por abrir la página
  const loadOnce = (): void => {
    if (!cache && !list && !requested.current) {
      requested.current = true
      load()
    }
  }

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
      <small>{t('autoeq.text')}</small>
      <div className="finder-search">
        <Search size={17} />
        <input
          className="input"
          value={q}
          aria-label={t('autoeq.title')}
          onFocus={loadOnce}
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
      <div className={`finder-list ${q.trim() ? '' : 'idle'}`} ref={box}>
        {!list && !failed && q.trim() && (
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
      <div className="panel-actions start">
        <button className="btn btn-ghost btn-sm" onClick={onManual}>
          {t('autoeq.manual')}
        </button>
      </div>
    </>
  )
}

function Detail({ picked, back }: { picked: Picked; back: () => void }): React.JSX.Element {
  const t = useT()
  const [bass, setBass] = useState(0)
  const [treble, setTreble] = useState(0)
  const [name, setName] = useState(picked.entry.name)
  const [target, setTarget] = useState<Target>('autoeq')
  const [like, setLike] = useState<AutoEqEntry | null>(null)
  const [fitted, setFitted] = useState<Band[] | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    setFitted(null)
    if (target === 'autoeq' || (target === 'like' && !like)) return
    let live = true
    setBusy(true)
    api
      .autoeqFit(picked.entry.path, target === 'like' ? null : target, target === 'like' ? like!.path : null)
      .then((b) => live && setFitted(b))
      .catch((e) => live && toast(errorText(e), 'err'))
      .finally(() => live && setBusy(false))
    return () => {
      live = false
    }
  }, [picked, target, like])
  const base = fitted ?? picked.bands
  const bands = useMemo(() => withTaste(base, bass, treble), [base, bass, treble])
  const preamp = fitted ? autoPreamp(bands) : picked.preamp

  const apply = (): void => {
    if (target !== 'autoeq' && !fitted) return
    setCurve({ ...currentCurve(), bands, preamp })
    if (name.trim()) savePreset(name.trim().slice(0, 48))
    setState({ selected: null })
    toast(t('autoeq.ok', { n: bands.length }))
    back()
  }

  return (
    <>
      <div className="panel-title">
        <button className="icon-btn" onClick={back} aria-label={t('autoeq.back')}>
          <ArrowLeft size={17} />
        </button>
        <div className="txt">
          <b>{picked.entry.name}</b>
          <small>
            {t('autoeq.by', { s: picked.entry.source })} · {t(`autoeq.forms.${picked.entry.form}`)}
          </small>
        </div>
      </div>
      <div className="target-row">
        <Select
          size="sm"
          value={target}
          label={t('autoeq.target')}
          options={TARGETS.map((x) => ({ value: x, label: x === 'autoeq' || x === 'like' ? t(`autoeq.targets.${x}`) : x }))}
          onChange={setTarget}
        />
        {busy && <LoaderCircle size={16} className="spin" />}
      </div>
      {target === 'like' && <LikePicker own={picked.entry} value={like} onPick={setLike} />}
      {target === 'like' && like && rig(like.path) !== rig(picked.entry.path) && <p className="hint warn">{t('autoeq.rigWarn')}</p>}
      {target !== 'autoeq' && target !== 'like' && <p className="hint">{t('autoeq.targetHint')}</p>}
      <Preview bands={bands} />
      <div className="taste">
        <Taste label={t('autoeq.bass')} value={bass} min={-6} max={12} onChange={setBass} />
        <Taste label={t('autoeq.treble')} value={treble} min={-6} max={6} onChange={setTreble} />
      </div>
      <p className="hint">{t('autoeq.tasteHint')}</p>
      <label className="label" htmlFor="autoeq-name" style={{ marginTop: 8 }}>
        {t('autoeq.name')}
      </label>
      <input id="autoeq-name" className="input" maxLength={48} value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && apply()} />
      <div className="panel-actions">
        <button className="btn btn-ghost" onClick={back}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={target !== 'autoeq' && !fitted} onClick={apply}>
          {t('autoeq.import')}
        </button>
      </div>
    </>
  )
}

/** «Que suene como…»: otro modelo del índice (el de la misma fuente y banco, primero) */
function LikePicker({ own, value, onPick }: { own: AutoEqEntry; value: AutoEqEntry | null; onPick: (e: AutoEqEntry) => void }): React.JSX.Element {
  const t = useT()
  const [q, setQ] = useState('')
  const results = useMemo(() => {
    const words = q.split(/\s+/).map(key).filter(Boolean)
    if (!cache || !words.length) return []
    return cache
      .filter((e) => e.path !== own.path && words.every((w) => key(e.name).includes(w)))
      .sort((a, b) => Number(rig(b.path) === rig(own.path)) - Number(rig(a.path) === rig(own.path)) || a.name.length - b.name.length)
      .slice(0, 8)
  }, [q, own])
  return (
    <div className="like-pick">
      <div className="finder-search">
        <Search size={17} />
        <input className="input" value={q} placeholder={value ? value.name : t('autoeq.likeSearch')} onChange={(e) => setQ(e.target.value)} aria-label={t('autoeq.targets.like')} />
      </div>
      {results.map((e) => (
        <button
          key={e.path}
          className={`finder-item ${value?.path === e.path ? 'hl' : ''}`}
          onClick={() => {
            onPick(e)
            setQ('')
          }}
        >
          <span className="txt">
            <b>{e.name}</b>
            <small>{t('autoeq.by', { s: e.source })}</small>
          </span>
        </button>
      ))}
    </div>
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
export function Preview({ bands }: { bands: Band[] }): React.JSX.Element {
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
    back()
  }
  return (
    <>
      <div className="panel-title">
        <button className="icon-btn" onClick={back} aria-label={t('autoeq.back')}>
          <ArrowLeft size={17} />
        </button>
        <div className="txt">
          <b>{t('autoeq.manual')}</b>
          <small>{t('autoeq.manualText')}</small>
        </div>
      </div>
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
      <label className="label" htmlFor="autoeq-manual-name" style={{ marginTop: 8 }}>
        {t('autoeq.name')}
      </label>
      <input id="autoeq-manual-name" className="input" maxLength={48} value={name} onChange={(e) => setName(e.target.value)} />
      <div className="panel-actions">
        <button className="btn btn-ghost" onClick={back}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!text.trim()} onClick={apply}>
          {t('autoeq.import')}
        </button>
      </div>
    </>
  )
}
