import { useEffect, useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Download, ExternalLink, Sparkles } from 'lucide-react'
import { changesFor } from '../lib/changelog'
import { parseApo } from '../lib/eq'
import { currentCurve, savePreset, setCurve, setState, toast, updateConfig, useStore } from '../lib/store'
import { installUpdate, isPortable, RELEASES, useUpdate } from '../lib/update'
import { currentLang, errorText, useT } from '../i18n'
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

export function AutoEqModal(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.modal === 'autoeq')
  const [text, setText] = useState('')
  const [name, setName] = useState('')
  const [over, setOver] = useState(false)
  useEffect(() => {
    setText('')
    setName('')
  }, [open])
  if (!open) return null
  const apply = (): void => {
    const r = parseApo(text)
    if (!r) return toast(t('autoeq.bad'), 'err')
    const c = currentCurve()
    setCurve({ ...c, bands: r.bands, preamp: r.preamp })
    if (name.trim()) savePreset(name.trim())
    toast(t('autoeq.ok', { n: r.bands.length }))
    setState({ selected: null })
    close()
  }
  return (
    <Modal onClose={close} wide>
      <h2>{t('autoeq.title')}</h2>
      <p>{t('autoeq.text')}</p>
      <button className="btn btn-sm" style={{ marginBottom: 14 }} onClick={() => void openUrl('https://autoeq.app')}>
        <ExternalLink size={14} /> {t('autoeq.open')}
      </button>
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
      <label className="label" htmlFor="autoeq-name" style={{ marginTop: 14 }}>
        {t('autoeq.name')}
      </label>
      <input id="autoeq-name" className="input" maxLength={48} value={name} placeholder={t('autoeq.namePh')} onChange={(e) => setName(e.target.value)} />
      <div className="modal-actions">
        <button className="btn btn-ghost" onClick={close}>
          {t('common.cancel')}
        </button>
        <button className="btn btn-primary" disabled={!text.trim()} onClick={apply}>
          {t('autoeq.import')}
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
