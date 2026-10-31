import type { Config } from './api'

const media = window.matchMedia('(prefers-color-scheme: light)')

export function applyTheme(cfg: Pick<Config, 'theme' | 'accent'>): void {
  const root = document.documentElement
  root.dataset.theme = cfg.theme === 'system' ? (media.matches ? 'light' : 'dark') : cfg.theme
  root.dataset.accent = cfg.accent
}

/** Con «Como Windows», seguir los cambios del sistema en vivo */
export function followSystem(get: () => Config | null): void {
  media.addEventListener('change', () => {
    const c = get()
    if (c?.theme === 'system') applyTheme(c)
  })
}
