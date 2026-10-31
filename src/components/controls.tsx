import { useEffect, useRef, useState } from 'react'
import { ChevronDown, Check } from 'lucide-react'

export interface Option<T extends string | number> {
  value: T
  label: string
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
    <div ref={root} className={`dd ${open ? 'open' : ''} ${size ?? ''} ${block ? 'block' : ''}`}>
      <button type="button" className="dd-btn" aria-haspopup="listbox" aria-expanded={open} aria-label={label} onClick={() => setOpen(!open)}>
        <span>{current?.label ?? ''}</span>
        <ChevronDown size={15} />
      </button>
      {open && (
        <div className="dd-menu" role="listbox">
          {options.map((o, i) => (
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
          ))}
        </div>
      )}
    </div>
  )
}

/** Deslizador con la parte recorrida pintada del color de la app */
export function Range({ value, min, max, onChange, label }: { value: number; min: number; max: number; onChange: (v: number) => void; label?: string }): React.JSX.Element {
  const p = `${((value - min) / (max - min)) * 100}%`
  return (
    <input
      type="range"
      min={min}
      max={max}
      value={value}
      aria-label={label}
      style={{ '--p': p } as React.CSSProperties}
      onChange={(e) => onChange(+e.target.value)}
    />
  )
}
