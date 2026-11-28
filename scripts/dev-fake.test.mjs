import assert from 'node:assert/strict'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { HOST_PORT, OBS_PORT, fakesArgs, prepare, waitUntil } from './dev-fake.mjs'

test('prepare makes a fresh dev folder that points at the fakes', () => {
  const dir = join(mkdtempSync(join(tmpdir(), 'st-dev-fake-')), 'data')
  try {
    prepare(dir, false)
    assert.ok(existsSync(join(dir, 'dev-folder')), 'the dev marker, or DEV-1 refuses the folder')
    const settings = JSON.parse(readFileSync(join(dir, 'settings.json'), 'utf8'))
    assert.equal(settings.port, HOST_PORT, 'port written, or the release default applies')
    assert.equal(settings.obs.port, OBS_PORT)
    assert.ok(settings.twitch_client_id)

    writeFileSync(join(dir, 'toolkit.db'), 'old records')
    prepare(dir, true)
    assert.ok(existsSync(join(dir, 'toolkit.db')), '--keep keeps the folder')
    prepare(dir, false)
    assert.ok(!existsSync(join(dir, 'toolkit.db')), 'each run starts fresh')
  } finally {
    rmSync(join(dir, '..'), { recursive: true, force: true })
  }
})

test('the dev marker name matches the desktop app', () => {
  const rust = readFileSync(new URL('../apps/desktop/src/data_folder.rs', import.meta.url), 'utf8')
  const script = readFileSync(new URL('./dev-fake.mjs', import.meta.url), 'utf8')
  const name = /pub const DEV_MARKER: &str = "([^"]+)";/.exec(rust)?.[1]
  assert.ok(name)
  assert.match(script, new RegExp(`const DEV_MARKER = '${name}'`))
})

test('waiting for the fakes stops as soon as they exit or come up', async () => {
  assert.equal(await waitUntil(async () => true, () => false, 3, 1), true)
  let checks = 0
  const stopped = await waitUntil(async () => (checks++, false), () => true, 100, 1)
  assert.equal(stopped, false)
  assert.equal(checks, 0, 'an exited fake-services is not waited for')
  assert.equal(await waitUntil(async () => false, () => false, 3, 1), false, 'never up')
})

test('the panel is built and opened unless --no-ui', () => {
  const ui = fakesArgs(['node', 'dev-fake.mjs'])
  assert.deepEqual(ui.build.slice(-2), ['--features', 'ui'])
  assert.equal(ui.run.at(-1), '--ui')
  const plain = fakesArgs(['node', 'dev-fake.mjs', '--no-ui'])
  assert.ok(!plain.build.includes('--features'))
  assert.ok(!plain.run.includes('--ui'))
})

test('the fake OBS overlays point at the dev:fake Host port', () => {
  const { run } = fakesArgs(['node', 'dev-fake.mjs', '--no-ui'])
  assert.deepEqual(run.slice(run.indexOf('--host-port'), run.indexOf('--host-port') + 2), ['--host-port', String(HOST_PORT)])
})
