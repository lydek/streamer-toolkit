import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Must match the Host's default port (docs/architecture.md §5).
const HOST = 'http://127.0.0.1:47810'

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: HOST, changeOrigin: true },
      '/ws': { target: HOST, changeOrigin: true, ws: true },
    },
  },
  build: {
    rollupOptions: {
      input: {
        admin: resolve(import.meta.dirname, 'admin/index.html'),
        'overlay-chat': resolve(import.meta.dirname, 'overlay/chat/index.html'),
      },
    },
  },
})
