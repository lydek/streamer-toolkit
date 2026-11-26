// `pnpm dev:fake [--keep]`: the fake OBS and fake Twitch (crates/fakes) plus a dev build in its own data folder,
// signed in and connected from the start. The folder is made fresh each run (fake events reach the business records);
// `--keep` keeps the last one. Send events with `cargo run -q -p fakes -- send follow` (see docs/dev.md).
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { createConnection } from 'node:net'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(import.meta.dirname, '..')
export const DATA_DIR = resolve(root, 'target', 'dev-fake-data')
export const OBS_PORT = 4466
export const TWITCH_PORT = 4467
// Not 47810 (release, where OBS browser sources point) nor 47811 (a normal dev folder), so all three can run.
export const HOST_PORT = 47812
// apps/desktop/src/data_folder.rs DEV_MARKER: without it a dev build refuses a folder it did not create.
const DEV_MARKER = 'dev-folder'

/** Makes the data folder: the dev marker, and settings that point at the fakes (written with `port`, see DEV-2). */
export function prepare(dir, keep) {
  if (!keep) rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, DEV_MARKER), '')
  const settings = join(dir, 'settings.json')
  if (!existsSync(settings)) {
    const values = {
      port: HOST_PORT,
      twitch_client_id: 'fake-client',
      setup_done: true,
      obs: { host: '127.0.0.1', port: OBS_PORT },
    }
    writeFileSync(settings, JSON.stringify(values, null, 2))
  }
}

const listening = (port) =>
  new Promise((done) => {
    const socket = createConnection({ host: '127.0.0.1', port }, () => {
      socket.destroy()
      done(true)
    })
    socket.on('error', () => done(false))
  })

/** Polls `ready` until it is true (`true`), `stopped` turns true or the tries run out (`false`). */
export async function waitUntil(ready, stopped, tries = 100, delayMs = 100) {
  for (let i = 0; i < tries; i++) {
    if (stopped()) return false
    if (await ready()) return true
    await new Promise((r) => setTimeout(r, delayMs))
  }
  return !stopped() && (await ready())
}

async function main() {
  const keep = process.argv.includes('--keep')
  for (const port of [OBS_PORT, TWITCH_PORT]) {
    if (await listening(port)) {
      console.error(`127.0.0.1:${port} is taken (another fake-services?); stop it first.`)
      process.exit(1)
    }
  }
  prepare(DATA_DIR, keep)
  const build = spawnSync('cargo', ['build', '-q', '-p', 'fakes'], { cwd: root, stdio: 'inherit', shell: false })
  if (build.status !== 0) process.exit(build.status ?? 1)
  const exe = join(root, 'target', 'debug', process.platform === 'win32' ? 'fake-services.exe' : 'fake-services')
  const fakes = spawn(exe, ['--obs-port', String(OBS_PORT), '--twitch-port', String(TWITCH_PORT)], {
    stdio: 'inherit',
  })
  let app
  process.on('exit', () => {
    fakes.kill()
    app?.kill()
  })
  process.on('SIGINT', () => process.exit(130))
  fakes.on('exit', (code) => {
    console.error(`fake-services stopped (${code}).`)
    process.exit(1)
  })
  const up = async () => (await listening(OBS_PORT)) && (await listening(TWITCH_PORT))
  if (!(await waitUntil(up, () => fakes.exitCode !== null))) {
    console.error(`fake-services did not start on 127.0.0.1:${OBS_PORT} and ${TWITCH_PORT}.`)
    process.exit(1)
  }
  const tauri = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
  app = spawn(process.execPath, [tauri, 'dev', '--', '--', '--data-dir', DATA_DIR], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, TOOLKIT_FAKE_SERVICES: `127.0.0.1:${TWITCH_PORT}`, DEV_DATA_DIR: DATA_DIR },
  })
  app.on('exit', (code) => process.exit(code ?? 0))
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
