import { useEffect, useRef, type ReactNode } from 'react'
import logoUrl from '../assets/logo.svg'

export function Modal({
  children,
  onClose,
  wide,
  className = ''
}: {
  children: ReactNode
  onClose?: () => void
  wide?: boolean
  className?: string
}): React.JSX.Element {
  useEffect(() => {
    if (!onClose) return
    const onKey = (e: KeyboardEvent): void => {
      // Un desplegable o menú abierto se cierra él solo primero
      if (e.key === 'Escape' && !document.querySelector('.dd.open, .ctx')) {
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [onClose])
  const box = useRef<HTMLDivElement>(null)
  // Tab no se escapa del modal, y al cerrarlo el foco vuelve a donde estaba
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null
    if (!box.current?.contains(document.activeElement)) box.current?.querySelector<HTMLElement>('[autofocus], input, button')?.focus()
    const trap = (e: KeyboardEvent): void => {
      if (e.key !== 'Tab' || !box.current) return
      const all = [...box.current.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), textarea, [tabindex]:not([tabindex="-1"])')]
      if (!all.length) return
      const [first, last] = [all[0], all[all.length - 1]]
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', trap)
    return () => {
      window.removeEventListener('keydown', trap)
      before?.focus?.()
    }
  }, [])
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div ref={box} className={`modal ${wide ? 'wide' : ''} ${className}`} role="dialog" aria-modal="true">
        {children}
      </div>
    </div>
  )
}

export function Switch({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label?: string }): React.JSX.Element {
  return <button type="button" role="switch" aria-checked={on} aria-label={label} className={`switch ${on ? 'on' : ''}`} onClick={() => onChange(!on)} />
}

export function Logo({ size = 22 }: { size?: number }): React.JSX.Element {
  return <img src={logoUrl} width={size} height={size} alt="" draggable={false} />
}
