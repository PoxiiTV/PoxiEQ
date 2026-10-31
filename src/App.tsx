import { useEffect } from 'react'
import { api } from './lib/api'
import { autoPreamp } from './lib/eq'
import { allPresets, currentDevice, useStore } from './lib/store'
import { useT } from './i18n'
import { TitleBar, Toasts } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { Editor } from './components/Editor'
import { Meter } from './components/Meter'
import { Setup } from './components/Setup'
import { Settings } from './components/Settings'
import { ImportModal, SavePresetModal, ShareModal, UpdateModal, WhatsNew } from './components/Overlays'
import { AutoEqModal } from './components/AutoEq'
import { ContextMenu } from './components/ContextMenu'

export function App(): React.JSX.Element {
  const t = useT()
  const ready = useStore((s) => !!s.engine?.installed && !!currentDevice(s)?.enabled)
  const device = useStore((s) => (ready ? currentDevice(s)?.id : undefined))
  const lang = useStore((s) => s.config?.lang)
  const presets = useStore((s) => s.config?.presets)

  // Captura de lo que suena (medidores y analizador): solo con la ventana a la vista, en la bandeja no gasta CPU
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

  // El menú de la bandeja y el aviso en pantalla, en el idioma de la app
  useEffect(() => {
    void api.trayLabels({ open: t('tray.open'), bypass: t('tray.bypass'), quit: t('tray.quit'), presets: t('tray.presets'), eqOn: t('tray.eqOn'), eqOff: t('tray.eqOff') })
  }, [lang, t])

  // Rust aplica presets sin la interfaz (atajos, bandeja, preset por app): se le mandan ya resueltos
  useEffect(() => {
    void api.presetsSync(allPresets().map((p) => ({ id: p.id, name: p.name, curve: p.curve, autoGain: autoPreamp(p.curve.bands) })))
  }, [lang, presets])

  return (
    <>
      <div className="ambient" />
      <div className="app">
        <TitleBar />
        <main className="main">
          <Sidebar />
          <section className="stage">{ready ? <Editor /> : <Setup />}</section>
          {ready && <Meter />}
        </main>
      </div>
      <Settings />
      <SavePresetModal />
      <ShareModal />
      <ImportModal />
      <AutoEqModal />
      <UpdateModal />
      <WhatsNew />
      <ContextMenu />
      <Toasts />
    </>
  )
}
