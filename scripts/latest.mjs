// Genera deploy-hosting/latest.json para el auto-update (tauri-plugin-updater) a partir de la versión, la firma
// del instalador y las novedades en español del CHANGELOG. Uso: node scripts/latest.mjs <carpeta de salida>
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const out = process.argv[2] ?? 'deploy-hosting'
const root = new URL('..', import.meta.url)
const { version } = JSON.parse(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8'))
const setup = `PoxiEQ-Setup-${version}.exe`
const signature = readFileSync(join(out, `${setup}.sig`), 'utf8').trim()

const changelog = readFileSync(new URL('CHANGELOG.md', root), 'utf8').replace(/\r\n/g, '\n')
/** Desde `from` hasta `to` (o hasta el final si no aparece) */
const between = (text, from, to) => {
  const a = text.indexOf(from)
  if (a < 0) return ''
  const b = text.indexOf(to, a + from.length)
  return text.slice(a, b < 0 ? undefined : b)
}
const es = between(between(changelog, `## ${version}\n`, '\n## '), '### 🇪🇸', '### 🇬🇧')
const notes = es
  .split('\n')
  .filter((l) => l.startsWith('- '))
  .map((l) => l.replace(/\*\*(.+?)\*\*/g, '$1'))
  .join('\n')

const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      signature,
      url: `https://github.com/PoxiiTV/PoxiEQ/releases/download/v${version}/${setup}`
    }
  }
}
writeFileSync(join(out, 'latest.json'), JSON.stringify(latest, null, 2) + '\n')
console.log(`latest.json -> ${version}`)
