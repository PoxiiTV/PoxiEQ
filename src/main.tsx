import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import '@fontsource-variable/nunito'
import './styles/app.css'
import { api } from './lib/api'
import { getState, refreshEngine, setState } from './lib/store'
import { applyTheme, followSystem } from './lib/theme'
import { startUpdateChecks } from './lib/update'
import { App } from './App'

// Nada del navegador: ni su menú contextual ni sus atajos (imprimir, buscar, recargar, guardar página…)
window.addEventListener('contextmenu', (e) => {
  if (!import.meta.env.DEV || !e.shiftKey) e.preventDefault()
})
window.addEventListener('keydown', (e) => {
  const k = e.key.toLowerCase()
  if (e.key === 'F5' || e.key === 'F3' || e.key === 'F7' || (e.ctrlKey && ['r', 'p', 's', 'g', 'u', 'j', 'h', 'o', 'd', 'f'].includes(k))) e.preventDefault()
})

async function start(): Promise<void> {
  const [app, config, engine] = await Promise.all([api.appInfo(), api.configGet(), api.engineStatus()])
  applyTheme(config)
  followSystem(() => getState().config)
  setState({ app, config, engine })
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>
  )
  // Bypass desde la bandeja o el atajo: Rust ya lo guardó, aquí solo se pinta
  void listen<boolean>('bypass', (e) => {
    const c = getState().config
    if (c) setState({ config: { ...c, bypass: e.payload } })
  })
  // Cascos enchufados o desenchufados mientras la app estaba escondida
  window.addEventListener('focus', () => void refreshEngine())
  // Se enseña la ventana ya pintada (sin parpadeo blanco), salvo si arrancó con Windows en la bandeja
  if (!app.startHidden) requestAnimationFrame(() => void getCurrentWindow().show())
  startUpdateChecks()
}

async function boot(): Promise<void> {
  // Si Rust aún no estuviera listo, se reintenta en vez de quedarse la ventana en negro
  for (let i = 0; ; i++) {
    try {
      return await start()
    } catch (e) {
      if (i >= 10) throw e
      await new Promise((r) => setTimeout(r, 200))
    }
  }
}

void boot()
