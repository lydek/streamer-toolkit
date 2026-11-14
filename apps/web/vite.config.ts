import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import VueI18nPlugin from '@intlify/unplugin-vue-i18n/vite'
import { devDataDir, devServer, hostProxy } from './dev-server.ts'

export default defineConfig(({ mode }) => ({
  // Locale JSON is precompiled at build time; the runtime-only vue-i18n build never needs eval.
  plugins: [vue(), VueI18nPlugin({ include: resolve(import.meta.dirname, 'src/locales/**') }), devServer()],
  server: {
    port: 5173,
    strictPort: true,
    // Replaced by devServer(): only the null origin, and never on /api, /ws or /templates.
    cors: false,
    proxy: hostProxy(devDataDir()),
    fs: {
      // Vite's defaults (this list replaces them; dev-server.test.ts keeps it a superset) plus Cargo's target folder:
      // the dev data folder (settings, logs, WebView2 profile) is inside it, and the null origin may read what Vite serves.
      deny: ['.env', '.env.*', '*.{crt,pem,key,p12,pfx,cer,der}', '.npmrc', '.yarnrc.yml', '**/.git/**', '**/target/**'],
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
