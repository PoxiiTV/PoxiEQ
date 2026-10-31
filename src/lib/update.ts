import { create } from 'zustand'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getState } from './store'

// Actualizaciones: al abrir y cada 6 h. El instalador se actualiza solo (firmado); el portable abre la descarga.

export const RELEASES = 'https://github.com/PoxiiTV/PoxiEQ/releases/latest'
const EVERY_MS = 6 * 3600_000

interface UpdState {
  update: Update | null
  version: string
  notes: string
  progress: number | null
}

export const useUpdate = create<UpdState>(() => ({ update: null, version: '', notes: '', progress: null }))

let shown = ''

/** Devuelve `true` si hay versión nueva (y saca el aviso). */
export async function checkForUpdates(manual = false): Promise<boolean> {
  try {
    const u = await check()
    if (!u) return false
    if (manual || u.version !== shown) {
      shown = u.version
      useUpdate.setState({ update: u, version: u.version, notes: u.body ?? '', progress: null })
    }
    return true
  } catch {
    return false
  }
}

export function startUpdateChecks(): void {
  if (import.meta.env.DEV) return
  setTimeout(() => void checkForUpdates(), 4000)
  setInterval(() => void checkForUpdates(), EVERY_MS)
}

export async function installUpdate(): Promise<void> {
  const u = useUpdate.getState().update
  if (!u) return
  let total = 0
  let done = 0
  await u.downloadAndInstall((ev) => {
    if (ev.event === 'Started') total = ev.data.contentLength ?? 0
    if (ev.event === 'Progress') {
      done += ev.data.chunkLength
      useUpdate.setState({ progress: total ? Math.round((done / total) * 100) : null })
    }
  })
  await relaunch()
}

export const isPortable = (): boolean => !!getState().app?.portable
