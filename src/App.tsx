import { useEffect } from 'react'
import { api } from './lib/api'
import { autoPreamp } from './lib/eq'
import { allPresets, currentDevice, currentOutput, useStore, works } from './lib/store'
import { useT } from './i18n'
import { TitleBar, Toasts } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { Banners, EqPage } from './components/Editor'
import { SoundPage } from './components/Dynamics'
import { PhonesPage } from './components/Phones'
import { StudioPage } from './components/Studio'
import { RoomModal, RoomPage } from './components/Room'
import { MicPage } from './components/Mic'
import { PluginsPage } from './components/Plugins'
import { AppsPage } from './components/Apps'
import { SettingsPage } from './components/Settings'
import { Meter } from './components/Meter'
import { Setup } from './components/Setup'
import { ImportModal, SavePresetModal, ShareModal, UpdateModal, WhatsNew } from './components/Overlays'
import { HearingModal } from './components/Hearing'
import { DiagnoseModal } from './components/Diagnose'
import { ContextMenu } from './components/ContextMenu'
import { QuickPage } from './components/Quick'

export function App(): React.JSX.Element {
  const t = useT()
  // Lo que miden los medidores: el micrófono en su página y, en el resto, la salida (si ya pasa por PoxiEQ)
  const metered = useStore((s) => {
    const d = currentDevice(s)
    return s.engine?.installed && works(d) ? d?.id : undefined
  })
  const lang = useStore((s) => s.config?.lang)
  const presets = useStore((s) => s.config?.presets)
  const quick = useStore((s) => !!s.config?.quick.on)
  const ready = useStore((s) => !!s.engine?.installed && works(currentOutput(s)))

  // Captura de lo que suena (medidores y analizador): solo con la ventana a la vista, en la bandeja no gasta CPU
  useEffect(() => {
    if (!metered) return
    const sync = (): void => void (document.hidden ? api.analyzerStop() : api.analyzerStart(metered))
    sync()
    document.addEventListener('visibilitychange', sync)
    return () => {
      document.removeEventListener('visibilitychange', sync)
      void api.analyzerStop()
    }
  }, [metered])

  // El menú de la bandeja y el aviso en pantalla, en el idioma de la app
  useEffect(() => {
    void api.trayLabels({ open: t('tray.open'), bypass: t('tray.bypass'), quit: t('tray.quit'), presets: t('tray.presets'), eqOn: t('tray.eqOn'), eqOff: t('tray.eqOff'), broken: t('tray.broken'), repair: t('tray.repair'), pluginsOff: t('tray.pluginsOff'), game: t('apps.game'), chat: t('apps.chat'), mini: t('tray.mini') })
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
        {quick ? (
          // Modo rápido: sin menú ni medidores, solo los presets (o la activación si esta salida no la tiene)
          <main className="main">
            <section className="stage">{ready ? <QuickPage /> : <Setup />}</section>
          </main>
        ) : (
          <main className="main">
            <Sidebar />
            <section className="stage">
              <Page />
            </section>
            {metered && <Meter />}
          </main>
        )}
      </div>
      <SavePresetModal />
      <ShareModal />
      <ImportModal />
      <HearingModal />
      <RoomModal />
      <DiagnoseModal />
      <UpdateModal />
      <WhatsNew />
      <ContextMenu />
      <Toasts />
    </>
  )
}

/** La sección elegida en el menú. Las de las salidas piden antes activar el motor en la salida que se edita. */
function Page(): React.JSX.Element {
  const page = useStore((s) => s.page)
  const ready = useStore((s) => !!s.engine?.installed && works(currentOutput(s)))
  if (page === 'settings') return <SettingsPage />
  if (page === 'mic') return <MicPage />
  if (page === 'plugins') return <PluginsPage />
  if (page === 'apps') return <AppsPage />
  if (!ready) return <Setup />
  return (
    <>
      <Banners />
      {page === 'eq' && <EqPage />}
      {page === 'sound' && <SoundPage />}
      {page === 'phones' && <PhonesPage />}
      {page === 'studio' && <StudioPage />}
      {page === 'room' && <RoomPage />}
    </>
  )
}
