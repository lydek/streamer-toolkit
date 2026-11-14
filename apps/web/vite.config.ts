import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import VueI18nPlugin from '@intlify/unplugin-vue-i18n/vite'

// Must match the Host's default port (docs/architecture.md §5).
const HOST = 'http://127.0.0.1:47810'

export default defineConfig(({ mode }) => ({
  // Locale JSON is precompiled at build time; the runtime-only vue-i18n build never needs eval.
  plugins: [vue(), VueI18nPlugin({ include: resolve(import.meta.dirname, 'src/locales/**') })],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: HOST, changeOrigin: true },
      '/ws': { target: HOST, changeOrigin: true, ws: true },
      '/templates': { target: HOST, changeOrigin: true },
    },
  },
  // `--mode port` builds only the small port-change page into Tauri's frontendDist.
  build:
    mode === 'port'
      ? { outDir: 'dist-port', rollupOptions: { input: resolve(import.meta.dirname, 'port/index.html') } }
      : {
          rollupOptions: {
            input: {
              admin: resolve(import.meta.dirname, 'admin/index.html'),
              'overlay-chat': resolve(import.meta.dirname, 'overlay/chat/index.html'),
            },
          },
        },
}))
