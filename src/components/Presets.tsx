import { useEffect, useRef, useState } from 'react'
import { Check, ChevronDown, Pencil, Share2, Trash2 } from 'lucide-react'
import type { Preset } from '../lib/api'
import { BUILTIN, builtinCurve } from '../lib/eq'
import { MAX_NAME } from '../lib/share'
import { applyPreset, currentCurve, deletePreset, renamePreset, setState, useStore } from '../lib/store'
import { useT } from '../i18n'

const NONE: Preset[] = []

/** Desplegable de presets: los de serie y «Mis presets», con el que suena marcado. Los propios se renombran,
 *  comparten y borran desde el propio menú (sin diálogos del navegador). */
export function PresetMenu(): React.JSX.Element {
  const t = useT()
  const active = useStore((s) => currentCurve(s).preset)
  const mine = useStore((s) => s.config?.presets ?? NONE)
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const root = useRef<HTMLDivElement>(null)
  const menu = useRef<HTMLDivElement>(null)

  const builtin = BUILTIN.map((b) => ({ id: b.id, name: t(`presets.builtin.${b.id}`) }))
  const current = builtin.find((p) => p.id === active)?.name ?? mine.find((p) => p.id === active)?.name

  useEffect(() => {
    if (!open) return setEditing(null)
    // Al abrir, el foco va al que suena (o al primero) para moverse con las flechas
    const items = menu.current?.querySelectorAll<HTMLElement>('.pm-pick')
    ;(menu.current?.querySelector<HTMLElement>('.pm-pick[aria-checked="true"]') ?? items?.[0])?.focus()
    const onDown = (e: MouseEvent): void => {
      if (root.current?.contains(e.target as Node)) return
      // Un nombre a medio cambiar se guarda (su `blur`) antes de que el menú desaparezca
      ;(document.activeElement as HTMLElement | null)?.blur()
      setOpen(false)
    }
    window.addEventListener('mousedown', onDown)
    return () => window.removeEventListener('mousedown', onDown)
  }, [open])

  const close = (): void => {
    setOpen(false)
    root.current?.querySelector<HTMLElement>('.dd-btn')?.focus()
  }
  const choose = (id: string, curve: Preset['curve']): void => {
    applyPreset(curve, id)
    close()
  }
  const commitName = (): void => {
    if (editing && draft.trim()) renamePreset(editing, draft)
    setEditing(null)
  }
  const onKey = (e: React.KeyboardEvent): void => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      close()
    } else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !(e.target instanceof HTMLInputElement)) {
      e.preventDefault()
      const items = [...(menu.current?.querySelectorAll<HTMLElement>('.pm-pick') ?? [])]
      const i = items.indexOf(document.activeElement as HTMLElement)
      items[(i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus()
    }
  }

  return (
    <div
      ref={root}
      className={`dd pm ${open ? 'open' : ''}`}
      onKeyDown={open ? onKey : undefined}
      // Con Tab fuera del menú, se cierra
      onBlur={(e) => open && e.relatedTarget && !root.current?.contains(e.relatedTarget) && setOpen(false)}
    >
      <button type="button" className="dd-btn" aria-haspopup="menu" aria-expanded={open} aria-label={`${t('presets.menu')}: ${current ?? t('presets.custom')}`} onClick={() => (open ? close() : setOpen(true))}>
        <span className="pm-label">{t('presets.title')}</span>
        <b className="pm-name">{current ?? t('presets.custom')}</b>
        <ChevronDown size={15} />
      </button>
      {open && (
        <div ref={menu} className="dd-menu pm-menu" role="menu">
          <div className="dd-group" role="presentation">
            {t('presets.builtinTitle')}
          </div>
          {builtin.map((p) => (
            <div key={p.id} className={`pm-row ${active === p.id ? 'on' : ''}`}>
              <button type="button" className="pm-pick" role="menuitemradio" aria-checked={active === p.id} onClick={() => choose(p.id, builtinCurve(p.id))}>
                <span className="name">{p.name}</span>
                {active === p.id && <Check size={14} />}
              </button>
            </div>
          ))}
          <div className="dd-group" role="presentation">
            {t('presets.mine')}
          </div>
          {mine.length === 0 && <div className="pm-empty small muted">{t('presets.mineEmpty')}</div>}
          {mine.map((p) =>
            editing === p.id ? (
              <div key={p.id} className="pm-row editing">
                <input
                  className="input pm-input"
                  autoFocus
                  value={draft}
                  maxLength={MAX_NAME}
                  spellCheck={false}
                  aria-label={t('presets.rename')}
                  onChange={(e) => setDraft(e.target.value)}
                  onBlur={commitName}
                  onFocus={(e) => e.currentTarget.select()}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') commitName()
                    if (e.key === 'Escape') {
                      e.stopPropagation()
                      setEditing(null)
                    }
                  }}
                />
              </div>
            ) : (
              <div key={p.id} className={`pm-row own ${active === p.id ? 'on' : ''}`}>
                <button type="button" className="pm-pick" role="menuitemradio" aria-checked={active === p.id} onClick={() => choose(p.id, p.curve)}>
                  <span className="name">{p.name}</span>
                  {active === p.id && <Check size={14} />}
                </button>
                <span className="pm-actions">
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={`${t('presets.rename')}: ${p.name}`}
                    onClick={() => {
                      setDraft(p.name)
                      setEditing(p.id)
                    }}
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={`${t('share.title')}: ${p.name}`}
                    onClick={() => {
                      setOpen(false)
                      setState({ sharing: p })
                    }}
                  >
                    <Share2 size={14} />
                  </button>
                  <button type="button" className="icon-btn" aria-label={`${t('common.delete')}: ${p.name}`} onClick={() => deletePreset(p.id)}>
                    <Trash2 size={14} />
                  </button>
                </span>
              </div>
            )
          )}
        </div>
      )}
    </div>
  )
}
