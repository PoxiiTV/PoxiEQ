import { useCallback, useEffect, useState } from 'react'
import { CheckCircle2, LoaderCircle, SearchCheck, TriangleAlert } from 'lucide-react'
import { api, type Device, type Diagnosis } from '../lib/api'
import { runSetup } from '../lib/actions'
import { currentMic, currentOutput, setState, toast, updateConfig, useStore } from '../lib/store'
import { errorText, useT } from '../i18n'
import { Modal } from './ui'

const close = (): void => setState({ modal: null })

interface Finding {
  key: string
  text: string
  fix?: { label: string; run: () => void }
}

/** «¿Por qué no suena?»: repasa la salida y el micrófono que se ajustan y dice qué falla, cada cosa con su arreglo.
 *  Lo que se ve sin permisos va solo; lo de si el motor carga de verdad pide un permiso de administrador. */
export function DiagnoseModal(): React.JSX.Element | null {
  const open = useStore((s) => s.modal === 'diagnose')
  if (!open) return null
  return <Diagnose />
}

function Diagnose(): React.JSX.Element {
  const t = useT()
  const engine = useStore((s) => s.engine)
  const output = useStore((s) => currentOutput(s))
  const mic = useStore((s) => currentMic(s))
  const devices = [output, mic && (mic.enabled || mic.broken) ? mic : null].filter((d): d is Device => !!d)
  const ids = devices.map((d) => d.id).join()
  const [diag, setDiag] = useState<Record<string, Diagnosis>>({})
  const [deep, setDeep] = useState<Record<string, boolean>>({})
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(() => {
    void Promise.all(ids.split(',').filter(Boolean).map(async (id) => [id, await api.engineDiagnose(id)] as const)).then((r) => setDiag(Object.fromEntries(r)))
  }, [ids])
  useEffect(load, [load, engine])

  const fix = async (run: () => unknown): Promise<void> => {
    await run()
    load()
  }
  const check = async (d: Device): Promise<void> => {
    setBusy(d.id)
    try {
      const c = await api.engineCheck(d.id)
      setDeep((x) => ({ ...x, [d.id]: c.preMix && c.postMix }))
    } catch (e) {
      toast(errorText(e), 'err', { secs: 8 })
    } finally {
      setBusy(null)
    }
  }

  return (
    <Modal onClose={close} wide className="diag">
      <h2>
        <SearchCheck size={20} /> {t('diag.title')}
      </h2>
      <p className="hint">{t('diag.hint')}</p>
      {devices.length === 0 && <p>{t('device.none')}</p>}
      {devices.map((d) => {
        const list = findings(d)
        return (
          <section key={d.id} className="diag-dev">
            <h3>
              {d.name} <small>{d.device}</small>
            </h3>
            {list.length === 0 && deep[d.id] !== false && (
              <div className="diag-item ok">
                <CheckCircle2 size={16} />
                <span>{t(deep[d.id] ? 'diag.allOkDeep' : 'diag.allOk')}</span>
              </div>
            )}
            {list.map((f) => (
              <div key={f.key} className="diag-item">
                <TriangleAlert size={16} />
                <span>{f.text}</span>
                {f.fix && (
                  <button className="btn btn-sm btn-primary" onClick={() => void fix(f.fix!.run)}>
                    {f.fix.label}
                  </button>
                )}
              </div>
            ))}
            {d.enabled && !d.broken && deep[d.id] === undefined && (
              <button className="btn btn-sm btn-ghost" disabled={!!busy} onClick={() => void check(d)} data-tip={t('diag.deepTip')}>
                {busy === d.id && <LoaderCircle size={14} className="spin" />}
                {t('diag.deep')}
              </button>
            )}
          </section>
        )
      })}
      <div className="modal-actions">
        <button className="btn btn-primary" onClick={close}>
          {t('common.close')}
        </button>
      </div>
    </Modal>
  )

  function findings(d: Device): Finding[] {
    const cfg = useStore.getState().config
    const out: Finding[] = []
    const add = (key: string, text: string, fix?: Finding['fix']): void => void out.push({ key, text, fix })
    const repair = { label: t('setup.repair'), run: () => runSetup([d.id], []) }
    if (!engine?.installed && !d.broken) add('engine', t('diag.engine'), { label: t('setup.btnInstall'), run: () => runSetup([d.id], []) })
    else if (d.broken) add('broken', t(`setup.broken.${d.broken}`), repair)
    else if (!d.enabled) add('off', t('diag.off'), { label: t('setup.btn'), run: () => runSetup([d.id], []) })
    if (deep[d.id] === false) add('notLoaded', t('diag.notLoaded'), repair)
    if (d.input ? cfg?.mic.bypass : cfg?.bypass)
      add('bypass', t(d.input ? 'diag.bypassMic' : 'diag.bypass'), {
        label: t('diag.turnOn'),
        run: () => (d.input && cfg ? updateConfig({ mic: { ...cfg.mic, bypass: false } }) : updateConfig({ bypass: false }))
      })
    const def = engine?.devices.find((x) => x.input === d.input && x.default)
    if (def && !d.default)
      add('notDefault', t(d.input ? 'diag.notDefaultMic' : 'diag.notDefault', { d: def.name }), {
        label: t('diag.useDefault', { d: def.name }),
        run: () => updateConfig(d.input ? { micDevice: def.id } : { device: def.id })
      })
    const x = diag[d.id]
    if (x?.muted) add('muted', t('diag.muted'))
    if (x?.spatial) add('spatial', t('diag.spatial'), { label: t('diag.openSound'), run: () => api.soundSettings() })
    if (x?.exclusive) add('exclusive', t('diag.exclusive'))
    if (x?.vendor.length) add('vendor', t('diag.vendor', { n: x.vendor.join(', ') }))
    if (!d.input && engine?.installed && engine.dsp !== 'current') add('dsp', t('diag.dsp'), { label: t('dsp.updateBtn'), run: () => runSetup([], []) })
    return out
  }
}
