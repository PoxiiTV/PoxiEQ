import { Mic as MicIcon, Power } from 'lucide-react'
import { currentMic, setState, updateConfig, useStore, works } from '../lib/store'
import { useT } from '../i18n'
import { Select } from './controls'
import { AddBand, CurveEditor, HistoryButtons } from './Editor'
import { Install, MicDynamics, SectionHead } from './Dynamics'
import { SectionPresets } from './Presets'
import { Setup } from './Setup'

/** Página «Micrófono»: su propio selector y encendido, su EQ, la puerta de ruido y el compresor. Lo que se ajusta
 *  aquí lo reciben Discord, OBS y cualquier app que use el micro; los medidores enseñan el micro mientras tanto. */
export function MicPage(): React.JSX.Element {
  const t = useT()
  const devices = useStore((s) => s.engine?.devices)
  const mic = useStore((s) => currentMic(s))
  const ready = useStore((s) => !!s.engine?.installed && works(currentMic(s)))
  const off = useStore((s) => !!s.config?.mic.bypass)
  const mics = (devices ?? []).filter((d) => d.input && !d.unplugged)
  const toggle = (): void => {
    const cfg = useStore.getState().config
    if (cfg) updateConfig({ mic: { ...cfg.mic, bypass: !cfg.mic.bypass } })
  }
  const tip = t(off ? 'mic.bypassOn' : 'mic.bypassOff')

  return (
    <div className="page mic-page">
      <div className="toolbar">
        {mic && (
          <div className="device-pick mic">
            <span className={`dot ${works(mic) ? 'ok' : mic.broken ? 'bad' : ''}`} />
            <span className="kind">
              <MicIcon size={15} />
            </span>
            <Select
              size="sm"
              value={mic.id}
              label={t('mic.pick')}
              options={mics.map((d) => ({ value: d.id, label: `${d.name} · ${d.device}${d.default ? ` (${t('device.default')})` : ''}` }))}
              onChange={(id) => {
                setState({ selected: null })
                updateConfig({ micDevice: id })
              }}
            />
          </div>
        )}
        <div className="spacer" />
        {ready && (
          <button className={`power tip-right ${off ? 'bypassed' : ''}`} onClick={toggle} data-tip={tip} aria-label={tip} aria-pressed={!off}>
            <Power size={15} />
            <span>MIC</span>
          </button>
        )}
      </div>
      {!ready ? (
        <Setup />
      ) : (
        <>
          <SectionPresets section="mic">
            <div className="spacer" />
            <HistoryButtons />
            <AddBand />
          </SectionPresets>
          {off && (
            <div className="banner">
              <Power size={16} />
              <span>{t('mic.bypassBanner')}</span>
              <div className="spacer" />
              <button className="btn btn-sm btn-primary" onClick={toggle}>
                {t('eq.bypassResume')}
              </button>
            </div>
          )}
          <small className="studio-hint">{t('mic.intro')}</small>
          <section>
            <SectionHead n={1} label={t('mic.eq')} />
            <CurveEditor />
          </section>
          <section>
            <SectionHead n={2} label={t('mic.dynamics')} />
            <Install />
            <MicDynamics />
          </section>
        </>
      )}
    </div>
  )
}
