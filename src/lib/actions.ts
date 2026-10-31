import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import { getState, refreshEngine, setState, toast, type SetupStep } from './store'
import { errorText, t } from '../i18n'

/** Instala el motor si falta y activa/quita PoxiEQ en las salidas indicadas (un aviso de UAC). */
export async function runSetup(enable: string[], disable: string[]): Promise<boolean> {
  if (getState().setup) return false
  setState({ setup: getState().engine?.installed ? 'install' : 'download' })
  const off = await listen<string>('engine-progress', (e) => setState({ setup: e.payload as SetupStep }))
  try {
    const r = await api.engineSetup(enable, disable)
    const engine = await refreshEngine()
    if (r.error) {
      toast(`${t('err.engine.setup')}: ${r.error}`, 'err', { secs: 10 })
      return false
    }
    const failed = r.checks.some((c) => !c.preMix && !c.postMix)
    if (failed) toast(t('setup.partial'), 'info', { secs: 12 })
    else if (enable.length) toast(t('setup.ok', { d: engine.devices.find((d) => d.id === enable[0])?.name ?? '' }))
    return true
  } catch (e) {
    toast(errorText(e), 'err', { secs: 8 })
    return false
  } finally {
    off()
    setState({ setup: null })
  }
}
