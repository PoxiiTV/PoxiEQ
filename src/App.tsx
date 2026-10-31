import { useEffect } from 'react'
import { api } from './lib/api'
import { currentDevice, useStore } from './lib/store'
import { useT } from './i18n'
import { TitleBar, Toasts } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { Editor } from './components/Editor'
import { Setup } from './components/Setup'
import { Settings } from './components/Settings'
import { SavePresetModal, UpdateModal, WhatsNew } from './components/Overlays'
import { AutoEqModal } from './components/AutoEq'
import { ContextMenu } from './components/ContextMenu'

export function App(): React.JSX.Element {
  const t = useT()
  const ready = useStore((s) => !!s.engine?.installed && !!currentDevice(s)?.enabled)
  const lang = useStore((s) => s.config?.lang)

  // El menú de la bandeja, en el idioma de la app
  useEffect(() => {
    void api.trayLabels({ open: t('tray.open'), bypass: t('tray.bypass'), quit: t('tray.quit') })
  }, [lang, t])

  return (
    <>
      <div className="ambient" />
      <div className="app">
        <TitleBar />
        <main className="main">
          <Sidebar />
          <section className="stage">{ready ? <Editor /> : <Setup />}</section>
        </main>
      </div>
      <Settings />
      <SavePresetModal />
      <AutoEqModal />
      <UpdateModal />
      <WhatsNew />
      <ContextMenu />
      <Toasts />
    </>
  )
}
