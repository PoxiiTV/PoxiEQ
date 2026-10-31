// Genera .assets/icon.svg: la «P» pixelada de la suite (misma rejilla que PoxiPass) en fucsia, con la sombra
// en escalones y un mini ecualizador dorado abajo a la derecha.
// Uso: node scripts/icon.mjs  (después: npx tauri icon .assets/icon.svg -o src-tauri/icons)
import { writeFileSync } from 'node:fs'

const B = 4 // tamaño del píxel
const P = ['#####.', '##..##', '##..##', '#####.', '##....', '##....', '##....']
const rects = (map, x0, y0) =>
  map.flatMap((row, y) => [...row].map((c, x) => (c === '#' ? [x0 + x * B, y0 + y * B, B, B] : null)).filter(Boolean))
const letter = rects(P, 0, 0)
// Barras del ecualizador (x, alto), con la base en y = 30
const bars = [[20, 7], [23, 3], [26, 5]].map(([x, h]) => [x, 30 - h, 2, h])

const path = (list, d) => list.map(([x, y, w, h]) => `M${x + d} ${y + d}h${w}v${h}h-${w}z`).join('')
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-7 -5 40 40" shape-rendering="crispEdges">\
<rect x="-7" y="-5" width="40" height="40" rx="7" fill="#170c19"/>\
<path fill="#4d2852" d="${path(letter, 2)}"/>\
<path fill="#8b4995" d="${path(letter, 1)}"/>\
<path fill="#e879f9" d="${path(letter, 0)}"/>\
<path fill="#997e39" d="${path(bars, 1)}"/>\
<path fill="#ffd25f" d="${path(bars, 0)}"/></svg>`
writeFileSync(new URL('../.assets/icon.svg', import.meta.url), svg)
console.log('ok')
