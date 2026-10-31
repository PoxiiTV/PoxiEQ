import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import '@fontsource-variable/nunito'
import './styles/app.css'
import { api, type Config } from './lib/api'
import { currentCurve, getState, grouper, redo, refreshEngine, setState, STUDIO_OFF, toast, undo } from './lib/store'
import { keyLabel } from './lib/hotkeys'
import { MAX_FILE } from './lib/share'
import { t } from './i18n'
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
  // Deshacer / rehacer la curva y la dinámica (en un campo de texto, el suyo; con una ventana abierta, nada)
  if (!e.ctrlKey || e.altKey || (k !== 'z' && k !== 'y')) return
  const el = e.target as HTMLElement
  const typing = el.closest('textarea') || (el instanceof HTMLInputElement && el.type !== 'range')
  if (typing || document.querySelector('.modal-backdrop')) return
  e.preventDefault()
  if (k === 'y' || e.shiftKey) redo()
  else undo()
})

// Un arrastre (curva, deslizador) es un solo paso de deshacer: se agrupa mientras haya un botón pulsado
window.addEventListener('pointerdown', () => grouper.down(), true)
window.addEventListener('pointerup', () => grouper.up(), true)
window.addEventListener('pointercancel', () => grouper.up(), true)

// Arrastrar un `.poxieq` a la ventana lo importa; cualquier otro archivo se ignora (sin que el navegador lo abra)
window.addEventListener('dragover', (e) => {
  if (e.dataTransfer?.types.includes('Files')) e.preventDefault()
})
window.addEventListener('drop', (e) => {
  if (e.defaultPrevented) return
  e.preventDefault()
  const f = e.dataTransfer?.files[0]
  if (!f || !getState().config) return
  if (!f.name.toLowerCase().endsWith('.poxieq') || f.size > MAX_FILE) return toast(t('share.dropBad'), 'info')
  void f.text().then((importCode) => setState({ importCode, modal: 'import' }))
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
  // Cambios hechos desde Rust (bandeja, atajos, preset por app): ya están guardados, aquí solo se pintan
  void listen<Config>('config', (e) => {
    const s = getState()
    const bands = currentCurve({ ...s, config: e.payload }).bands.length
    setState({ config: e.payload, selected: s.selected !== null && s.selected < bands ? s.selected : null })
  })
  // Al esconder la ventana Rust quita lo momentáneo (monitorización, simulación, test de audición, medición de
  // sala): aquí se refleja
  void listen('live-reset', () => setState((s) => ({ studio: STUDIO_OFF, studioPreset: null, modal: s.modal === 'hearing' || s.modal === 'room' ? null : s.modal })))
  // Atajos que tiene otro programa: se avisa de los nuevos (y Ajustes los marca)
  const failedHotkeys = (list: string[]): void => {
    const fresh = list.filter((k) => !getState().hotkeysFailed.includes(k))
    setState({ hotkeysFailed: list })
    if (fresh.length) toast(t('keys.failed', { k: fresh.map(keyLabel).join(', ') }), 'err', { secs: 10 })
  }
  void listen<string[]>('hotkeys-failed', (e) => failedHotkeys(e.payload))
  void api.hotkeysFailed().then(failedHotkeys)
  // Windows ha cambiado de salida predeterminada o se ha conectado algo: Rust ya ha pasado a esa salida si se
  // sigue a Windows (llega su id) y aquí se refresca la lista y se avisa. Si aún no tiene el motor activado, se
  // ve la tarjeta de activación (nunca se lanza el permiso de administrador solo).
  void listen<string | null>('devices', (e) =>
    void refreshEngine().then((engine) => {
      const d = e.payload && engine.devices.find((x) => x.id === e.payload)
      if (d) toast(t('follow.now', { d: `${d.name} · ${d.device}` }), 'info', { secs: 4 })
    })
  )
  // Por si algo cambió sin aviso mientras la ventana estaba escondida
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
