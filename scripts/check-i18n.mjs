// Comprueba los textos: cada código de error que puede mandar Rust tiene su mensaje en español e inglés, y no
// falta en inglés ninguna clave del español. Uso: node scripts/check-i18n.mjs (sale con error si falta algo).
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import es from '../src/i18n/es.ts'
import en from '../src/i18n/en.ts'

const dir = fileURLToPath(new URL('../src-tauri/src/', import.meta.url))
const files = readdirSync(dir, { recursive: true }).filter((f) => String(f).endsWith('.rs'))
const codes = new Set()
for (const f of files) {
  const src = readFileSync(join(dir, String(f)), 'utf8')
  for (const m of src.matchAll(/problem\("([a-zA-Z]+\.[a-zA-Z.]+)"/g)) codes.add(m[1])
}

const keys = (o, p = '') =>
  Object.entries(o).flatMap(([k, v]) => (v && typeof v === 'object' && !Array.isArray(v) && p !== 'err.' ? keys(v, `${p}${k}.`) : [`${p}${k}`]))
const enKeys = new Set(keys(en))
const problems = [
  ...[...codes].filter((c) => !es.err[c]).map((c) => `es: falta err «${c}»`),
  ...[...codes].filter((c) => !en.err?.[c]).map((c) => `en: falta err «${c}»`),
  ...keys(es).filter((k) => !enKeys.has(k)).map((k) => `en: falta «${k}»`)
]
if (problems.length) {
  console.error(problems.join('\n'))
  process.exit(1)
}
console.log(`i18n OK: ${codes.size} códigos de error de Rust y ${enKeys.size} textos en inglés`)
