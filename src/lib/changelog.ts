// Lee del CHANGELOG.md (empaquetado en la app) las novedades por versión y en el idioma pedido.
// Mismo formato y parser que PoxiLauncher.
import raw from '../../CHANGELOG.md?raw'

export interface ChangeItem {
  emoji: string
  title: string
  text: string
}

const EMOJI = /^(\p{Extended_Pictographic}(️|‍\p{Extended_Pictographic})*)\s*/u
const plain = (t: string): string => t.replace(/\*\*(.+?)\*\*/g, '$1').replace(/`([^`]+)`/g, '$1')

export function changesFor(version: string, lang: 'es' | 'en'): ChangeItem[] {
  const text = raw.replace(/\r\n/g, '\n')
  const start = text.indexOf(`## ${version}\n`)
  if (start < 0) return []
  const next = text.indexOf('\n## ', start + 4)
  const section = text.slice(start, next < 0 ? undefined : next)
  const langStart = section.indexOf(lang === 'es' ? '### 🇪🇸' : '### 🇬🇧')
  if (langStart < 0) return []
  const langEnd = section.indexOf('\n### ', langStart + 4)
  return section
    .slice(langStart, langEnd < 0 ? undefined : langEnd)
    .split('\n')
    .filter((l) => l.startsWith('- '))
    .map((l) => {
      let line = l.slice(2).trim()
      const e = EMOJI.exec(line)
      const emoji = e ? e[1] : '✨'
      if (e) line = line.slice(e[0].length)
      const m = /^\*\*(.+?)\*\*[:.]?\s*(.*)$/.exec(line)
      const body = plain(m?.[2] ?? '').replace(/^[\s,;:.–-]+/, '')
      return m ? { emoji, title: m[1], text: body.charAt(0).toUpperCase() + body.slice(1) } : { emoji, title: plain(line), text: '' }
    })
}
