import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/api': { target: 'https://127.0.0.1:8800', secure: false, changeOrigin: true },
      '/ws': { target: 'wss://127.0.0.1:8800', ws: true, secure: false },
    },
  },
})
