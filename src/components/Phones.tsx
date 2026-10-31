import { Ear } from 'lucide-react'
import type { Preset } from '../lib/api'
import { applyPreset, currentCurve, setState, useStore } from '../lib/store'
import { useT } from '../i18n'
import { AutoEqPanel, Preview } from './AutoEq'
import { HeadphoneCards, Install, SectionHead } from './Dynamics'

const NONE: Preset[] = []

/** Página «Cascos»: corrección de tu modelo, escucha con cascos y test de audición */
export function PhonesPage(): React.JSX.Element {
  const t = useT()
  return (
    <div className="page">
      <section>
        <SectionHead n={1} label={t('autoeq.title')} />
        <AutoEqPanel />
      </section>
      <section>
        <SectionHead n={2} label={t('phones.listening')} />
        <Install />
        <small className="studio-hint">{t('phones.listeningHint')}</small>
        <HeadphoneCards />
      </section>
      <section>
        <SectionHead n={3} label={t('hearing.title')} />
        <HearingCard />
      </section>
    </div>
  )
}

/** Test de audición: el asistente va en una ventana; aquí, el último resultado guardado si lo hay */
function HearingCard(): React.JSX.Element {
  const t = useT()
  const presets = useStore((s) => s.config?.presets ?? NONE)
  const active = useStore((s) => currentCurve(s).preset)
  // El preset del test es el único con ajuste por oído; repetir el test lo sustituye
  const result = presets.findLast((p) => p.curve.ears)
  return (
    <div className="dyn-card">
      <div className="dyn-card-head">
        <Ear size={16} />
        <b>{t('hearing.cardTitle')}</b>
        {result && active === result.id && <span className="chip ok">{t('common.inUse')}</span>}
      </div>
      <small>{t('hearing.cardText')}</small>
      {result && (
        <div className="hearing-curves">
          {(['left', 'right'] as const).map((e) => (
            <div key={e}>
              <span className="label">{t(`hearing.${e}`)}</span>
              <Preview bands={[...result.curve.bands, ...(result.curve.ears?.[e] ?? [])]} />
            </div>
          ))}
        </div>
      )}
      <div className="panel-actions start">
        <button className={`btn btn-sm ${result ? '' : 'btn-primary'}`} onClick={() => setState({ modal: 'hearing' })}>
          {t(result ? 'hearing.again' : 'hearing.start')}
        </button>
        {result && active !== result.id && (
          <button className="btn btn-sm btn-primary" onClick={() => applyPreset(result.curve, result.id)}>
            {t('phones.apply', { n: result.name })}
          </button>
        )}
      </div>
    </div>
  )
}
