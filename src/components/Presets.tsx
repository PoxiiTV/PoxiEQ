import { useEffect, useRef, useState } from 'react'
import { Check, ChevronDown, Import, Pencil, Save, Share2, Trash2 } from 'lucide-react'
import type { Preset, Section, SectionPreset } from '../lib/api'
import { BUILTIN, builtinCurve } from '../lib/eq'
import { MAX_NAME, type ShareSection } from '../lib/share'
import {
  activeSectionPreset,
  applyPreset,
  applySectionPreset,
  currentCurve,
  deletePreset,
  deleteSectionPreset,
  renamePreset,
  renameSectionPreset,
  sectionOff,
  sectionPresets,
  setState,
  sharePreset,
  useStore
} from '../lib/store'
import { useT } from '../i18n'

const NONE: Preset[] = []

interface Item {
  id: string
  name: string
}

/** Desplegable de presets: los de serie (si hay) y «Mis presets», con el que suena marcado. Los propios se
 *  renombran, comparten y borran desde el propio menú (sin diálogos del navegador). */
function PresetMenu({
  builtin = [],
  mine,
  active,
  idle,
  empty,
  onPick,
  onRename,
  onShare,
  onDelete
}: {
  builtin?: Item[]
  mine: Item[]
  active: string | null | undefined
  /** Texto cuando no suena ningún preset */
  idle: string
  empty: string
  onPick: (id: string) => void
  onRename: (id: string, name: string) => void
  /** Sin él, el preset no se comparte (p. ej. una cadena de plugins: las rutas son de este PC) */
  onShare?: (id: string) => void
  onDelete: (id: string) => void
}): React.JSX.Element {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const root = useRef<HTMLDivElement>(null)
  const menu = useRef<HTMLDivElement>(null)
  const current = [...builtin, ...mine].find((p) => p.id === active)?.name

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
  const choose = (id: string): void => {
    onPick(id)
    close()
  }
  const commitName = (): void => {
    if (editing && draft.trim()) onRename(editing, draft)
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
  const pick = (p: Item): React.JSX.Element => (
    <button type="button" className="pm-pick" role="menuitemradio" aria-checked={active === p.id} onClick={() => choose(p.id)}>
      <span className="name">{p.name}</span>
      {active === p.id && <Check size={14} />}
    </button>
  )

  return (
    <div
      ref={root}
      className={`dd pm ${open ? 'open' : ''}`}
      onKeyDown={open ? onKey : undefined}
      // Con Tab fuera del menú, se cierra
      onBlur={(e) => open && e.relatedTarget && !root.current?.contains(e.relatedTarget) && setOpen(false)}
    >
      <button type="button" className="dd-btn" aria-haspopup="menu" aria-expanded={open} aria-label={`${t('presets.menu')}: ${current ?? idle}`} onClick={() => (open ? close() : setOpen(true))}>
        <span className="pm-label">{t('presets.title')}</span>
        <b className="pm-name">{current ?? idle}</b>
        <ChevronDown size={15} />
      </button>
      {open && (
        <div ref={menu} className="dd-menu pm-menu" role="menu">
          {builtin.length > 0 && (
            <>
              <div className="dd-group" role="presentation">
                {t('presets.builtinTitle')}
              </div>
              {builtin.map((p) => (
                <div key={p.id} className={`pm-row ${active === p.id ? 'on' : ''}`}>
                  {pick(p)}
                </div>
              ))}
            </>
          )}
          <div className="dd-group" role="presentation">
            {t('presets.mine')}
          </div>
          {mine.length === 0 && <div className="pm-empty small muted">{empty}</div>}
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
                {pick(p)}
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
                  {onShare && (
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label={`${t('share.title')}: ${p.name}`}
                      onClick={() => {
                        setOpen(false)
                        onShare(p.id)
                      }}
                    >
                      <Share2 size={14} />
                    </button>
                  )}
                  <button type="button" className="icon-btn" aria-label={`${t('common.delete')}: ${p.name}`} onClick={() => onDelete(p.id)}>
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

/** Presets del ecualizador: los de serie y los propios, sobre la curva que se edita */
export function EqPresetMenu(): React.JSX.Element {
  const t = useT()
  const active = useStore((s) => currentCurve(s).preset)
  const mine = useStore((s) => s.config?.presets ?? NONE)
  const builtin = BUILTIN.map((b) => ({ id: b.id, name: t(`presets.builtin.${b.id}`) }))
  return (
    <PresetMenu
      builtin={builtin}
      mine={mine}
      active={active}
      idle={t('presets.custom')}
      empty={t('presets.mineEmpty')}
      onPick={(id) => {
        const p = mine.find((x) => x.id === id)
        applyPreset(p ? p.curve : builtinCurve(id), id)
      }}
      onRename={renamePreset}
      onShare={(id) => {
        const p = mine.find((x) => x.id === id)
        if (p) sharePreset('eq', p)
      }}
      onDelete={deletePreset}
    />
  )
}

/** Presets propios de Sonido, Cascos, Estudio o Micrófono */
function SectionPresetMenu({ section }: { section: Section }): React.JSX.Element {
  const t = useT()
  const mine = useStore((s) => sectionPresets(s, section))
  const active = useStore((s) => activeSectionPreset(s, section))
  const off = useStore((s) => sectionOff(s, section))
  const find = (id: string): SectionPreset | undefined => mine.find((p) => p.id === id)
  return (
    <PresetMenu
      mine={mine}
      active={active}
      idle={t(off ? 'presets.none' : 'presets.custom')}
      empty={t(`sections.${section}.empty`)}
      onPick={(id) => {
        const p = find(id)
        if (p) applySectionPreset(section, p)
      }}
      onRename={(id, name) => renameSectionPreset(section, id, name)}
      onShare={
        section === 'plugins'
          ? undefined
          : (id) => {
              const p = find(id)
              if (p) sharePreset(section, p)
            }
      }
      onDelete={(id) => deleteSectionPreset(section, id)}
    />
  )
}

/** «Guardar» e «Importar» de la cabecera de una sección */
export function PresetActions({ section }: { section: ShareSection | 'plugins' }): React.JSX.Element {
  const t = useT()
  return (
    <>
      <button className="btn btn-sm hb" data-tip={t('presets.save')} aria-label={t('presets.save')} onClick={() => setState({ modal: 'savePreset', saveFor: section })}>
        <Save size={15} />
        <span className="btn-label">{t('presets.saveShort')}</span>
      </button>
      {section !== 'plugins' && (
        <button className="btn btn-sm hb hb-early" data-tip={t('share.importTitle')} aria-label={t('share.importTitle')} onClick={() => setState({ modal: 'import', importCode: '' })}>
          <Import size={15} />
          <span className="btn-label">{t('presets.import')}</span>
        </button>
      )}
    </>
  )
}

/** Cabecera de presets de una sección, como la del ecualizador: desplegable, Guardar e Importar */
export function SectionPresets({ section, children }: { section: Section; children?: React.ReactNode }): React.JSX.Element {
  return (
    <div className="toolbar section-bar">
      <SectionPresetMenu section={section} />
      <PresetActions section={section} />
      {children}
    </div>
  )
}
