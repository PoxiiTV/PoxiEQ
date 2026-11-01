import { LoaderCircle, RotateCcw, ShieldCheck, Wrench, Zap } from 'lucide-react'
import { runSetup } from '../lib/actions'
import { currentDevice, editingMic, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Logo } from './ui'

/** Tarjeta cuando el motor falta o lo que se edita (la salida o, en su página, el micrófono) aún no pasa por
 *  PoxiEQ, o pasaba y algo lo rompió (entonces «Reparar», que es activarlo otra vez). Activar es un aviso de UAC
 *  por dispositivo: solo con el botón, nunca solo. */
export function Setup(): React.JSX.Element {
  const t = useT()
  const installed = useStore((s) => !!s.engine?.installed)
  const device = useStore((s) => currentDevice(s))
  const mic = useStore((s) => editingMic(s))
  const step = useStore((s) => s.setup)

  if (!device) {
    return (
      <div className="setup">
        <div className="setup-card">
          <Logo size={64} />
          <h1>{t(mic ? 'mic.none' : 'device.none')}</h1>
        </div>
      </div>
    )
  }
  const broken = device.broken
  const title = broken ? t('setup.titleBroken', { d: device.name }) : installed ? t('setup.title', { d: device.name }) : t('setup.titleInstall')
  const text = broken ? t(`setup.broken.${broken}`) : !installed ? t('setup.textInstall') : device.input ? t('setup.textMic') : t('setup.text')
  const btn = broken ? t('setup.repair') : installed ? t('setup.btn') : t('setup.btnInstall')
  return (
    <div className="setup">
      <div className="setup-card">
        <Logo size={72} />
        <h1>{title}</h1>
        <p className="device-name">{device.device}</p>
        <p>{text}</p>
        <ul className="setup-points">
          <li>
            <RotateCcw size={16} /> {t('setup.noReboot')}
          </li>
          <li>
            <ShieldCheck size={16} /> {t('setup.uac')}
          </li>
        </ul>
        <button className="btn btn-primary btn-lg" disabled={!!step} onClick={() => void runSetup([device.id], [])}>
          {step ? <LoaderCircle size={18} className="spin" /> : broken ? <Wrench size={18} /> : <Zap size={18} />}
          {step === 'download' ? t('setup.download') : step ? t(broken ? 'setup.repairing' : 'setup.install') : btn}
        </button>
      </div>
    </div>
  )
}
