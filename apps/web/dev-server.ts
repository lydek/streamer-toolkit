// Vite dev server pieces (docs/architecture.md §5 開發模式). Node-only; never part of the browser bundle.
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { Plugin, ProxyOptions } from 'vite'

// Must match crates/host/src/lib.rs (dev-server.test.ts checks it).
export const ADMIN_CSP =
  "script-src 'self'; connect-src 'self' ipc: http://ipc.localhost; " +
  "style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; frame-src 'none'"

const DEFAULT_PORT = 47810
// Vite's proxy matches these as plain prefixes, so the null-origin rule uses the same test.
const HOST_PREFIXES = ['/api', '/ws']
const isHostPath = (url = '') => HOST_PREFIXES.some((p) => url.startsWith(p))

/** `tauri dev` runs the debug build, whose data folder sits next to the executable. */
// shortcut: ignores `--data-dir`, and CARGO_TARGET_DIR must be absolute (cargo resolves a relative one from its
// own working directory, not apps/web); pass the folder here if dev runs ever need either.
export const devDataDir = () =>
  resolve(process.env.CARGO_TARGET_DIR ?? resolve(import.meta.dirname, '../../target'), 'debug', 'data')

/** The port saved in the data folder's settings.json; the Host's default when there is none yet. */
export function hostPort(dataDir: string): number {
  try {
    const { port } = JSON.parse(readFileSync(resolve(dataDir, 'settings.json'), 'utf8'))
    return Number.isInteger(port) && port > 0 && port <= 65535 ? port : DEFAULT_PORT
  } catch {
    return DEFAULT_PORT
  }
}

/**
 * `/api` and `/ws` go to the Host. The target is read on every request, not once at startup: Vite starts before
 * the app, and a first run may save a free port instead of the default, or the port page may save a new one.
 */
export function hostProxy(dataDir: string): Record<string, ProxyOptions> {
  const options = (ws: boolean): ProxyOptions => ({
    changeOrigin: true,
    ws,
    // http-proxy copies `proxy.options` into each request's options, so a getter makes the target per request.
    configure: (proxy) =>
      Object.defineProperty(proxy.options, 'target', {
        // 127.0.0.1, not localhost: Node may resolve localhost to ::1, but the Host binds IPv4 only.
        get: () => `http://127.0.0.1:${hostPort(dataDir)}`,
        enumerable: true,
      }),
  })
  return Object.fromEntries(HOST_PREFIXES.map((p) => [p, options(p === '/ws')]))
}

export function devServer(): Plugin {
  return {
    name: 'streamer-toolkit-dev-server',
    configureServer(server) {
      // Added here, these run before Vite's own middlewares (including the proxy).
      server.middlewares.use((req, res, next) => {
        if (!isHostPath(req.url)) {
          // Sandboxed preview iframes have the null origin and load /@vite/client, /src/... as CORS module scripts.
          res.setHeader('Vary', 'Origin')
          if (req.headers.origin === 'null') res.setHeader('Access-Control-Allow-Origin', 'null')
          // The admin UI is an SPA under /admin/: every page load below it gets its index, as the Host does.
          if (/^\/admin(\/|\?|$)/.test(req.url ?? '') && req.headers.accept?.includes('text/html')) {
            req.url = '/admin/index.html'
          }
        }
        next()
      })
    },
  }
}
