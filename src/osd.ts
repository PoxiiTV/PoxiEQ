// Aviso en pantalla (ventana `osd`): Rust llama a `window.osd(...)` y luego enseña la ventana sin activarla.
// Sin IPC ni permisos: solo pinta lo que le llega.
import '@fontsource-variable/nunito'
import './styles/osd.css'

interface Payload {
  title: string
  sub: string
  kind: 'preset' | 'on' | 'off'
  theme: 'dark' | 'light' | 'system'
  accent: string
}

declare global {
  interface Window {
    osd: (p: Payload) => void
  }
}

/** Lo que se queda a la vista antes de salir (la ventana se esconde un poco después desde Rust) */
const VISIBLE_MS = 1200

const root = document.documentElement
const box = document.getElementById('osd')!
const title = document.getElementById('osd-title')!
const sub = document.getElementById('osd-sub')!
let timer = 0

window.osd = (p) => {
  const light = p.theme === 'light' || (p.theme === 'system' && matchMedia('(prefers-color-scheme: light)').matches)
  root.dataset.theme = light ? 'light' : 'dark'
  root.dataset.accent = p.accent
  title.textContent = p.title
  sub.textContent = p.sub
  box.dataset.kind = p.kind
  // Reinicia la entrada aunque llegue otro aviso con el anterior aún a la vista
  box.classList.remove('show')
  void box.offsetWidth
  box.classList.add('show')
  clearTimeout(timer)
  timer = window.setTimeout(() => box.classList.remove('show'), VISIBLE_MS)
}
