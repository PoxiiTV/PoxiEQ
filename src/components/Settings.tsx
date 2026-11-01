import { useEffect, useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { AudioLines, Info, Keyboard, LoaderCircle, Palette } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { api, type Accent, type Config, type Device } from '../lib/api'
import { runSetup } from '../lib/actions'
import { allPresets, listed, refreshEngine, setState, toast, updateConfig, useStore } from '../lib/store'
import { applyTheme } from '../lib/theme'
import { checkForUpdates } from '../lib/update'
import { errorText, t, useT } from '../i18n'
import { KeyRecorder, Range, Select, type Option } from './controls'
import { Logo, Switch } from './ui'

type Cat = 'general' | 'keys' | 'engine' | 'about'
const CATS: [Cat, LucideIcon][] = [
  ['general', Palette],
  ['keys', Keyboard],
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

/** Página «Ajustes»: sus apartados en una barra de pestañas arriba */
export function SettingsPage(): React.JSX.Element {
  const t = useT()
  const [cat, setCat] = useState<Cat>('general')
  return (
    <div className="page settings-page">
      <div className="segmented settings-tabs" role="tablist" aria-label={t('settings.title')}>
        {CATS.map(([c, Icon]) => (
          <button key={c} role="tab" aria-selected={cat === c} className={cat === c ? 'on' : ''} onClick={() => setCat(c)}>
            <Icon size={15} />
            <span>{t(`settings.cats.${c}`)}</span>
          </button>
        ))}
      </div>
      <div className="settings-body" role="tabpanel">
        {cat === 'general' && <General />}
        {cat === 'keys' && <Keys />}
        {cat === 'engine' && <Engine />}
        {cat === 'about' && <About />}
      </div>
    </div>
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
      <Row title={t('settings.follow')} hint={t('settings.followHint')}>
        <Switch on={cfg.followDefault} label={t('settings.follow')} onChange={(followDefault) => set({ followDefault })} />
      </Row>
      <h3 style={{ marginTop: 22 }}>{t('settings.sound')}</h3>
      <Row title={t('settings.loudness')} hint={t('settings.loudnessHint')}>
        <Switch on={cfg.loudness.on} label={t('settings.loudness')} onChange={(on) => set({ loudness: { ...cfg.loudness, on } })} />
      </Row>
      {cfg.loudness.on && (
        <>
          <Row title={t('settings.loudnessStrength')}>
            <div className="set-range">
              <Range value={Math.round(cfg.loudness.strength * 100)} min={0} max={100} label={t('settings.loudnessStrength')} onChange={(v) => set({ loudness: { ...cfg.loudness, strength: v / 100 } })} />
              <b className="num">{Math.round(cfg.loudness.strength * 100)} %</b>
            </div>
          </Row>
          <Row title={t('settings.loudnessRef')} hint={t('settings.loudnessRefHint')}>
            <div className="set-range">
              <Range value={cfg.loudness.offset} min={-30} max={30} label={t('settings.loudnessRef')} onChange={(offset) => set({ loudness: { ...cfg.loudness, offset } })} />
              <b className="num">{cfg.loudness.offset > 0 ? '+' : ''}{cfg.loudness.offset} dB</b>
            </div>
          </Row>
        </>
      )}
      <Row title={t('settings.analyzer')} hint={t('settings.analyzerHint')}>
        <Switch on={cfg.analyzer} label={t('settings.analyzer')} onChange={(analyzer) => set({ analyzer })} />
      </Row>
      {cfg.analyzer && (
        <Row title={t('settings.analyzerMode')} hint={t(`settings.analyzerModes.${cfg.analyzerMode}Hint`)}>
          <div className="segmented">
            {(['spectrum', 'spectrogram'] as const).map((m) => (
              <button key={m} className={cfg.analyzerMode === m ? 'on' : ''} onClick={() => set({ analyzerMode: m })}>
                {t(`settings.analyzerModes.${m}`)}
              </button>
            ))}
          </div>
        </Row>
      )}
    </>
  )
}

/** Presets para un desplegable; si el elegido ya no existe (se borró), se enseña como tal en vez de en blanco */
export function presetOptions(cfg: Config, value: string | null, none?: string): Option<string>[] {
  const list: Option<string>[] = allPresets(cfg).map((p) => ({ value: p.id, label: p.name }))
  if (value && !list.some((o) => o.value === value)) list.push({ value, label: t('auto.missing') })
  return none === undefined ? list : [{ value: '', label: none }, ...list]
}

/** Atajos globales: bypass y nueve huecos de preset, cada uno con la combinación que se grabe */
function Keys(): React.JSX.Element {
  const t = useT()
  const cfg = useStore((s) => s.config!)
  const failed = useStore((s) => s.hotkeysFailed)
  const h = cfg.hotkeys
  const all = [h.bypass, ...h.presets.map((p) => p.keys), h.mixerGame, h.mixerChat]
  /** Las combinaciones de los demás atajos (`own` = posición en `all`), para no repetir ninguna */
  const others = (own: number): string[] => all.filter((k, i): k is string => !!k && i !== own)
  const setSlot = (i: number, patch: Partial<(typeof h.presets)[number]>): void =>
    updateConfig({ hotkeys: { ...h, presets: h.presets.map((p, j) => (j === i ? { ...p, ...patch } : p)) } })
  return (
    <>
      <h3>{t('keys.title')}</h3>
      <p className="hint set-intro">{t('keys.hint')}</p>
      <Row title={t('keys.bypass')} hint={t('keys.bypassHint')}>
        <KeyRecorder
          value={h.bypass}
          taken={others(0)}
          failed={!!h.bypass && failed.includes(h.bypass)}
          label={t('keys.bypass')}
          onChange={(bypass) => updateConfig({ hotkeys: { ...h, bypass } })}
        />
      </Row>
      <h3 style={{ marginTop: 22 }}>{t('keys.presets')}</h3>
      {h.presets.map((p, i) => (
        <Row key={i} title={t('keys.slot', { n: i + 1 })}>
          <Select size="sm" value={p.preset ?? ''} options={presetOptions(cfg, p.preset, t('keys.none'))} label={t('keys.slot', { n: i + 1 })} onChange={(v) => setSlot(i, { preset: v || null })} />
          <KeyRecorder value={p.keys} taken={others(i + 1)} failed={!!p.keys && !!p.preset && failed.includes(p.keys)} label={t('keys.slot', { n: i + 1 })} onChange={(keys) => setSlot(i, { keys })} />
        </Row>
      ))}
      <h3 style={{ marginTop: 22 }}>{t('apps.mixer')}</h3>
      {(['mixerGame', 'mixerChat'] as const).map((k, j) => (
        <Row key={k} title={t(`keys.${k}`)}>
          <KeyRecorder
            value={h[k]}
            taken={others(h.presets.length + 1 + j)}
            failed={!!h[k] && failed.includes(h[k]!)}
            label={t(`keys.${k}`)}
            onChange={(v) => updateConfig({ hotkeys: { ...h, [k]: v } })}
          />
        </Row>
      ))}
    </>
  )
}

function Engine(): React.JSX.Element {
  const t = useT()
  const engine = useStore((s) => s.engine)
  const step = useStore((s) => s.setup)
  const devices = engine?.devices ?? []
  const outputs = devices.filter((d) => !d.input && listed(d))
  const inputs = devices.filter((d) => d.input)
  const [want, setWant] = useState<Record<string, boolean>>({})
  useEffect(() => setWant(Object.fromEntries(devices.map((d) => [d.id, d.enabled]))), [engine]) // eslint-disable-line react-hooks/exhaustive-deps
  // Uno roto que sigue marcado se vuelve a activar (es lo que lo repara)
  const enable = devices.filter((d) => want[d.id] && (!d.enabled || d.broken)).map((d) => d.id)
  const disable = devices.filter((d) => !want[d.id] && d.enabled).map((d) => d.id)
  const hint = (d: Device): string =>
    [d.device, d.default && t('device.default'), d.pair && t('device.calls'), d.broken && t('device.broken')].filter(Boolean).join(' · ')
  return (
    <>
      <h3>{t('settings.cats.engine')}</h3>
      <Row title={t('settings.engineStatus')} hint={engine?.installed ? t('settings.engineOn') : t('settings.engineOff')}>
        <span className={`chip ${engine?.installed ? 'ok' : ''}`}>{engine?.installed ? t('device.on') : t('device.off')}</span>
      </Row>
      <AutoEnableRow />
      <Row title={t('diag.title')} hint={t('diag.rowHint')}>
        <button className="btn btn-sm" onClick={() => setState({ modal: 'diagnose' })}>
          {t('diag.btn')}
        </button>
      </Row>
      <h3 style={{ marginTop: 22 }}>{t('settings.devices')}</h3>
      <div className="hint" style={{ marginBottom: 6 }}>
        {t('settings.devicesHint')}
      </div>
      {outputs.map((d) => (
        <Row key={d.id} title={d.name} hint={hint(d)}>
          <Switch on={!!want[d.id]} label={d.name} onChange={(on) => setWant({ ...want, [d.id]: on })} />
        </Row>
      ))}
      {inputs.length > 0 && (
        <>
          <h3 style={{ marginTop: 22 }}>{t('settings.inputs')}</h3>
          <div className="hint" style={{ marginBottom: 6 }}>
            {t('settings.inputsHint')}
          </div>
          {inputs.map((d) => (
            <Row key={d.id} title={d.name} hint={hint(d)}>
              <Switch on={!!want[d.id]} label={d.name} onChange={(on) => setWant({ ...want, [d.id]: on })} />
            </Row>
          ))}
        </>
      )}
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

/** Activar solos los dispositivos nuevos: crea o quita la tarea programada (un permiso de administrador) */
function AutoEnableRow(): React.JSX.Element {
  const t = useT()
  const on = useStore((s) => !!s.config?.autoEnable)
  const installed = useStore((s) => !!s.engine?.installed)
  const [busy, setBusy] = useState(false)
  const set = async (v: boolean): Promise<void> => {
    setBusy(true)
    try {
      await api.autoEnableSet(v)
      toast(t(v ? 'settings.autoEnableOn' : 'settings.autoEnableOff'))
    } catch (e) {
      toast(errorText(e), 'err', { secs: 8 })
    } finally {
      setBusy(false)
    }
  }
  return (
    <Row title={t('settings.autoEnable')} hint={t('settings.autoEnableHint')}>
      {busy ? <LoaderCircle size={16} className="spin" /> : <Switch on={on} label={t('settings.autoEnable')} onChange={(v) => installed && void set(v)} />}
    </Row>
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
