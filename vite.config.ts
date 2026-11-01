import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri espera un puerto fijo y no debe tapar los errores de Rust en la consola
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  // Dos páginas: la app y el aviso en pantalla (ventana `osd`, ligera: no carga React)
  build: { target: 'es2022', chunkSizeWarningLimit: 1500, rollupOptions: { input: { main: 'index.html', osd: 'osd.html', mini: 'mini.html' } } }
})
