import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { type AddressInfo } from 'node:net'
import { createServer as createHttpServer, request, type IncomingHttpHeaders } from 'node:http'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { createServer, resolveConfig, type ViteDevServer } from 'vite'
import {
  ADMIN_CSP,
  DEV_DEFAULT_PORT,
  OVERLAY_CSP,
  PREVIEW_PATHS,
  devCsp,
  devDataDir,
  hostPort,
} from './dev-server.ts'

const portOf = (s: { address(): unknown }) => (s.address() as AddressInfo).port

describe('hostPort', () => {
  it("falls back to a dev build's default port for a missing, broken or invalid settings file", () => {
    const dir = mkdtempSync(join(tmpdir(), 'st-dev-port-'))
    expect(hostPort(dir)).toBe(47811)
    writeFileSync(join(dir, 'settings.json'), '{oops')
    expect(hostPort(dir)).toBe(47811)
    writeFileSync(join(dir, 'settings.json'), '{"port":"5000"}')
    expect(hostPort(dir)).toBe(47811)
    writeFileSync(join(dir, 'settings.json'), '{"port":51234,"twitch_client_id":""}')
    expect(hostPort(dir)).toBe(51234)
    rmSync(dir, { recursive: true })
  })

  it('matches the dev default port of the desktop app', () => {
    const main = readFileSync(resolve(import.meta.dirname, '../../apps/desktop/src/main.rs'), 'utf8')
    expect(Number(/const DEV_DEFAULT_PORT: u16 = (\d+);/.exec(main)?.[1])).toBe(DEV_DEFAULT_PORT)
  })
})

describe('devDataDir', () => {
  it('reads DEV_DATA_DIR for a run with --data-dir, else the debug build folder', () => {
    const custom = join(tmpdir(), 'st-dev-data')
    expect(devDataDir({ DEV_DATA_DIR: custom })).toBe(resolve(custom))
    const target = join(tmpdir(), 'st-target')
    expect(devDataDir({ CARGO_TARGET_DIR: target })).toBe(resolve(target, 'debug', 'data'))
    expect(devDataDir({})).toBe(resolve(import.meta.dirname, '../../target/debug/data'))
  })
})

const hostSource = () => readFileSync(resolve(import.meta.dirname, '../../crates/host/src/lib.rs'), 'utf8')

it.each([
  ['ADMIN_CSP', ADMIN_CSP],
  ['OVERLAY_CSP', OVERLAY_CSP],
])('%s matches the Host', (name, csp) => {
  const literal = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(hostSource())?.[1]
  // A trailing backslash continues a Rust string literal and drops the newline and leading spaces.
  expect(literal?.replace(/\\\r?\n\s*/g, '')).toBe(csp)
})

it('PREVIEW_PATHS matches the Host', () => {
  const list = /const PREVIEW_PATHS: \[&str; \d+\] = \[([^\]]*)\];/.exec(hostSource())?.[1]
  expect(list?.match(/"[^"]*"/g)?.map((s) => s.slice(1, -1))).toEqual(PREVIEW_PATHS)
})

it('picks the CSP the Host sends for each page', () => {
  const admin = `${ADMIN_CSP}; frame-src http://localhost:5173/overlay/chat/ http://localhost:5173/overlay/alerts/ http://localhost:5173/overlay/checkin/`
  expect(devCsp('/admin/', 'localhost:5173')).toBe(admin)
  expect(devCsp('/port/', 'localhost:5173')).toBe(admin)
  expect(devCsp('/overlay/chat/', 'localhost:5173')).toBe(OVERLAY_CSP)
  // Nothing from a Host header gets into the policy except a plain host name and port.
  expect(devCsp('/admin/', "x; script-src 'unsafe-inline'")).toBe(`${ADMIN_CSP}; frame-src 'none'`)
  expect(devCsp('/admin/')).toBe(`${ADMIN_CSP}; frame-src 'none'`)
})

/** Runs `fn` with these environment variables set (`undefined` removes one), then puts them back. */
async function withEnv<T>(vars: Record<string, string | undefined>, fn: () => Promise<T>): Promise<T> {
  const previous = Object.fromEntries(Object.keys(vars).map((k) => [k, process.env[k]]))
  const apply = (v: Record<string, string | undefined>) => {
    for (const [k, value] of Object.entries(v)) {
      if (value === undefined) delete process.env[k]
      else process.env[k] = value
    }
  }
  apply(vars)
  try {
    return await fn()
  } finally {
    apply(previous)
  }
}

const startVite = () =>
  createServer({
    configFile: resolve(import.meta.dirname, 'vite.config.ts'),
    logLevel: 'silent',
    server: { port: 0, host: '127.0.0.1' },
    optimizeDeps: { noDiscovery: true, include: [] },
  })

describe('dev server with DEV_DATA_DIR', () => {
  const repo = resolve(import.meta.dirname, '../..').replaceAll('\\', '/')
  // Inside the repo but outside target/, where only the DEV_DATA_DIR rule can deny it.
  const name = `st-dev-data-test-${process.pid}`
  const dataDir = join(repo, 'apps', 'web', name)
  let vite: ViteDevServer
  let base = ''

  beforeAll(async () => {
    mkdirSync(dataDir, { recursive: true })
    writeFileSync(join(dataDir, 'settings.json'), '{"port":1}')
    vite = await withEnv({ DEV_DATA_DIR: dataDir }, startVite)
    await vite.listen()
    base = `http://127.0.0.1:${portOf(vite.httpServer!)}`
  })

  afterAll(async () => {
    await vite?.close()
    rmSync(dataDir, { recursive: true, force: true })
  })

  it('never serves files in DEV_DATA_DIR', async () => {
    // 403, not 404: the file is there and the deny rule refuses it.
    const denied = await fetch(`${base}/@fs/${repo}/apps/web/${name}/settings.json`, { headers: { Origin: 'null' } })
    expect(denied.status).toBe(403)
    const served = await fetch(`${base}/@fs/${repo}/Cargo.toml`, { headers: { Origin: 'null' } })
    expect(served.status).toBe(200)
  })
})

// The real vite.config.ts against a fake Host; CARGO_TARGET_DIR points the data folder at a temp directory.
describe('dev server', () => {
  const targetDir = mkdtempSync(join(tmpdir(), 'st-dev-target-'))
  const dataDir = join(targetDir, 'debug', 'data')
  const seen: { url?: string; headers: IncomingHttpHeaders; upgrade: boolean }[] = []
  const host = createHttpServer((req, res) => {
    seen.push({ url: req.url, headers: req.headers, upgrade: false })
    res.end('host')
  })
  host.on('upgrade', (req, socket) => {
    seen.push({ url: req.url, headers: req.headers, upgrade: true })
    socket.destroy()
  })
  let vite: ViteDevServer
  let base = ''

  beforeAll(async () => {
    await new Promise<void>((r) => host.listen(0, '127.0.0.1', r))
    // DEV_DATA_DIR from the developer's shell would win over CARGO_TARGET_DIR.
    vite = await withEnv({ CARGO_TARGET_DIR: targetDir, DEV_DATA_DIR: undefined }, startVite)
    await vite.listen()
    base = `http://127.0.0.1:${portOf(vite.httpServer!)}`
    // Saved after Vite started, as on a data folder's first run when the default port is taken.
    mkdirSync(dataDir, { recursive: true })
    writeFileSync(join(dataDir, 'settings.json'), JSON.stringify({ port: portOf(host) }))
  })

  afterAll(async () => {
    await vite?.close()
    host.close()
    rmSync(targetDir, { recursive: true, force: true })
  })

  it('proxies /api to the port saved after startup', async () => {
    const res = await fetch(`${base}/api/ping`, { headers: { Origin: 'null' } })
    expect(await res.text()).toBe('host')
    expect(seen.at(-1)).toMatchObject({ url: '/api/ping', upgrade: false })
    expect(seen.at(-1)?.headers.host).toBe(`127.0.0.1:${portOf(host)}`)
    expect(res.headers.get('access-control-allow-origin')).toBeNull()
  })

  it('proxies /ws upgrades to the same port', async () => {
    await new Promise<void>((done) => {
      const req = request(`${base}/ws`, { headers: { Connection: 'Upgrade', Upgrade: 'websocket', Origin: 'null' } })
      req.on('error', () => done())
      req.on('close', () => done())
      req.end()
    })
    expect(seen.at(-1)).toMatchObject({ url: '/ws', upgrade: true })
  })

  it('proxies /templates to the Host without opening it to the null origin', async () => {
    const res = await fetch(`${base}/templates/chat/style.css`, { headers: { Origin: 'null' } })
    expect(await res.text()).toBe('host')
    expect(seen.at(-1)).toMatchObject({ url: '/templates/chat/style.css', upgrade: false })
    expect(res.headers.get('access-control-allow-origin')).toBeNull()
  })

  it('proxies /sounds and /music to the Host without opening them to the null origin', async () => {
    for (const path of ['/sounds/x', '/music/x', '/media/x', '/checkin/x']) {
      const res = await fetch(`${base}${path}`, { headers: { Origin: 'null' } })
      expect(await res.text()).toBe('host')
      expect(res.headers.get('access-control-allow-origin')).toBeNull()
    }
  })

  it('leaves the CSP to devServer()', () => {
    // Vite applies server.headers after every middleware, so a CSP there would replace the overlay CSP.
    // Checked on the config: local HTTPS filters (e.g. AdGuard) may rewrite CSP headers on the wire.
    expect(vite.config.server.headers?.['Content-Security-Policy']).toBeUndefined()
  })

  it('opens Vite modules to the null origin only', async () => {
    const open = await fetch(`${base}/@vite/client`, { headers: { Origin: 'null' } })
    expect(open.status).toBe(200)
    expect(open.headers.get('access-control-allow-origin')).toBe('null')

    const other = await fetch(`${base}/@vite/client`, { headers: { Origin: 'http://localhost:5173' } })
    expect(other.headers.get('access-control-allow-origin')).toBeNull()
  })

  it("keeps Vite's default fs.deny list", async () => {
    const defaults = (await resolveConfig({ configFile: false, logLevel: 'silent' }, 'serve')).server.fs.deny
    expect(vite.config.server.fs.deny).toEqual(expect.arrayContaining(defaults))
  })

  it('never serves files under the Cargo target folder, where the dev data folder lives', async () => {
    const repo = resolve(import.meta.dirname, '../..').replaceAll('\\', '/')
    const dir = join(repo, 'target', 'st-dev-server-test')
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'settings.json'), '{"port":1}')
    try {
      const denied = await fetch(`${base}/@fs/${repo}/target/st-dev-server-test/settings.json`, { headers: { Origin: 'null' } })
      expect(denied.status).toBe(403)
      // Control: a file outside target/ is served, so the 403 comes from the deny rule.
      const served = await fetch(`${base}/@fs/${repo}/Cargo.toml`, { headers: { Origin: 'null' } })
      expect(served.status).toBe(200)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('serves the admin index for deep paths', async () => {
    for (const path of ['/admin', '/admin/', '/admin/chat', '/admin/chat/settings?x=1', '/admin/templates/my.theme']) {
      const res = await fetch(`${base}${path}`, { headers: { Accept: 'text/html' } })
      expect(res.status, path).toBe(200)
      expect(await res.text(), path).toContain('/src/admin/main.ts')
    }
  })
})
