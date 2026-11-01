import { useMemo, useState } from 'react'
import { AppWindow, LoaderCircle, Plus, RotateCcw, Trash2, X } from 'lucide-react'
import { api, type AppRule, type OpenApp, type Voice } from '../lib/api'
import { currentCurve, listed, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Range, Select, type Option } from './controls'
import { SectionHead, Slider } from './Dynamics'
import { presetOptions } from './Settings'
import { Switch } from './ui'

/** Apps de voz de serie (las mismas que `VOICE_APPS` en Rust) */
const VOICE_APPS = ['discord.exe', 'teamspeak.exe', 'ts3client_win64.exe', 'ms-teams.exe', 'teams.exe', 'zoom.exe', 'skype.exe', 'mumble.exe', 'whatsapp.exe', 'whatsapp.root.exe']

/** Página «Apps»: preset y salida por aplicación, la voz por encima del juego y el mezclador juego ↔ chat */
export function AppsPage(): React.JSX.Element {
  const t = useT()
  return (
    <div className="page apps-page">
      <section>
        <SectionHead n={1} label={t('apps.rules')} />
        <small className="studio-hint">{t('apps.rulesHint')}</small>
        <Rules />
      </section>
      <section>
        <SectionHead n={2} label={t('apps.duck')} />
        <small className="studio-hint">{t('apps.duckHint')}</small>
        <Duck />
      </section>
      <section>
        <SectionHead n={3} label={t('apps.mixer')} />
        <small className="studio-hint">{t('apps.mixerHint')}</small>
        <Mixer />
      </section>
    </div>
  )
}

/** Elegir una app abierta (con ventana) */
function AppPicker({ taken, onPick }: { taken: string[]; onPick: (a: OpenApp) => void }): React.JSX.Element {
  const t = useT()
  const [apps, setApps] = useState<OpenApp[] | null>(null)
  const [loading, setLoading] = useState(false)
  const open = async (): Promise<void> => {
    setLoading(true)
    try {
      setApps(await api.appsList())
    } finally {
      setLoading(false)
    }
  }
  const free = apps?.filter((a) => !taken.includes(a.exe)) ?? []
  if (apps === null)
    return (
      <button className="btn btn-sm set-add" disabled={loading} onClick={() => void open()}>
        {loading ? <LoaderCircle size={15} className="spin" /> : <Plus size={15} />}
        {t('auto.add')}
      </button>
    )
  return (
    <div className="app-pick" role="group" aria-label={t('auto.pick')}>
      <div className="app-pick-head">
        <b>{t('auto.pick')}</b>
        <button className="icon-btn" aria-label={t('common.close')} onClick={() => setApps(null)}>
          <X size={15} />
        </button>
      </div>
      {free.length === 0 && <div className="small muted app-pick-empty">{t('auto.none')}</div>}
      {free.map((a) => (
        <button
          key={a.exe}
          className="app-pick-item"
          onClick={() => {
            onPick(a)
            setApps(null)
          }}
        >
          <AppWindow size={16} />
          <b>{a.name}</b>
          <small>{a.exe}</small>
        </button>
      ))}
    </div>
  )
}

function Rules(): React.JSX.Element {
  const t = useT()
  const cfg = useStore((s) => s.config!)
  // El selector devuelve la lista tal cual: si filtrara dentro, cada lectura sería otra y React entraría en bucle
  const devices = useStore((s) => s.engine?.devices)
  const outputs = useMemo(() => (devices ?? []).filter((d) => !d.input && listed(d)), [devices])
  const rules = cfg.appRules
  const setRules = (appRules: AppRule[]): void => updateConfig({ appRules })
  const edit = (i: number, patch: Partial<AppRule>): void => setRules(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)))
  const outOptions: Option<string>[] = [{ value: '', label: t('apps.outDefault') }, ...outputs.map((d) => ({ value: d.id, label: `${d.name} · ${d.device}` }))]
  return (
    <>
      {rules.length === 0 && <div className="set-empty">{t('auto.empty')}</div>}
      {rules.map((r, i) => (
        <div key={r.exe} className="set-row app-rule" role="group" aria-label={r.name}>
          <div className="txt">
            <b>{r.name}</b>
            <small>{r.exe}</small>
          </div>
          <Select size="sm" value={r.preset} options={presetOptions(cfg, r.preset, t('apps.noPreset'))} label={t('auto.preset')} onChange={(preset) => edit(i, { preset })} />
          <Select size="sm" value={r.output ?? ''} options={outOptions} label={t('apps.output')} onChange={(v) => edit(i, { output: v || null })} />
          <Switch on={r.on} label={t('auto.on')} onChange={(on) => edit(i, { on })} />
          <button className="icon-btn" aria-label={t('common.delete')} onClick={() => setRules(rules.filter((_, j) => j !== i))}>
            <Trash2 size={15} />
          </button>
        </div>
      ))}
      <AppPicker taken={rules.map((r) => r.exe)} onPick={(a) => setRules([...rules, { exe: a.exe, name: a.name, preset: currentCurve().preset ?? 'b:flat', on: true, output: null }])} />
      <small className="studio-hint">{t('apps.outputHint')}</small>
    </>
  )
}

function useVoice(): [Voice, (patch: Partial<Voice>) => void] {
  const voice = useStore((s) => s.config!.voice)
  return [voice, (patch) => updateConfig({ voice: { ...voice, ...patch } })]
}

function Duck(): React.JSX.Element {
  const t = useT()
  const [v, set] = useVoice()
  return (
    <>
      <div className="set-row" role="group" aria-label={t('apps.duck')}>
        <div className="txt">
          <b>{t('apps.duckOn')}</b>
          <small>{t('apps.duckAmount', { n: v.duckDb })}</small>
        </div>
        <Switch on={v.duck} label={t('apps.duckOn')} onChange={(duck) => set({ duck })} />
      </div>
      <div className="duck-amount">
        <Slider label={t('apps.duckDb')} value={v.duckDb} min={3} max={30} fmt={(n) => `−${n} dB`} onChange={(duckDb) => set({ duckDb })} />
      </div>
      <div className="voice-apps">
        <b>{t('apps.voiceApps')}</b>
        <div className="chips">
          {v.apps.map((a) => (
            <span key={a} className="chip">
              {a}
              <button className="icon-btn" aria-label={`${t('common.delete')}: ${a}`} onClick={() => set({ apps: v.apps.filter((x) => x !== a) })}>
                <X size={12} />
              </button>
            </span>
          ))}
          <button className="btn btn-sm btn-ghost" onClick={() => set({ apps: VOICE_APPS })} data-tip={t('apps.voiceReset')} aria-label={t('apps.voiceReset')}>
            <RotateCcw size={14} />
          </button>
        </div>
        <AppPicker taken={v.apps} onPick={(a) => set({ apps: [...v.apps, a.exe] })} />
      </div>
    </>
  )
}

function Mixer(): React.JSX.Element {
  const t = useT()
  const [v, set] = useVoice()
  const pct = (att: number): number => Math.round(100 * (1 - att))
  return (
    <div className="mixer">
      <span className={v.mixer < 0 ? 'on' : ''}>
        {t('apps.game')} <b className="num">{pct(Math.max(0, v.mixer))} %</b>
      </span>
      <Range value={Math.round(v.mixer * 100)} min={-100} max={100} step={5} label={t('apps.mixer')} onChange={(n) => set({ mixer: n / 100 })} />
      <span className={v.mixer > 0 ? 'on' : ''}>
        {t('apps.chat')} <b className="num">{pct(Math.max(0, -v.mixer))} %</b>
      </span>
      <button className="btn btn-sm" disabled={v.mixer === 0} onClick={() => set({ mixer: 0 })}>
        {t('apps.center')}
      </button>
    </div>
  )
}
