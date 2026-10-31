// Atajos de teclado: la combinación que se graba en Ajustes, en el formato del registrador de Rust
// (`control+alt+Numpad0`, con `KeyboardEvent.code`: la tecla física, igual en cualquier distribución), y su
// texto para enseñar. Rust la valida (AltGr, sin modificador…) y la deja en su forma canónica.

type KeyInfo = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>

/** Combinación de una pulsación, o `null` si solo se ha pulsado un modificador (aún no está completa) */
export function comboFromEvent(e: KeyInfo): string | null {
  if (!e.code || /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(e.code)) return null
  const mods = [e.shiftKey && 'shift', e.ctrlKey && 'control', e.altKey && 'alt', e.metaKey && 'super'].filter(Boolean)
  return [...mods, e.code].join('+')
}

const MODS: Record<string, string> = { control: 'Ctrl', alt: 'Alt', shift: 'Shift', super: 'Win' }
const NUMPAD: Record<string, string> = { Add: '+', Subtract: '−', Multiply: '*', Divide: '/', Decimal: '.', Enter: 'Enter', Equal: '=' }
const KEYS: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Backquote: 'º',
  Minus: "'",
  Equal: '¡',
  BracketLeft: '`',
  BracketRight: '+',
  Backslash: 'ç',
  Semicolon: 'ñ',
  Quote: '´',
  Comma: ',',
  Period: '.',
  Slash: '-',
  PageUp: 'RePág',
  PageDown: 'AvPág'
}

/** Texto corto: `control+alt+Numpad0` → `Ctrl+Alt+Num 0` (las teclas de símbolos, como en un teclado español) */
export function keyLabel(keys: string): string {
  return keys
    .split('+')
    .map((k) => {
      if (MODS[k]) return MODS[k]
      if (k.startsWith('Numpad')) {
        const d = k.slice(6)
        return `Num ${NUMPAD[d] ?? d}`
      }
      if (/^(Key|Digit)./.test(k)) return k.replace(/^(Key|Digit)/, '')
      return KEYS[k] ?? k
    })
    .join('+')
}
