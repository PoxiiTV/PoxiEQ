import { LoaderCircle, RotateCcw, ShieldCheck, Zap } from 'lucide-react'
import { runSetup } from '../lib/actions'
import { currentDevice, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Logo } from './ui'

/** Pantalla cuando el motor falta o la salida elegida aún no pasa por PoxiEQ */
export function Setup(): React.JSX.Element {
  const t = useT()
  const installed = useStore((s) => !!s.engine?.installed)
  const device = useStore((s) => currentDevice(s))
  const step = useStore((s) => s.setup)

  if (!device) {
    return (
      <div className="setup">
        <div className="setup-card">
          <Logo size={64} />
          <h1>{t('device.none')}</h1>
        </div>
      </div>
    )
  }
  return (
    <div className="setup">
      <div className="setup-card">
        <Logo size={72} />
        <h1>{installed ? t('setup.title', { d: device.name }) : t('setup.titleInstall')}</h1>
        <p className="device-name">{device.device}</p>
        <p>{!installed ? t('setup.textInstall') : device.input ? t('setup.textMic') : t('setup.text')}</p>
        <ul className="setup-points">
          <li>
            <RotateCcw size={16} /> {t('setup.noReboot')}
          </li>
          <li>
            <ShieldCheck size={16} /> {t('setup.uac')}
          </li>
        </ul>
        <button className="btn btn-primary btn-lg" disabled={!!step} onClick={() => void runSetup([device.id], [])}>
          {step ? <LoaderCircle size={18} className="spin" /> : <Zap size={18} />}
          {step === 'download' ? t('setup.download') : step ? t('setup.install') : installed ? t('setup.btn') : t('setup.btnInstall')}
        </button>
      </div>
    </div>
  )
}
