import { useEffect, useRef } from 'react'
import { listen } from '@tauri-apps/api/event'

const BANDS = 96
// Frecuencia central de cada banda (igual reparto que en Rust: 20 Hz → 20 kHz logarítmico)
const CENTERS = Array.from({ length: BANDS }, (_, i) => 20 * 1000 ** ((i + 0.5) / BANDS))
const FLOOR = -84
const CEIL = -6

/** Espectro de lo que suena, detrás de la curva. Sube rápido y cae despacio, como un medidor de verdad. */
export function Analyzer({
  x,
  top,
  height,
  width,
  fullHeight
}: {
  x: (f: number) => number
  top: number
  height: number
  width: number
  fullHeight: number
}): React.JSX.Element {
  const canvas = useRef<HTMLCanvasElement>(null)
  const level = useRef(new Float32Array(BANDS).fill(FLOOR))
  const geo = useRef({ x, top, height })
  geo.current = { x, top, height }

  useEffect(() => {
    let frame = 0
    const draw = (): void => {
      frame = 0
      const c = canvas.current
      const ctx = c?.getContext('2d')
      if (!c || !ctx) return
      const { x, top, height } = geo.current
      const dpr = window.devicePixelRatio || 1
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, c.width, c.height)
      const accent = getComputedStyle(c).getPropertyValue('--accent').trim() || '#06b6d4'
      const grad = ctx.createLinearGradient(0, top, 0, top + height)
      grad.addColorStop(0, `${accent}55`)
      grad.addColorStop(1, `${accent}05`)
      ctx.fillStyle = grad
      ctx.beginPath()
      ctx.moveTo(x(CENTERS[0]), top + height)
      level.current.forEach((db, i) => {
        const k = Math.min(1, Math.max(0, (db - FLOOR) / (CEIL - FLOOR)))
        ctx.lineTo(x(CENTERS[i]), top + height * (1 - k))
      })
      ctx.lineTo(x(CENTERS[BANDS - 1]), top + height)
      ctx.closePath()
      ctx.fill()
    }
    const off = listen<number[]>('spectrum', (e) => {
      const l = level.current
      e.payload.forEach((db, i) => {
        l[i] = db > l[i] ? l[i] + (db - l[i]) * 0.6 : l[i] + (db - l[i]) * 0.18
      })
      if (!frame) frame = requestAnimationFrame(draw)
    })
    return () => {
      void off.then((f) => f())
      cancelAnimationFrame(frame)
    }
  }, [])

  const dpr = window.devicePixelRatio || 1
  return <canvas ref={canvas} className="analyzer" width={width * dpr} height={fullHeight * dpr} style={{ width, height: fullHeight }} />
}
