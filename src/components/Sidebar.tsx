import { Headphones, Import, Save, Search, Share2, SlidersHorizontal, Trash2 } from 'lucide-react'
import { BUILTIN, builtinCurve } from '../lib/eq'
import { applyPreset, currentCurve, deletePreset, getState, setState, toast, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'

const NONE: never[] = []

/** Nombre cortado con «…»: el completo va en el tooltip (solo entonces, para no repetir lo que ya se lee) */
function tipIfCut(item: HTMLElement): void {
  // Un fotograma después: con el ratón encima aparecen los botones de la fila y el nombre se acorta
  requestAnimationFrame(() => {
    const name = item.querySelector<HTMLElement>('.name')
    if (name && name.scrollWidth > name.clientWidth) item.dataset.tip = name.textContent ?? ''
    else delete item.dataset.tip
  })
}

export function Sidebar(): React.JSX.Element {
  const t = useT()
  const active = useStore((s) => currentCurve(s).preset)
  const presets = useStore((s) => s.config?.presets ?? NONE)

  const remove = (id: string): void => {
    const before = getState().config?.presets ?? []
    deletePreset(id)
    toast(t('presets.deleted'), 'info', { secs: 6, action: { label: t('presets.undo'), run: () => updateConfig({ presets: before }) } })
  }

  return (
    <aside className="sidebar">
      <div className="nav-title">{t('presets.title')}</div>
      {BUILTIN.map((p) => (
        <button
          key={p.id}
          className={`nav-item ${active === p.id ? 'active' : ''}`}
          onPointerEnter={(e) => tipIfCut(e.currentTarget)}
          onClick={() => applyPreset(builtinCurve(p.id), p.id)}
        >
          <SlidersHorizontal size={16} />
          <span className="name">{t(`presets.builtin.${p.id}`)}</span>
        </button>
      ))}
      <div className="nav-title with-action">
        {t('presets.mine')}
        <button className="icon-btn tip-right" data-tip={t('share.importTitle')} aria-label={t('share.importTitle')} onClick={() => setState({ modal: 'import', importCode: '' })}>
          <Import size={14} />
        </button>
      </div>
      {presets.length === 0 && <div className="nav-empty small muted">{t('presets.mineEmpty')}</div>}
      {presets.map((p) => (
        <div
          key={p.id}
          className={`nav-item has-action ${active === p.id ? 'active' : ''}`}
          role="button"
          tabIndex={0}
          onPointerEnter={(e) => tipIfCut(e.currentTarget)}
          onClick={() => applyPreset(p.curve, p.id)}
          onKeyDown={(e) => e.key === 'Enter' && e.target === e.currentTarget && applyPreset(p.curve, p.id)}
        >
          <Headphones size={16} />
          <span className="name">{p.name}</span>
          <span className="row-actions">
            <button
              className="icon-btn row-action"
              aria-label={t('share.title')}
              onClick={(e) => {
                e.stopPropagation()
                setState({ sharing: p })
              }}
            >
              <Share2 size={14} />
            </button>
            <button
              className="icon-btn row-action"
              aria-label={t('common.delete')}
              onClick={(e) => {
                e.stopPropagation()
                remove(p.id)
              }}
            >
              <Trash2 size={14} />
            </button>
          </span>
        </div>
      ))}
      <div className="spacer" />
      <div className="sidebar-actions">
        <button className="btn btn-sm btn-block" onClick={() => setState({ modal: 'savePreset' })}>
          <Save size={15} /> {t('presets.save')}
        </button>
        <button className="btn btn-sm btn-block" onClick={() => setState({ modal: 'autoeq' })}>
          <Search size={15} /> {t('presets.autoeq')}
        </button>
      </div>
    </aside>
  )
}
