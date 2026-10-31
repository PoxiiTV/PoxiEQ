import { Headphones, Save, Search, SlidersHorizontal, Trash2 } from 'lucide-react'
import { BUILTIN, builtinCurve } from '../lib/eq'
import { applyPreset, currentCurve, deletePreset, getState, setState, toast, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'

const NONE: never[] = []

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
        <button key={p.id} className={`nav-item ${active === p.id ? 'active' : ''}`} onClick={() => applyPreset(builtinCurve(p.id), p.id)}>
          <SlidersHorizontal size={16} />
          <span className="name">{t(`presets.builtin.${p.id}`)}</span>
        </button>
      ))}
      <div className="nav-title">{t('presets.mine')}</div>
      {presets.length === 0 && <div className="nav-empty small muted">{t('presets.mineEmpty')}</div>}
      {presets.map((p) => (
        <div key={p.id} className={`nav-item has-action ${active === p.id ? 'active' : ''}`} role="button" tabIndex={0} onClick={() => applyPreset(p.curve, p.id)} onKeyDown={(e) => e.key === 'Enter' && applyPreset(p.curve, p.id)}>
          <Headphones size={16} />
          <span className="name">{p.name}</span>
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
