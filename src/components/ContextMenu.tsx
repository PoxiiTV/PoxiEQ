import { useEffect, useState } from 'react'
import { ClipboardPaste, Copy, Scissors, TextSelect } from 'lucide-react'
import { useT } from '../i18n'

type Field = HTMLInputElement | HTMLTextAreaElement

/** Escribe en un campo como si lo hubiera tecleado la persona (React se entera del cambio) */
function insert(el: Field, text: string): void {
  el.focus()
  el.setRangeText(text, el.selectionStart ?? el.value.length, el.selectionEnd ?? el.value.length, 'end')
  el.dispatchEvent(new Event('input', { bubbles: true }))
}

/** Menú contextual propio para los campos de texto (el del navegador está desactivado en toda la app). */
export function ContextMenu(): React.JSX.Element | null {
  const t = useT()
  const [menu, setMenu] = useState<{ x: number; y: number; el: Field } | null>(null)

  useEffect(() => {
    const open = (e: MouseEvent): void => {
      const el = (e.target as HTMLElement).closest('input, textarea') as Field | null
      if (!el || (el instanceof HTMLInputElement && ['checkbox', 'range', 'button'].includes(el.type))) return setMenu(null)
      const w = 190
      const h = 168
      setMenu({ el, x: Math.min(e.clientX, window.innerWidth - w - 8), y: Math.min(e.clientY, window.innerHeight - h - 8) })
    }
    const close = (): void => setMenu(null)
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') close()
    }
    const onDown = (e: MouseEvent): void => {
      if (!(e.target as HTMLElement).closest('.ctx')) close()
    }
    window.addEventListener('contextmenu', open)
    window.addEventListener('mousedown', onDown)
    window.addEventListener('blur', close)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('contextmenu', open)
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('blur', close)
      window.removeEventListener('keydown', onKey)
    }
  }, [])

  if (!menu) return null
  const { el } = menu
  const selected = (el.selectionEnd ?? 0) > (el.selectionStart ?? 0)
  const secret = el instanceof HTMLInputElement && el.type === 'password'
  const editable = !el.readOnly && !el.disabled
  const run = (fn: () => void | Promise<void>) => async () => {
    setMenu(null)
    await fn()
  }
  const items: [string, React.ReactNode, boolean, () => void | Promise<void>][] = [
    [t('ctx.cut'), <Scissors size={15} key="i" />, selected && !secret && editable, () => void document.execCommand('cut')],
    [t('ctx.copy'), <Copy size={15} key="i" />, selected && !secret, () => void document.execCommand('copy')],
    [t('ctx.paste'), <ClipboardPaste size={15} key="i" />, editable, async () => insert(el, await navigator.clipboard.readText())],
    [t('ctx.selectAll'), <TextSelect size={15} key="i" />, !!el.value, () => (el.focus(), el.select())]
  ]
  return (
    <div className="dd-menu ctx" style={{ position: 'fixed', left: menu.x, top: menu.y, right: 'auto', minWidth: 190, transformOrigin: 'top left' }} role="menu">
      {items.map(([label, icon, on, fn]) => (
        <button key={label} role="menuitem" disabled={!on} onMouseDown={(e) => e.preventDefault()} onClick={run(fn)}>
          <span className="row" style={{ gap: 10 }}>
            {icon}
            {label}
          </span>
        </button>
      ))}
    </div>
  )
}
