import { useEffect, useRef, useState } from 'react'
import { ChevronDown, Check, TriangleAlert } from 'lucide-react'
import { api } from '../lib/api'
import { comboFromEvent, keyLabel } from '../lib/hotkeys'
import { errorText, useT } from '../i18n'

export interface Option<T extends string | number> {
  value: T
  label: string
  /** Encabezado del grupo: se pinta encima de la primera opción de cada grupo */
  group?: string
}

/** Desplegable propio (el `<select>` del navegador abre una lista nativa) */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  size,
  block,
  label
}: {
  value: T
  options: Option<T>[]
  onChange: (v: T) => void
  size?: 'sm'
  block?: boolean
  label?: string
}): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const [up, setUp] = useState(false)
  const [hl, setHl] = useState(0)
  const root = useRef<HTMLDivElement>(null)
  const current = options.find((o) => o.value === value)

  useEffect(() => {
    if (!open) return
    setHl(Math.max(0, options.findIndex((o) => o.value === value)))
    const onDown = (e: MouseEvent): void => {
      if (!root.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        setOpen(false)
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        setHl((i) => (i + (e.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length)
      } else if (e.key === 'Enter') {
        e.preventDefault()
        setHl((i) => {
          const o = options[i]
          if (o) onChange(o.value)
          return i
        })
        setOpen(false)
      }
    }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [open, options, value, onChange])

  return (
    <div ref={root} className={`dd ${open ? 'open' : ''} ${up ? 'up' : ''} ${size ?? ''} ${block ? 'block' : ''}`}>
      <button
        type="button"
        className="dd-btn"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={label}
        onClick={() => {
          if (!open && root.current) setUp(opensUp(root.current, options.length))
          setOpen(!open)
        }}
      >
        <span>{current?.label ?? ''}</span>
        <ChevronDown size={15} />
      </button>
      {open && (
        <div className="dd-menu" role="listbox">
          {options.map((o, i) => [
            o.group && o.group !== options[i - 1]?.group && (
              <div key={`g:${o.group}`} className="dd-group" role="presentation">
                {o.group}
              </div>
            ),
            <button
              key={String(o.value)}
              type="button"
              role="option"
              aria-selected={o.value === value}
              className={`${o.value === value ? 'on' : ''} ${i === hl ? 'hl' : ''}`}
              onMouseEnter={() => setHl(i)}
              onClick={() => {
                onChange(o.value)
                setOpen(false)
              }}
            >
              {o.label}
              {o.value === value && <Check size={14} />}
            </button>
          ])}
        </div>
      )}
    </div>
  )
}

/** La lista se abre hacia arriba si abajo no cabe dentro de lo que la recortaría (un panel con scroll o la ventana) */
function opensUp(el: HTMLElement, items: number): boolean {
  const r = el.getBoundingClientRect()
  let top = 0
  let bottom = window.innerHeight
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll|hidden)/.test(getComputedStyle(p).overflowY)) {
      const b = p.getBoundingClientRect()
      top = Math.max(top, b.top)
      bottom = Math.min(bottom, b.bottom)
      break
    }
  }
  const need = Math.min(260, items * 32 + 12) + 10
  return bottom - r.bottom < need && r.top - top > bottom - r.bottom
}

/** Grabador de atajos: se pulsa y se teclea la combinación. Esc cancela y Supr o Retroceso lo quitan. Rust la
 *  valida (sin modificador, AltGr…) y la devuelve en su forma canónica; `taken` son las de los demás atajos. */
export function KeyRecorder({
  value,
  taken,
  failed,
  onChange,
  label
}: {
  value: string | null
  taken: string[]
  /** Windows no la ha dejado registrar (la tiene otro programa) */
  failed?: boolean
  onChange: (keys: string | null) => void
  label: string
}): React.JSX.Element {
  const t = useT()
  const [rec, setRec] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const root = useRef<HTMLDivElement>(null)
  // Lo último de las props, sin volver a montar los escuchadores mientras se graba
  const latest = useRef({ taken, onChange })
  latest.current = { taken, onChange }

  useEffect(() => {
    if (!rec) return
    // Con los atajos registrados, Windows se quedaría la pulsación antes de llegar aquí
    void api.hotkeysPause(true)
    const stop = (): void => setRec(false)
    const onKey = (e: KeyboardEvent): void => {
      e.preventDefault()
      e.stopPropagation()
      if (e.repeat) return
      const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey
      if (plain && e.key === 'Escape') return stop()
      if (plain && (e.key === 'Delete' || e.key === 'Backspace')) {
        setError(null)
        latest.current.onChange(null)
        return stop()
      }
      const combo = comboFromEvent(e)
      if (!combo) return
      api
        .hotkeyCheck(combo)
        .then((keys) => {
          if (latest.current.taken.includes(keys)) return setError(t('keys.taken'))
          setError(null)
          latest.current.onChange(keys)
          stop()
        })
        .catch((err: unknown) => setError(errorText(err)))
    }
    const onDown = (e: MouseEvent): void => {
      if (!root.current?.contains(e.target as Node)) stop()
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('mousedown', onDown)
    window.addEventListener('blur', stop)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('blur', stop)
      void api.hotkeysPause(false)
    }
  }, [rec, t])

  return (
    <div ref={root} className="key-rec-wrap">
      <button
        type="button"
        className={`key-rec tip-right ${rec ? 'rec' : ''} ${failed && !rec ? 'bad' : ''} ${value ? '' : 'empty'}`}
        aria-label={`${label}: ${value ? keyLabel(value) : t('keys.noKey')}`}
        aria-pressed={rec}
        data-tip={failed && !rec ? t('keys.failedTip') : t('keys.change')}
        onClick={() => {
          setError(null)
          setRec(!rec)
        }}
      >
        {failed && !rec && <TriangleAlert size={13} />}
        <span className="num">{rec ? t('keys.press') : value ? keyLabel(value) : t('keys.noKey')}</span>
      </button>
      {error && (
        <small className="key-rec-err" role="alert">
          {error}
        </small>
      )}
    </div>
  )
}

/** Deslizador con la parte recorrida pintada del color de la app */
export function Range({
  value,
  min,
  max,
  step,
  onChange,
  label
}: {
  value: number
  min: number
  max: number
  step?: number
  onChange: (v: number) => void
  label?: string
}): React.JSX.Element {
  const p = `${((value - min) / (max - min)) * 100}%`
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      aria-label={label}
      style={{ '--p': p } as React.CSSProperties}
      onChange={(e) => onChange(+e.target.value)}
    />
  )
}
