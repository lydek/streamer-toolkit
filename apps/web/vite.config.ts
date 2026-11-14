import { resolve } from 'node:path'
import { type Plugin, defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import VueI18nPlugin from '@intlify/unplugin-vue-i18n/vite'
import { ADMIN_CSP, devDataDir, devServer, hostProxy } from './dev-server'

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
  plugins: [vue(), VueI18nPlugin({ include: resolve(import.meta.dirname, 'src/locales/**') }), overlayCsp, devServer()],
  server: {
    port: 5173,
    strictPort: true,
    // Same CSP as the Host's /admin, so violations show up while developing.
    headers: { 'Content-Security-Policy': ADMIN_CSP },
    // Replaced by devServer(): only the null origin, and never on /api or /ws.
    cors: false,
    proxy: hostProxy(devDataDir()),
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
