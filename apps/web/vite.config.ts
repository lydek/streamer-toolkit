import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import VueI18nPlugin from '@intlify/unplugin-vue-i18n/vite'
import { devDataDir, devServer, hostProxy } from './dev-server.ts'

export default defineConfig(({ mode }) => ({
  // Locale JSON is precompiled at build time; the runtime-only vue-i18n build never needs eval.
  // public/ holds only the release-time THIRD_PARTY_LICENSES.txt (scripts/third-party-licenses.mjs); the port page needs none.
  publicDir: mode === 'port' ? false : 'public',
  plugins: [vue(), VueI18nPlugin({ include: resolve(import.meta.dirname, 'src/locales/**') }), devServer()],
  server: {
    port: 5173,
    strictPort: true,
    // Replaced by devServer(): only the null origin, and never on /api, /ws, /templates or /sounds.
    cors: false,
    proxy: hostProxy(devDataDir()),
    fs: {
      // Vite's defaults (this list replaces them; dev-server.test.ts keeps it a superset) plus Cargo's target folder:
      // the dev data folder (settings, logs, WebView2 profile) is inside it, and the null origin may read what Vite serves.
      deny: ['.env', '.env.*', '*.{crt,pem,key,p12,pfx,cer,der}', '.npmrc', '.yarnrc.yml', '**/.git/**', '**/target/**'],
    },
  },
  // Every dev page, incl. the E2E harness: with build inputs set, Vite would scan only those at startup and reload
  // all open pages when another page brings a new dependency ("optimized dependencies changed").
  optimizeDeps: {
    entries: [
      'admin/index.html',
      'overlay/chat/index.html',
      'overlay/alerts/index.html',
      'overlay/checkin/index.html',
      'overlay/sound/index.html',
      'overlay/clip/index.html',
      'port/index.html',
      'e2e/harness/index.html',
    ],
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
              'overlay-alerts': resolve(import.meta.dirname, 'overlay/alerts/index.html'),
              'overlay-checkin': resolve(import.meta.dirname, 'overlay/checkin/index.html'),
              'overlay-sound': resolve(import.meta.dirname, 'overlay/sound/index.html'),
              'overlay-clip': resolve(import.meta.dirname, 'overlay/clip/index.html'),
            },
          },
        },
}))
