import { useState } from 'react'
import { FileAudio, LoaderCircle, Trash2, Upload } from 'lucide-react'
import { api, type Ir } from '../lib/api'
import { currentOutput, outputSettings, toast, updateConfig, useStore } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Switch } from './ui'

const W = 320
const H = 90
/** Margen de la curva (dB por encima y por debajo de su valor a 1 kHz) */
const SPAN = 18

function setIr(ir: Ir | null): void {
  const s = useStore.getState()
  const id = currentOutput(s)?.id
  if (id && s.config) updateConfig({ perDevice: { ...s.config.perDevice, [id]: { ...outputSettings(s), ir } } })
}

/** Respuesta al impulso de la salida (una por salida): BRIR para cascos o FIR de corrección de sala */
export function ConvolutionCard({ hint }: { hint: string }): React.JSX.Element {
  const t = useT()
  const ir = useStore((s) => outputSettings(s).ir)
  const [busy, setBusy] = useState(false)
  const load = async (): Promise<void> => {
    const path = await api.pickFile('wav')
    if (!path) return
    setBusy(true)
    try {
      setIr(await api.irLoad(path))
      toast(t('ir.loaded'))
    } catch (e) {
      toast(errorText(e), 'err', { secs: 8 })
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className={`dyn-card ir-card ${ir?.on ? 'on' : ''}`} role="group" aria-label={t('ir.title')}>
      <div className="dyn-card-head">
        <FileAudio size={16} />
        <b>{t('ir.title')}</b>
        {ir && <Switch on={ir.on} label={t('ir.title')} onChange={(on) => setIr({ ...ir, on })} />}
      </div>
      <small className="studio-hint">{hint}</small>
      {ir && (
        <div className="ir-info">
          <IrCurve response={ir.response} />
          <div className="ir-meta">
            <b title={ir.name}>{ir.name}</b>
            <small>{t('ir.meta', { c: ir.channels, r: (ir.rate / 1000).toFixed(1), s: ir.seconds.toFixed(2), l: ir.latency.toFixed(1) })}</small>
          </div>
        </div>
      )}
      <div className="ir-actions">
        <button className="btn btn-sm" disabled={busy} onClick={() => void load()}>
          {busy ? <LoaderCircle size={14} className="spin" /> : <Upload size={14} />}
          {t(ir ? 'ir.replace' : 'ir.load')}
        </button>
        {ir && (
          <button className="btn btn-sm btn-ghost" onClick={() => setIr(null)}>
            <Trash2 size={14} />
            {t('ir.remove')}
          </button>
        )}
      </div>
    </div>
  )
}

/** Respuesta en frecuencia (20 Hz–20 kHz, logarítmica), centrada en su valor a 1 kHz */
function IrCurve({ response }: { response: number[] }): React.JSX.Element {
  const n = response.length
  const ref = response[Math.round(((n - 1) * Math.log10(50)) / 3)] ?? 0
  const pts = response.map((v, i) => `${((i / (n - 1)) * W).toFixed(1)},${(H / 2 - ((Math.max(-SPAN, Math.min(SPAN, v - ref)) / SPAN) * H) / 2).toFixed(1)}`).join(' ')
  return (
    <svg className="ir-curve" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden>
      <line x1={0} x2={W} y1={H / 2} y2={H / 2} />
      <polyline points={pts} />
    </svg>
  )
}
