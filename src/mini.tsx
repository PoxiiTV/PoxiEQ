// Mini-barra (ventana `mini`): siempre encima y sin quitar el foco. Bypass, preset (anterior/siguiente), medidor y
// LUFS de la salida que se edita. Comparte con la app la configuración (Rust manda `config` en cada cambio).
import { StrictMode, useEffect, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { listen } from '@tauri-apps/api/event'
import { ChevronLeft, ChevronRight, Power, X } from 'lucide-react'
import '@fontsource-variable/nunito'
import './styles/mini.css'
import { api, type Config } from './lib/api'
import { allPresets, applyPreset, currentCurve, currentOutput, setState, updateConfig, useStore } from './lib/store'
import { useT } from './i18n'
import { applyTheme } from './lib/theme'

interface Levels {
  peak: number[]
  /** LUFS a corto plazo (3 s) */
  shortTerm: number
}

const MIN = -60
const frac = (db: number): number => Math.min(1, Math.max(0, (db - MIN) / -MIN))

function Mini(): React.JSX.Element {
  const t = useT()
  const bypass = useStore((s) => !!s.config?.bypass)
  const active = useStore((s) => currentCurve(s).preset)
  const device = useStore((s) => currentOutput(s)?.id)
  const cfg = useStore((s) => s.config)
  const presets = useMemo(() => (cfg ? allPresets(cfg) : []), [cfg])
  const [lv, setLv] = useState<Levels | null>(null)

  useEffect(() => {
    const off = listen<Levels>('levels', (e) => setLv(e.payload))
    return () => void off.then((f) => f())
  }, [])
  // El medidor solo mientras la barra está a la vista (Rust lo para cuando ninguna ventana lo pide)
  useEffect(() => {
    if (!device) return
    const sync = (): void => void (document.hidden ? api.analyzerStop() : api.analyzerStart(device))
    sync()
    document.addEventListener('visibilitychange', sync)
    return () => {
      document.removeEventListener('visibilitychange', sync)
      void api.analyzerStop()
    }
  }, [device])

  const i = presets.findIndex((p) => p.id === active)
  const step = (d: number): void => {
    if (!presets.length) return
    const p = presets[(i + d + presets.length) % presets.length]
    applyPreset(p.curve, p.id)
  }
  const name = i >= 0 ? presets[i].name : t('presets.custom')
  return (
    <div className="mini" data-tauri-drag-region>
      <button className={`mini-power ${bypass ? 'off' : ''}`} onClick={() => updateConfig({ bypass: !bypass })} aria-pressed={!bypass} aria-label={t(bypass ? 'titlebar.bypassOn' : 'titlebar.bypassOff')}>
        <Power size={15} />
      </button>
      <div className="mini-preset">
        <button className="mini-btn" onClick={() => step(-1)} aria-label={t('mini.prev')}>
          <ChevronLeft size={15} />
        </button>
        <b title={name}>{name}</b>
        <button className="mini-btn" onClick={() => step(1)} aria-label={t('mini.next')}>
          <ChevronRight size={15} />
        </button>
      </div>
      <div className="mini-meter" aria-hidden data-tauri-drag-region>
        {[0, 1].map((c) => (
          <i key={c} style={{ transform: `scaleX(${frac(lv?.peak[c] ?? MIN)})` }} />
        ))}
      </div>
      <span className="mini-lufs num" data-tauri-drag-region>
        {lv && lv.shortTerm > -70 ? lv.shortTerm.toFixed(1) : '−∞'}
        <small>LUFS</small>
      </span>
      <button className="mini-btn" onClick={() => void api.miniSet(false)} aria-label={t('common.close')}>
        <X size={14} />
      </button>
    </div>
  )
}

async function start(): Promise<void> {
  const [config, engine] = await Promise.all([api.configGet(), api.engineStatus()])
  applyTheme(config)
  setState({ config, engine })
  void listen<Config>('config', (e) => {
    applyTheme(e.payload)
    setState({ config: e.payload })
  })
  void listen('devices', () => void api.engineStatus().then((engine) => setState({ engine })))
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <Mini />
    </StrictMode>
  )
}

void start()
