import { useEffect, useRef } from 'react'
import { listen } from '@tauri-apps/api/event'
import type { AnalyzerMode } from '../lib/api'

const BANDS = 96
// Frecuencia central de cada banda (igual reparto que en Rust: 20 Hz → 20 kHz logarítmico)
const CENTERS = Array.from({ length: BANDS }, (_, i) => 20 * 1000 ** ((i + 0.5) / BANDS))
const FLOOR = -84
const CEIL = -6
/** Retención de picos: cuánto se queda quieto el pico (ms) y a qué ritmo cae luego (dB/s) */
const HOLD = 900
const FALL = 12
/** Espectrograma: píxeles CSS que baja cada fotograma (~30 por segundo: unos 6 s de historia en la curva) */
const ROW = 2

const level01 = (db: number): number => Math.min(1, Math.max(0, (db - FLOOR) / (CEIL - FLOOR)))

function rgb(color: string): [number, number, number] {
  const c = color.trim()
  if (c.startsWith('#') && c.length === 7) return [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as [number, number, number]
  const parts = c.split(',').map((v) => parseInt(v, 10))
  return parts.length === 3 && parts.every(Number.isFinite) ? (parts as [number, number, number]) : [6, 182, 212]
}

/** Paleta del espectrograma con el color de la app: lo flojo transparente, lo fuerte hacia el segundo tono y blanco */
function palette(a: [number, number, number], b: [number, number, number]): Uint8ClampedArray {
  const lut = new Uint8ClampedArray(256 * 4)
  const mix = (p: number[], q: number[], t: number): number[] => p.map((v, i) => v + (q[i] - v) * t)
  for (let i = 0; i < 256; i++) {
    const k = i / 255
    const s = Math.min(1, Math.max(0, (k - 0.1) / 0.65))
    const alpha = s * s * (3 - 2 * s) * 0.92
    const c = k < 0.55 ? a : k < 0.85 ? mix(a, b, (k - 0.55) / 0.3) : mix(b, [255, 255, 255], ((k - 0.85) / 0.15) * 0.7)
    lut.set([c[0], c[1], c[2], alpha * 255], i * 4)
  }
  return lut
}

/**
 * Lo que suena, detrás de la curva. Espectro: sube rápido y cae despacio, como un medidor de verdad, con una
 * línea de picos que se queda un momento y baja poco a poco. Espectrograma: el tiempo cae hacia abajo.
 */
export function Analyzer({
  x,
  top,
  height,
  width,
  fullHeight,
  mode
}: {
  x: (f: number) => number
  top: number
  height: number
  width: number
  fullHeight: number
  mode: AnalyzerMode
}): React.JSX.Element {
  const canvas = useRef<HTMLCanvasElement>(null)
  const geo = useRef({ x, top, height })
  geo.current = { x, top, height }

  useEffect(() => {
    const level = new Float32Array(BANDS).fill(FLOOR)
    const peak = new Float32Array(BANDS).fill(FLOOR)
    const holdUntil = new Float64Array(BANDS)
    let last = performance.now()
    let frame = 0
    // Espectrograma: banda (fraccionaria) de cada columna de píxeles y paleta, recalculadas solo si cambian
    let cols = new Float32Array(0)
    let colsKey = ''
    let row: ImageData | null = null
    let lut: Uint8ClampedArray = new Uint8ClampedArray(0)
    let lutKey = ''

    const context = (): [HTMLCanvasElement, CanvasRenderingContext2D] | null => {
      const c = canvas.current
      const ctx = c?.getContext('2d')
      return c && ctx ? [c, ctx] : null
    }

    const drawSpectrum = (): void => {
      frame = 0
      const cc = context()
      if (!cc) return
      const [c, ctx] = cc
      const { x, top, height } = geo.current
      const dpr = window.devicePixelRatio || 1
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, c.width, c.height)
      const style = getComputedStyle(c)
      const accent = style.getPropertyValue('--accent').trim() || '#06b6d4'
      const grad = ctx.createLinearGradient(0, top, 0, top + height)
      grad.addColorStop(0, `${accent}55`)
      grad.addColorStop(1, `${accent}05`)
      ctx.fillStyle = grad
      ctx.beginPath()
      ctx.moveTo(x(CENTERS[0]), top + height)
      level.forEach((db, i) => ctx.lineTo(x(CENTERS[i]), top + height * (1 - level01(db))))
      ctx.lineTo(x(CENTERS[BANDS - 1]), top + height)
      ctx.closePath()
      ctx.fill()
      // Picos: la línea se corta donde no hay nada que retener (no se pinta una raya por el suelo)
      ctx.beginPath()
      let drawing = false
      peak.forEach((db, i) => {
        if (db <= FLOOR + 1) return void (drawing = false)
        const px = x(CENTERS[i])
        const py = top + height * (1 - level01(db))
        if (drawing) ctx.lineTo(px, py)
        else ctx.moveTo(px, py)
        drawing = true
      })
      ctx.strokeStyle = style.getPropertyValue('--accent-2').trim() || '#67e8f9'
      ctx.globalAlpha = 0.85
      ctx.lineWidth = 1.25
      ctx.lineJoin = 'round'
      ctx.stroke()
      ctx.globalAlpha = 1
    }

    const drawGram = (db: number[]): void => {
      const cc = context()
      if (!cc) return
      const [c, ctx] = cc
      const { x, top, height } = geo.current
      const dpr = window.devicePixelRatio || 1
      const w = c.width
      const y0 = Math.round(top * dpr)
      const h = Math.round(height * dpr)
      const step = Math.max(1, Math.round(ROW * dpr))
      if (w <= 0 || h <= step) return
      // Las bandas están repartidas en log igual que el eje de la curva: columna → banda es lineal
      const x0 = x(CENTERS[0])
      const x1 = x(CENTERS[BANDS - 1])
      const key = `${w}:${dpr}:${x0}:${x1}`
      if (key !== colsKey) {
        colsKey = key
        cols = new Float32Array(w)
        for (let px = 0; px < w; px++) {
          const u = (((px + 0.5) / dpr - x0) / (x1 - x0)) * (BANDS - 1)
          cols[px] = u < -0.5 || u > BANDS - 0.5 ? -1 : Math.min(BANDS - 1, Math.max(0, u))
        }
        row = ctx.createImageData(w, 1)
      }
      const style = getComputedStyle(c)
      const colors = `${style.getPropertyValue('--accent-rgb')}|${style.getPropertyValue('--accent-2')}`
      if (colors !== lutKey) {
        lutKey = colors
        lut = palette(rgb(style.getPropertyValue('--accent-rgb')), rgb(style.getPropertyValue('--accent-2')))
      }
      const d = row!.data
      for (let px = 0; px < w; px++) {
        const u = cols[px]
        if (u < 0) {
          d[px * 4 + 3] = 0
          continue
        }
        const i = Math.floor(u)
        const f = u - i
        const v = db[i] * (1 - f) + db[Math.min(BANDS - 1, i + 1)] * f
        const li = Math.round(level01(v) * 255) * 4
        d[px * 4] = lut[li]
        d[px * 4 + 1] = lut[li + 1]
        d[px * 4 + 2] = lut[li + 2]
        d[px * 4 + 3] = lut[li + 3]
      }
      // Lo de antes baja `step` píxeles (copia del propio lienzo, en la GPU) y arriba entra la fila nueva
      ctx.setTransform(1, 0, 0, 1, 0, 0)
      ctx.drawImage(c, 0, y0, w, h - step, 0, y0 + step, w, h - step)
      for (let r = 0; r < step; r++) ctx.putImageData(row!, 0, y0 + r)
    }

    const off = listen<number[]>('spectrum', (e) => {
      if (e.payload.length !== BANDS) return
      if (mode === 'spectrogram') return drawGram(e.payload)
      const now = performance.now()
      const dt = Math.min(0.1, (now - last) / 1000)
      last = now
      e.payload.forEach((db, i) => {
        level[i] = db > level[i] ? level[i] + (db - level[i]) * 0.6 : level[i] + (db - level[i]) * 0.18
        if (db >= peak[i]) {
          peak[i] = db
          holdUntil[i] = now + HOLD
        } else if (now > holdUntil[i]) {
          peak[i] = Math.max(FLOOR, peak[i] - FALL * dt)
        }
      })
      if (!frame) frame = requestAnimationFrame(drawSpectrum)
    })
    return () => {
      void off.then((f) => f())
      cancelAnimationFrame(frame)
    }
  }, [mode])

  const dpr = window.devicePixelRatio || 1
  return <canvas ref={canvas} className="analyzer" width={width * dpr} height={fullHeight * dpr} style={{ width, height: fullHeight }} />
}
