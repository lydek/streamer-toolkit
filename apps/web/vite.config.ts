import { resolve } from 'node:path'
import { type Plugin, defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import VueI18nPlugin from '@intlify/unplugin-vue-i18n/vite'

// Must match the Host's default port (docs/architecture.md §5).
const HOST = 'http://127.0.0.1:47810'
// Must match OVERLAY_CSP in crates/host/src/lib.rs.
const OVERLAY_CSP = "script-src 'self'; object-src 'none'"

/** Dev only: overlay pages get the Overlay CSP the Host sends, so violations show up during development. */
const overlayCsp: Plugin = {
  name: 'overlay-csp',
  configureServer(server) {
    server.middlewares.use((req, res, next) => {
      if (req.url?.startsWith('/overlay/')) res.setHeader('Content-Security-Policy', OVERLAY_CSP)
      next()
    })
  },
}

export default defineConfig(({ mode }) => ({
  // Locale JSON is precompiled at build time; the runtime-only vue-i18n build never needs eval.
  plugins: [vue(), VueI18nPlugin({ include: resolve(import.meta.dirname, 'src/locales/**') }), overlayCsp],
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
