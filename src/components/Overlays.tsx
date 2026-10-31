import { useEffect, useMemo, useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Copy, Download, FileDown, Sparkles } from 'lucide-react'
import { api } from '../lib/api'
import { changesFor } from '../lib/changelog'
import { cleanName, decodePreset, encodePreset, MAX_FILE, MAX_NAME } from '../lib/share'
import { importPreset, savePreset, setState, toast, updateConfig, useStore } from '../lib/store'
import { installUpdate, isPortable, RELEASES, useUpdate } from '../lib/update'
import { currentLang, errorText, useT } from '../i18n'
import { Preview } from './AutoEq'
import { Logo, Modal } from './ui'

const close = (): void => setState({ modal: null })

export function SavePresetModal(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.modal === 'savePreset')
  const [name, setName] = useState('')
  useEffect(() => setName(''), [open])
  if (!open) return null
  const save = (): void => {
    if (!name.trim()) return
    savePreset(name.trim())
    toast(t('presets.saved'))
    close()
  }
  return (
    <Modal onClose={close}>
      <h2>{t('presets.saveTitle')}</h2>
      <label className="label" htmlFor="preset-name">
        {t('presets.name')}
      </label>
      <input id="preset-name" className="input" autoFocus maxLength={48} value={name} placeholder={t('presets.namePh')} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && save()} />
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!name.trim()} onClick={save}>
          {t('common.save')}
        </button>
      </div>
    </Modal>
  )
}

/** Compartir un preset: código para pegar en un chat o archivo `.poxieq` */
export function ShareModal(): React.JSX.Element | null {
  const t = useT()
  const preset = useStore((s) => s.sharing)
  const code = useMemo(() => (preset ? encodePreset(preset.name, preset.curve) : ''), [preset])
  const [busy, setBusy] = useState(false)
  if (!preset) return null
  const hide = (): void => setState({ sharing: null })
  const copy = (): void => void navigator.clipboard.writeText(code).then(() => toast(t('share.copied')))
  const save = async (): Promise<void> => {
    setBusy(true)
    try {
      toast(t('share.saved', { f: await api.presetExport(preset.name, code) }))
    } catch (e) {
      toast(errorText(e), 'err')
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal onClose={hide}>
      <h2>{t('share.title')}</h2>
      <p className="share-name">{preset.name}</p>
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

/** Importar un preset compartido: pegando el código o soltando el archivo `.poxieq` */
export function ImportModal(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.modal === 'import')
  const initial = useStore((s) => s.importCode)
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
  const [over, setOver] = useState(false)
  const parsed = useMemo(() => decodePreset(code), [code])
  useEffect(() => {
    if (open) setCode(initial)
  }, [open, initial])
  useEffect(() => setName(parsed?.name ?? ''), [parsed])
  if (!open) return null
  const hide = (): void => setState({ modal: null, importCode: '' })
  const ok = !!parsed && !!cleanName(name)
  const run = (): void => {
    if (!parsed || !ok) return
    importPreset(cleanName(name), parsed.curve)
    toast(t('share.imported', { n: cleanName(name) }))
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
        placeholder="PXEQ1.…"
        spellCheck={false}
        aria-label={t('share.code')}
        onChange={(e) => setCode(e.target.value)}
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
      {code.trim() && !parsed && <p className="hint danger-text">{t('share.bad')}</p>}
      {parsed && (
        <>
          <Preview bands={parsed.curve.bands} />
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
