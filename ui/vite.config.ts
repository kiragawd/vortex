import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: '../assets',
    emptyOutDir: true,
  },
  server: {
    // Dev server port (default for local dev). When running the backend locally
    // the API is expected on port 8080; allow overriding via BACKEND_URL env var.
    port: 3001,
    proxy: {
      '/api': {
        target: process.env.BACKEND_URL || 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
})
