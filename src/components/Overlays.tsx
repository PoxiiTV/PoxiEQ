import { useEffect, useMemo, useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Copy, Download, FileDown, FileUp, LoaderCircle, Sparkles } from 'lucide-react'
import { api } from '../lib/api'
import { changesFor } from '../lib/changelog'
import { SECTION_PARTS, type Part } from '../lib/dsp'
import { cleanName, decodeShare, MAX_FILE, MAX_NAME, type Shared } from '../lib/share'
import { readEqFile } from '../lib/formats'
import { autoPreamp } from '../lib/eq'
import { importShared, savePreset, saveSectionPreset, setState, toast, updateConfig, useStore } from '../lib/store'
import { installUpdate, isPortable, RELEASES, useUpdate } from '../lib/update'
import { currentLang, errorText, useT } from '../i18n'
import { Preview } from './AutoEq'
import { Logo, Modal } from './ui'

const close = (): void => setState({ modal: null })

export function SavePresetModal(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.modal === 'savePreset')
  const section = useStore((s) => s.saveFor)
  const [name, setName] = useState('')
  useEffect(() => setName(''), [open])
  if (!open) return null
  const save = (): void => {
    const clean = cleanName(name)
    if (!clean) return
    if (section === 'eq') savePreset(clean)
    else if (!saveSectionPreset(section, clean)) return
    toast(t('presets.saved'))
    close()
  }
  return (
    <Modal onClose={close}>
      <h2>{t('presets.saveTitle')}</h2>
      <p className="hint">{t(`sections.${section}.saves`)}</p>
      <label className="label" htmlFor="preset-name">
        {t('presets.name')}
      </label>
      <input id="preset-name" className="input" autoFocus maxLength={MAX_NAME} value={name} placeholder={t(`sections.${section}.namePh`)} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && save()} />
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!cleanName(name)} onClick={save}>
          {t('common.save')}
        </button>
      </div>
    </Modal>
  )
}

/** Compartir un preset: código para pegar en un chat o archivo `.poxieq` */
export function ShareModal(): React.JSX.Element | null {
  const t = useT()
  const sharing = useStore((s) => s.sharing)
  const [busy, setBusy] = useState(false)
  if (!sharing) return null
  const { name, code } = sharing
  const hide = (): void => setState({ sharing: null })
  const copy = (): void => void navigator.clipboard.writeText(code).then(() => toast(t('share.copied')))
  const save = async (): Promise<void> => {
    setBusy(true)
    try {
      toast(t('share.saved', { f: await api.presetExport(name, code) }))
    } catch (e) {
      toast(errorText(e), 'err')
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal onClose={hide}>
      <h2>{t('share.title')}</h2>
      <p className="share-name">{name}</p>
      <p className="hint">{t('share.text')}</p>
      <textarea className="textarea mono share-code" readOnly value={code} spellCheck={false} aria-label={t('share.code')} onFocus={(e) => e.currentTarget.select()} />
      <div className="modal-actions">
        <button className="btn btn-ghost" disabled={busy} onClick={() => void save()}>
          <FileDown size={16} /> {t('share.file')}
        </button>
        <button className="btn btn-primary" autoFocus onClick={copy}>
          <Copy size={16} /> {t('share.copy')}
        </button>
      </div>
    </Modal>
  )
}

/** Qué trae un preset que no es una curva: lo que va encendido (o «todo apagado») */
function useSummary(): (sh: Shared) => string {
  const t = useT()
  const labels: Partial<Record<Part, string>> = {
    leveler: t('dsp.leveler'),
    comp: t('dsp.comp'),
    limiter: t('dsp.limiter'),
    bassBoost: t('dsp.lows'),
    bass: t('dsp.bass'),
    dialog: t('dsp.dialog'),
    crossfeed: t('dsp.crossfeed'),
    speakers: t('dsp.speakers'),
    gate: t('mic.gate')
  }
  return (sh) => {
    let on: string[] = []
    if (sh.section === 'studio') {
      const s = sh.studio
      on = [s.listen !== 'stereo' && t(`studio.listen.${s.listen}`), s.flipL && 'Ø L', s.flipR && 'Ø R', s.swap && t('studio.swap'), s.sim && t('studio.simulating', { s: t(`studio.sims.${s.sim}.name`) })].filter(
        (x): x is string => !!x
      )
    } else if (sh.section !== 'eq') {
      on = SECTION_PARTS[sh.section].filter((k) => sh.dsp[k].on).map((k) => labels[k] ?? k)
    }
    return on.length ? on.join(' · ') : t('share.allOff')
  }
}

/** Importar un preset compartido: pegando el código o soltando el archivo `.poxieq`. Va a la sección que dice
 *  el código (y se enseña esa sección). */
export function ImportModal(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.modal === 'import')
  const initial = useStore((s) => s.importCode)
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
  const [over, setOver] = useState(false)
  const [fromFile, setFromFile] = useState<Shared | null>(null)
  const [reading, setReading] = useState(false)
  const shared = useMemo(() => decodeShare(code), [code])
  const parsed = fromFile ?? shared
  const summary = useSummary()
  /** Un archivo de EQ de otro programa (APO, Peace, REW, AutoEq, EasyEffects, GraphicEQ): va al ecualizador */
  const openFile = async (): Promise<void> => {
    const path = await api.pickFile('eq')
    if (!path) return
    setReading(true)
    try {
      const f = readEqFile(await api.textRead(path))
      if (!f) return toast(t('share.fileBad'), 'err')
      const bands = f.kind === 'graphic' ? await api.graphicFit(f.points) : f.bands
      const preamp = f.kind === 'graphic' ? autoPreamp(bands) : f.preamp
      const name = cleanName(path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') ?? '') || 'EQ'
      setCode('')
      setFromFile({ section: 'eq', name, curve: { preamp, autoPreamp: false, bands, preset: null } })
    } catch (e) {
      toast(errorText(e), 'err')
    } finally {
      setReading(false)
    }
  }
  useEffect(() => {
    if (open) setCode(initial)
    setFromFile(null)
  }, [open, initial])
  useEffect(() => setName(parsed?.name ?? ''), [parsed])
  if (!open) return null
  const hide = (): void => setState({ modal: null, importCode: '' })
  const ok = !!parsed && !!cleanName(name)
  const run = (): void => {
    if (!parsed || !ok || !importShared(parsed, cleanName(name))) return
    toast(t('share.imported', { n: cleanName(name), s: t(`nav.${parsed.section}`) }))
    hide()
  }
  return (
    <Modal onClose={hide}>
      <h2>{t('share.importTitle')}</h2>
      <p className="hint">{t('share.importText')}</p>
      <textarea
        autoFocus={!parsed}
        className={`textarea mono share-code ${over ? 'over' : ''} ${code.trim() && !parsed ? 'bad' : ''}`}
        value={code}
        placeholder="PXEQ2.…"
        spellCheck={false}
        aria-label={t('share.code')}
        onChange={(e) => {
          setFromFile(null)
          setCode(e.target.value)
        }}
        onDragOver={(e) => {
          e.preventDefault()
          setOver(true)
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault()
          setOver(false)
          const f = e.dataTransfer.files[0]
          if (f && f.size <= MAX_FILE) void f.text().then(setCode)
        }}
      />
      <button className="btn btn-sm btn-ghost" disabled={reading} onClick={() => void openFile()}>
        {reading ? <LoaderCircle size={14} className="spin" /> : <FileUp size={14} />}
        {t('share.fromFile')}
      </button>
      {code.trim() && !parsed && <p className="hint danger-text">{t('share.bad')}</p>}
      {parsed && (
        <>
          <p className="share-kind">
            <span className="chip">{t('share.kind', { s: t(`nav.${parsed.section}`) })}</span>
            {parsed.section !== 'eq' && <span className="small muted">{summary(parsed)}</span>}
          </p>
          {(parsed.section === 'eq' || parsed.section === 'mic') && <Preview bands={parsed.curve.bands} />}
          <label className="label" htmlFor="import-name">
            {t('presets.name')}
          </label>
          <input id="import-name" className="input" maxLength={MAX_NAME} value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && run()} />
        </>
      )}
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={hide}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!ok} onClick={run}>
          {t('share.import')}
        </button>
      </div>
    </Modal>
  )
}

export function UpdateModal(): React.JSX.Element | null {
  const t = useT()
  const { update, version, progress } = useUpdate()
  if (!update) return null
  const portable = isPortable()
  const items = changesFor(version, currentLang())
  const hide = (): void => useUpdate.setState({ update: null })
  return (
    <Modal onClose={progress === null ? hide : undefined} wide>
      <div className="row" style={{ marginBottom: 6 }}>
        <Sparkles size={20} color="var(--accent)" />
        <h2 style={{ margin: 0 }}>{t('update.title', { v: version })}</h2>
      </div>
      {items.length > 0 ? <ChangeList items={items} /> : <p className="small">{useUpdate.getState().notes}</p>}
      {portable && <p className="hint">{t('update.portable')}</p>}
      <div className="modal-actions">
        <button className="btn btn-ghost" disabled={progress !== null} onClick={hide}>
          {t('update.later')}
        </button>
        {portable ? (
          <button className="btn btn-primary" onClick={() => void openUrl(RELEASES)}>
            <Download size={16} /> {t('update.download')}
          </button>
        ) : (
          <button className="btn btn-primary" disabled={progress !== null} onClick={() => void installUpdate().catch((e: unknown) => toast(errorText(e), 'err'))}>
            {progress !== null ? t('update.downloading', { p: progress }) : t('update.install')}
          </button>
        )}
      </div>
    </Modal>
  )
}

function ChangeList({ items }: { items: { emoji: string; title: string; text: string }[] }): React.JSX.Element {
  return (
    <div className="col" style={{ gap: 10, margin: '14px 0' }}>
      {items.map((c, i) => (
        <div key={i} className="row" style={{ alignItems: 'flex-start' }}>
          <span style={{ fontSize: 18 }}>{c.emoji}</span>
          <div className="small" style={{ lineHeight: 1.5 }}>
            <b>{c.title}</b>
            {c.text && <span className="muted"> · {c.text}</span>}
          </div>
        </div>
      ))}
    </div>
  )
}

/** Tras actualizar, las novedades de la versión (una vez); también se abre desde Ajustes */
export function WhatsNew(): React.JSX.Element | null {
  const t = useT()
  const app = useStore((s) => s.app)
  const seen = useStore((s) => s.config?.seenVersion)
  const [open, setOpen] = useState(false)
  useEffect(() => {
    if (!app || seen === undefined) return
    if (seen && seen !== app.version && changesFor(app.version, currentLang()).length) setOpen(true)
    if (seen !== app.version) updateConfig({ seenVersion: app.version })
  }, [app, seen])
  useEffect(() => {
    const on = (): void => setOpen(true)
    window.addEventListener('poxi:whatsnew', on)
    return () => window.removeEventListener('poxi:whatsnew', on)
  }, [])
  if (!open || !app) return null
  return (
    <Modal onClose={() => setOpen(false)} wide>
      <div className="row" style={{ marginBottom: 6 }}>
        <Logo size={26} />
        <h2 style={{ margin: 0 }}>{t('whatsNew.title', { v: app.version })}</h2>
      </div>
      <ChangeList items={changesFor(app.version, currentLang())} />
      <div className="modal-actions">
        <button className="btn btn-primary" onClick={() => setOpen(false)}>
          {t('common.done')}
        </button>
      </div>
    </Modal>
  )
}
