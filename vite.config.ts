import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri espera un puerto fijo y no debe tapar los errores de Rust en la consola
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: 'es2022', chunkSizeWarningLimit: 1500 }
})
