import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Relative base so the built site works at a root domain or under a path.
export default defineConfig({
  plugins: [react()],
  base: './',
  build: { outDir: 'dist', sourcemap: false },
})
