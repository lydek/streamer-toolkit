import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { type AddressInfo } from 'node:net'
import { createServer as createHttpServer, request, type IncomingHttpHeaders } from 'node:http'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { createServer, resolveConfig, type ViteDevServer } from 'vite'
import { ADMIN_CSP, hostPort } from './dev-server.ts'

const portOf = (s: { address(): unknown }) => (s.address() as AddressInfo).port

describe('hostPort', () => {
  it('falls back to the default port for a missing, broken or invalid settings file', () => {
    const dir = mkdtempSync(join(tmpdir(), 'st-dev-port-'))
    expect(hostPort(dir)).toBe(47810)
    writeFileSync(join(dir, 'settings.json'), '{oops')
    expect(hostPort(dir)).toBe(47810)
    writeFileSync(join(dir, 'settings.json'), '{"port":"5000"}')
    expect(hostPort(dir)).toBe(47810)
    writeFileSync(join(dir, 'settings.json'), '{"port":51234,"twitch_client_id":""}')
    expect(hostPort(dir)).toBe(51234)
    rmSync(dir, { recursive: true })
  })
})

it('ADMIN_CSP matches the Host', () => {
  const rust = readFileSync(resolve(import.meta.dirname, '../../crates/host/src/lib.rs'), 'utf8')
  const literal = /const ADMIN_CSP: &str = "((?:[^"\\]|\\[\s\S])*)";/.exec(rust)?.[1]
  // A trailing backslash continues a Rust string literal and drops the newline and leading spaces.
  expect(literal?.replace(/\\\r?\n\s*/g, '')).toBe(ADMIN_CSP)
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
    const previous = process.env.CARGO_TARGET_DIR
    process.env.CARGO_TARGET_DIR = targetDir
    try {
      vite = await createServer({
        configFile: resolve(import.meta.dirname, 'vite.config.ts'),
        logLevel: 'silent',
        server: { port: 0, host: '127.0.0.1' },
        optimizeDeps: { noDiscovery: true, include: [] },
      })
    } finally {
      if (previous === undefined) delete process.env.CARGO_TARGET_DIR
      else process.env.CARGO_TARGET_DIR = previous
    }
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

  it('sends the admin CSP', () => {
    // Checked on the config: local HTTPS filters (e.g. AdGuard) may rewrite CSP headers on the wire.
    expect(vite.config.server.headers).toMatchObject({ 'Content-Security-Policy': ADMIN_CSP })
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
