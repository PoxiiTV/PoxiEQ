import { useEffect, useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { AudioLines, Info, LoaderCircle, Palette, X } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { api, type Accent, type Config } from '../lib/api'
import { runSetup } from '../lib/actions'
import { refreshEngine, setState, toast, updateConfig, useStore } from '../lib/store'
import { applyTheme } from '../lib/theme'
import { checkForUpdates } from '../lib/update'
import { errorText, useT } from '../i18n'
import { Select } from './controls'
import { Logo, Modal, Switch } from './ui'

type Cat = 'general' | 'engine' | 'about'
const CATS: [Cat, LucideIcon][] = [
  ['general', Palette],
  ['engine', AudioLines],
  ['about', Info]
]
const ACCENTS: Record<Accent, string> = {
  cyan: '#06b6d4',
  blue: '#3b82f6',
  emerald: '#10b981',
  violet: '#8b5cf6',
  pink: '#ec4899',
  amber: '#f59e0b',
  red: '#ef4444'
}
const REPO = 'https://github.com/PoxiiTV/PoxiEQ'

function Row({ title, hint, children }: { title: string; hint?: string; children?: React.ReactNode }): React.JSX.Element {
  return (
    <div className="set-row" role="group" aria-label={title}>
      <div className="txt">
        <b>{title}</b>
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  )
}

export function Settings(): React.JSX.Element | null {
  const t = useT()
  const open = useStore((s) => s.settingsOpen)
  const [cat, setCat] = useState<Cat>('general')
  if (!open) return null
  return (
    <Modal onClose={() => setState({ settingsOpen: false })} className="settings">
      <nav>
        <h2>{t('settings.title')}</h2>
        {CATS.map(([c, Icon]) => (
          <button key={c} className={`nav-item ${cat === c ? 'active' : ''}`} onClick={() => setCat(c)}>
            <Icon size={16} />
            {t(`settings.cats.${c}`)}
          </button>
        ))}
      </nav>
      <div className="body">
        <button className="icon-btn settings-close" onClick={() => setState({ settingsOpen: false })} aria-label={t('common.close')}>
          <X size={17} />
        </button>
        {cat === 'general' && <General />}
        {cat === 'engine' && <Engine />}
        {cat === 'about' && <About />}
      </div>
    </Modal>
  )
}

function General(): React.JSX.Element {
  const t = useT()
  const cfg = useStore((s) => s.config!)
  const [autostart, setAutostart] = useState<boolean | null>(null)
  useEffect(() => {
    void api.autostartGet().then(setAutostart)
  }, [])
  const set = (patch: Partial<Config>): void => {
    updateConfig(patch)
    if (patch.theme || patch.accent) applyTheme({ ...cfg, ...patch })
  }
  return (
    <>
      <h3>{t('settings.appearance')}</h3>
      <Row title={t('settings.theme')}>
        <div className="segmented">
          {(['dark', 'light', 'system'] as const).map((th) => (
            <button key={th} className={cfg.theme === th ? 'on' : ''} onClick={() => set({ theme: th })}>
              {t(`settings.themes.${th}`)}
            </button>
          ))}
        </div>
      </Row>
      <Row title={t('settings.accent')}>
        <div className="accents">
          {(Object.keys(ACCENTS) as Accent[]).map((a) => (
            <button key={a} className={cfg.accent === a ? 'on' : ''} style={{ background: ACCENTS[a] }} aria-label={a} onClick={() => set({ accent: a })} />
          ))}
        </div>
      </Row>
      <Row title={t('settings.lang')}>
        <Select
          value={cfg.lang ?? 'system'}
          options={[
            { value: 'system', label: t('settings.langSystem') },
            { value: 'es', label: 'Español' },
            { value: 'en', label: 'English' }
          ]}
          onChange={(v) => set({ lang: v === 'system' ? null : (v as 'es' | 'en') })}
        />
      </Row>
      <h3 style={{ marginTop: 22 }}>{t('settings.behaviour')}</h3>
      <Row title={t('settings.autostart')} hint={t('settings.autostartHint')}>
        {autostart !== null && (
          <Switch
            on={autostart}
            label={t('settings.autostart')}
            onChange={(on) =>
              void api
                .autostartSet(on)
                .then(() => setAutostart(on))
                .catch(() => toast(t('err.io.failed'), 'err'))
            }
          />
        )}
      </Row>
      <Row title={t('settings.closeToTray')} hint={t('settings.closeToTrayHint')}>
        <Switch on={cfg.closeToTray} label={t('settings.closeToTray')} onChange={(closeToTray) => set({ closeToTray })} />
      </Row>
      <Row title={t('settings.hotkey')} hint={t('settings.hotkeyHint')}>
        <Switch on={cfg.bypassHotkey} label={t('settings.hotkey')} onChange={(bypassHotkey) => set({ bypassHotkey })} />
      </Row>
      <Row title={t('settings.analyzer')} hint={t('settings.analyzerHint')}>
        <Switch on={cfg.analyzer} label={t('settings.analyzer')} onChange={(analyzer) => set({ analyzer })} />
      </Row>
    </>
  )
}

function Engine(): React.JSX.Element {
  const t = useT()
  const engine = useStore((s) => s.engine)
  const step = useStore((s) => s.setup)
  const devices = engine?.devices ?? []
  const [want, setWant] = useState<Record<string, boolean>>({})
  useEffect(() => setWant(Object.fromEntries(devices.map((d) => [d.id, d.enabled]))), [engine]) // eslint-disable-line react-hooks/exhaustive-deps
  const enable = devices.filter((d) => want[d.id] && !d.enabled).map((d) => d.id)
  const disable = devices.filter((d) => !want[d.id] && d.enabled).map((d) => d.id)
  return (
    <>
      <h3>{t('settings.cats.engine')}</h3>
      <Row title={t('settings.engineStatus')} hint={engine?.installed ? t('settings.engineOn') : t('settings.engineOff')}>
        <span className={`chip ${engine?.installed ? 'ok' : ''}`}>{engine?.installed ? t('device.on') : t('device.off')}</span>
      </Row>
      <h3 style={{ marginTop: 22 }}>{t('settings.devices')}</h3>
      <div className="hint" style={{ marginBottom: 6 }}>
        {t('settings.devicesHint')}
      </div>
      {devices.map((d) => (
        <Row key={d.id} title={d.name} hint={`${d.device}${d.default ? ` · ${t('device.default')}` : ''}`}>
          <Switch on={!!want[d.id]} label={d.name} onChange={(on) => setWant({ ...want, [d.id]: on })} />
        </Row>
      ))}
      <div className="modal-actions">
        <button className="btn btn-primary" disabled={!!step || (!enable.length && !disable.length)} onClick={() => void runSetup(enable, disable)}>
          {step && <LoaderCircle size={16} className="spin" />}
          {t('settings.apply')}
        </button>
      </div>
      {engine?.installed && <Uninstall />}
    </>
  )
}

/** Dos pasos (sin diálogo nativo): el primer clic arma el botón, el segundo desinstala */
function Uninstall(): React.JSX.Element {
  const t = useT()
  const [armed, setArmed] = useState(false)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (!armed) return
    const id = setTimeout(() => setArmed(false), 4000)
    return () => clearTimeout(id)
  }, [armed])
  const run = async (): Promise<void> => {
    if (!armed) return setArmed(true)
    setBusy(true)
    try {
      await api.engineUninstall()
      toast(t('settings.uninstalled'))
    } catch (e) {
      toast(errorText(e), 'err', { secs: 8 })
    } finally {
      setBusy(false)
      setArmed(false)
      await refreshEngine()
    }
  }
  return (
    <Row title={t('settings.uninstall')} hint={t('settings.uninstallHint')}>
      <button className="btn btn-sm btn-danger" disabled={busy} onClick={() => void run()}>
        {busy && <LoaderCircle size={14} className="spin" />}
        {armed ? t('settings.uninstallSure') : t('settings.uninstall')}
      </button>
    </Row>
  )
}

function About(): React.JSX.Element {
  const t = useT()
  const app = useStore((s) => s.app)
  const [checking, setChecking] = useState(false)
  return (
    <>
      <div className="about">
        <Logo size={64} />
        <div>
          <h3 style={{ margin: 0 }}>
            Poxi<b className="grad">EQ</b>
          </h3>
          <div className="muted small">{t('settings.version', { v: app?.version ?? '' })}</div>
        </div>
      </div>
      <div className="row" style={{ flexWrap: 'wrap', margin: '18px 0' }}>
        <button
          className="btn btn-sm"
          disabled={checking}
          onClick={async () => {
            setChecking(true)
            const found = await checkForUpdates(true)
            setChecking(false)
            if (!found) toast(t('settings.upToDate'), 'info')
          }}
        >
          {checking && <LoaderCircle size={14} className="spin" />}
          {t('settings.updates')}
        </button>
        <button className="btn btn-sm" onClick={() => window.dispatchEvent(new Event('poxi:whatsnew'))}>
          {t('settings.whatsNew')}
        </button>
        <button className="btn btn-sm btn-ghost" onClick={() => void openUrl(REPO)}>
          {t('settings.github')}
        </button>
      </div>
      <p className="hint">{t('settings.credits')}</p>
    </>
  )
}
