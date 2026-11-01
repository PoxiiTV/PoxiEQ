import { useEffect, useMemo, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { ArrowDown, ArrowUp, FolderPlus, LoaderCircle, Plus, RefreshCw, Search, SlidersHorizontal, Trash2, TriangleAlert, X } from 'lucide-react'
import { api, type FoundPlugin, type PluginSlot, type ScanResult } from '../lib/api'
import { go, pluginsDevice, pluginsSettings, setPlugins, setState, toast, updateConfig, useStore, works } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Install, SectionHead } from './Dynamics'
import { SectionPresets } from './Presets'
import { Setup } from './Setup'
import { Switch } from './ui'

/** Biblioteca escaneada (vive mientras la app está abierta; el escaneo de verdad tiene caché en Rust) */
let library: ScanResult | null = null

/** Carpetas que el audio de Windows (otra cuenta) puede leer seguro */
const readable = (path: string): boolean => /^[a-z]:\\program files( \(x86\))?\\/i.test(path)

/** Página «Plugins»: tus VST2 y VST3 en la cadena de la salida (o del micrófono), cada uno con su ventana */
export function PluginsPage(): React.JSX.Element {
  const t = useT()
  const target = useStore((s) => s.pluginsFor)
  const device = useStore((s) => pluginsDevice(s))
  const ready = useStore((s) => !!s.engine?.installed && works(pluginsDevice(s)))
  const hasMic = useStore((s) => s.engine?.devices.some((d) => d.input && !d.unplugged))

  return (
    <div className="page plugins-page">
      <SectionPresets section="plugins">
        {hasMic && (
          <div className="segmented" role="radiogroup" aria-label={t('plugins.for')}>
            {(['output', 'mic'] as const).map((k) => (
              <button key={k} role="radio" aria-checked={target === k} className={target === k ? 'on' : ''} onClick={() => setState({ pluginsFor: k })}>
                {t(`plugins.${k}`)}
              </button>
            ))}
          </div>
        )}
      </SectionPresets>
      {!device || !ready ? (
        target === 'output' ? (
          <Setup />
        ) : (
          <div className="banner">
            <TriangleAlert size={16} />
            <span>{t('plugins.micOff')}</span>
            <div className="spacer" />
            <button className="btn btn-sm btn-primary" onClick={() => go('mic')}>
              {t('nav.mic')}
            </button>
          </div>
        )
      ) : (
        <>
          <Install />
          <Chain />
          <Library />
        </>
      )}
    </div>
  )
}

function Chain(): React.JSX.Element {
  const t = useT()
  const device = useStore((s) => pluginsDevice(s))
  const list = useStore((s) => pluginsSettings(s).plugins)
  const editing = useStore((s) => s.editing)
  const ms = list.reduce((a, p) => a + (p.on ? p.latency / 48 : 0), 0)
  const put = (i: number, patch: Partial<PluginSlot>): void => setPlugins(list.map((p, j) => (i === j ? { ...p, ...patch } : p)))
  const move = (i: number, d: number): void => {
    const next = [...list]
    ;[next[i], next[i + d]] = [next[i + d], next[i]]
    setPlugins(next)
  }
  const edit = (p: PluginSlot): void => {
    if (device) api.pluginEdit(device.id, p.id).catch((e) => toast(errorText(e), 'err'))
  }
  return (
    <section>
      <SectionHead n={1} label={t('plugins.chain')}>
        {ms >= 0.05 && <span className="chip">{t('plugins.latency', { ms: ms.toFixed(1) })}</span>}
      </SectionHead>
      <small className="studio-hint">{t('plugins.chainHint')}</small>
      {list.length === 0 && <div className="plugins-empty">{t('plugins.empty')}</div>}
      <ol className="plugin-chain">
        {list.map((p, i) => {
          const open = !!device && editing.includes(`${device.id}/${p.id}`)
          return (
            <li key={p.id} className={`plugin-slot ${p.on ? 'on' : ''}`}>
              <span className="num">{i + 1}</span>
              <div className="txt">
                <b>{p.name}</b>
                <small>
                  {[p.vendor, p.format.toUpperCase(), p.latency > 0 && t('plugins.latency', { ms: (p.latency / 48).toFixed(1) })].filter(Boolean).join(' · ')}
                </small>
              </div>
              {!readable(p.path) && (
                <span className="warn-ico" data-tip={t('plugins.unreadable')}>
                  <TriangleAlert size={15} />
                </span>
              )}
              <button className="icon-btn" disabled={i === 0} onClick={() => move(i, -1)} aria-label={t('plugins.up')} data-tip={t('plugins.up')}>
                <ArrowUp size={15} />
              </button>
              <button className="icon-btn" disabled={i === list.length - 1} onClick={() => move(i, 1)} aria-label={t('plugins.down')} data-tip={t('plugins.down')}>
                <ArrowDown size={15} />
              </button>
              <button className={`btn btn-sm ${open ? 'btn-primary' : ''}`} onClick={() => edit(p)}>
                <SlidersHorizontal size={14} />
                {t(open ? 'plugins.editing' : 'plugins.edit')}
              </button>
              <Switch on={p.on} label={p.name} onChange={(on) => put(i, { on })} />
              <button className="icon-btn" onClick={() => setPlugins(list.filter((_, j) => j !== i))} aria-label={`${t('common.delete')}: ${p.name}`} data-tip={t('plugins.remove')}>
                <Trash2 size={15} />
              </button>
            </li>
          )
        })}
      </ol>
      <small className="studio-hint">{t('plugins.risks')}</small>
    </section>
  )
}

const MAX = 16

function Library(): React.JSX.Element {
  const t = useT()
  const [lib, setLib] = useState<ScanResult | null>(library)
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<[number, number] | null>(null)
  const [q, setQ] = useState('')
  const [folder, setFolder] = useState('')
  const list = useStore((s) => pluginsSettings(s).plugins)
  const folders = useStore((s) => s.config?.pluginFolders ?? [])

  const scan = async (force: boolean): Promise<void> => {
    setBusy(true)
    const off = await listen<[number, number]>('plugins-scan', (e) => setProgress(e.payload))
    try {
      library = await api.pluginsScan(force)
      setLib(library)
    } catch (e) {
      toast(errorText(e), 'err')
    } finally {
      off()
      setBusy(false)
      setProgress(null)
    }
  }
  useEffect(() => {
    if (!library) void scan(false)
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const shown = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean)
    return (lib?.plugins ?? []).filter((p) => words.every((w) => `${p.name} ${p.vendor}`.toLowerCase().includes(w)))
  }, [lib, q])

  const add = (f: FoundPlugin): void => {
    if (list.length >= MAX) return void toast(t('plugins.full', { n: MAX }), 'err')
    const id = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`
    setPlugins([...list, { ...f, id, on: true, state: '' }])
    toast(t('plugins.added', { n: f.name }))
  }
  const addFolder = (): void => {
    const f = folder.trim().replace(/[\\/]+$/, '')
    if (!/^[a-z]:\\/i.test(f)) return void toast(t('plugins.folderBad'), 'err')
    if (!folders.includes(f)) updateConfig({ pluginFolders: [...folders, f] })
    setFolder('')
  }

  return (
    <section>
      <SectionHead n={2} label={t('plugins.library')}>
        <button className="btn btn-sm" disabled={busy} onClick={() => void scan(true)}>
          {busy ? <LoaderCircle size={14} className="spin" /> : <RefreshCw size={14} />}
          {busy && progress ? t('plugins.scanning', { n: progress[0], t: progress[1] }) : t('plugins.scan')}
        </button>
      </SectionHead>
      <div className="finder-search">
        <Search size={17} />
        <input className="input" value={q} placeholder={t('plugins.search')} onChange={(e) => setQ(e.target.value)} aria-label={t('plugins.search')} />
      </div>
      <div className="plugin-lib">
        {!lib && busy && <div className="plugins-empty">{t('plugins.scanFirst')}</div>}
        {lib && shown.length === 0 && <div className="plugins-empty">{t(lib.plugins.length ? 'plugins.noMatch' : 'plugins.none')}</div>}
        {shown.map((p) => (
          <button key={`${p.path}|${p.cid}`} className="plugin-found" onClick={() => add(p)} data-tip={p.path}>
            <Plus size={14} />
            <b>{p.name}</b>
            <small>
              {p.vendor} · {p.format.toUpperCase()}
            </small>
          </button>
        ))}
      </div>
      {!!lib?.failed.length && (
        <details className="plugin-failed">
          <summary>{t('plugins.failed', { n: lib.failed.length })}</summary>
          <ul>
            {lib.failed.map((f) => (
              <li key={f.path}>
                <code>{f.path}</code> — {f.error === 'timeout' ? t('plugins.timeout') : f.error === 'crash' ? t('plugins.crash') : f.error}
              </li>
            ))}
          </ul>
        </details>
      )}
      <SectionHead label={t('plugins.folders')} />
      <small className="studio-hint">{t('plugins.foldersHint')}</small>
      <ul className="plugin-folders">
        {folders.map((f) => (
          <li key={f}>
            <code>{f}</code>
            <button className="icon-btn" onClick={() => updateConfig({ pluginFolders: folders.filter((x) => x !== f) })} aria-label={`${t('common.delete')}: ${f}`}>
              <X size={14} />
            </button>
          </li>
        ))}
      </ul>
      <div className="plugin-folder-add">
        <input className="input" value={folder} placeholder="D:\Plugins\VST3" onChange={(e) => setFolder(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && addFolder()} aria-label={t('plugins.folderAdd')} />
        <button className="btn btn-sm" onClick={addFolder} disabled={!folder.trim()}>
          <FolderPlus size={14} />
          {t('plugins.folderAdd')}
        </button>
      </div>
    </section>
  )
}
