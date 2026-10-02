import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served from a domain root, which is what Vercel's Vite preset does. An absolute base keeps the
// self hosted font at one path that both the stylesheet and the preload hint agree on.
export default defineConfig({
  plugins: [react()],
  base: '/',
  build: { outDir: 'dist', sourcemap: false },
})
