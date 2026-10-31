import { ArrowLeftRight, Car, Earbuds, Laptop, Smartphone, Speaker, Tv } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Listen, Sim } from '../lib/api'
import { setStudio, useStore } from '../lib/store'
import { useT } from '../i18n'
import { SectionHead } from './Dynamics'

const LISTEN: Listen[] = ['stereo', 'mono', 'side', 'left', 'right']
const SIMS: { id: Sim; icon: LucideIcon }[] = [
  { id: 'phone', icon: Smartphone },
  { id: 'laptop', icon: Laptop },
  { id: 'car', icon: Car },
  { id: 'earbuds', icon: Earbuds },
  { id: 'tv', icon: Tv },
  { id: 'speaker', icon: Speaker }
]

/** Página «Estudio»: monitorización y simulación de otros sistemas (momentáneas: el aviso de arriba lo recuerda) */
export function StudioPage(): React.JSX.Element {
  const t = useT()
  const studio = useStore((s) => s.studio)
  const bypass = useStore((s) => !!s.config?.bypass)

  return (
    <div className="page">
      <section>
        <SectionHead label={t('studio.monitor')} />
        <div className="studio-row">
          <div className="segmented listen" role="radiogroup" aria-label={t('studio.monitor')}>
            {LISTEN.map((l) => (
              <button key={l} role="radio" aria-checked={studio.listen === l} className={studio.listen === l ? 'on' : ''} data-tip={t(`studio.listen.${l}Tip`)} onClick={() => setStudio({ listen: l })}>
                {t(`studio.listen.${l}`)}
              </button>
            ))}
          </div>
          <div className="studio-toggles">
            <button className={`toggle ${studio.flipL ? 'on' : ''}`} aria-pressed={studio.flipL} data-tip={t('studio.flipTip')} onClick={() => setStudio({ flipL: !studio.flipL })}>
              Ø L
            </button>
            <button className={`toggle ${studio.flipR ? 'on' : ''}`} aria-pressed={studio.flipR} data-tip={t('studio.flipTip')} onClick={() => setStudio({ flipR: !studio.flipR })}>
              Ø R
            </button>
            <button className={`toggle ${studio.swap ? 'on' : ''}`} aria-pressed={studio.swap} data-tip={t('studio.swapTip')} onClick={() => setStudio({ swap: !studio.swap })}>
              <ArrowLeftRight size={15} />
              {t('studio.swap')}
            </button>
          </div>
        </div>
        <small className="studio-hint">{t(bypass ? 'studio.hintBypass' : 'studio.hint')}</small>
      </section>

      <section>
        <SectionHead label={t('studio.sim')} />
        <div className="sounds sims">
          {SIMS.map(({ id, icon: Icon }) => (
            <button key={id} className={`sound ${studio.sim === id ? 'on' : ''}`} aria-pressed={studio.sim === id} onClick={() => setStudio({ sim: studio.sim === id ? null : id })}>
              <span className="sound-top">
                <Icon size={18} />
              </span>
              <b>{t(`studio.sims.${id}.name`)}</b>
              <small>{t(`studio.sims.${id}.text`)}</small>
            </button>
          ))}
        </div>
        <small className="studio-hint">{t('studio.simHint')}</small>
      </section>
    </div>
  )
}
