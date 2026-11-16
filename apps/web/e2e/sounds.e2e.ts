// Sounds: the home tile and drawer, the full page's import, rename, volume and delete, and choosing sounds for
// alerts and check-ins, with the admin API mocked (file checks and storage are tested on the real Host in
// crates/modules/sound); then the overlays of a real Host playing the chosen sounds.
import { randomUUID } from 'node:crypto'
import { type Page, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, scrollsSideways, zh } from './fixtures'
import { HOST_PORT } from './ports'

type Schemas = components['schemas']

const HOST = `http://127.0.0.1:${HOST_PORT}`
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }
const HTML = '<img src=x onerror="window.__xss=1">'
const modules = [{ id: 'sound', enabled: true, closed: false }]
const fill = (text: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((s, [k, v]) => s.replace(`{${k}}`, String(v)), text)
const sound = (id: string, label: string, volume = 80): Schemas['Sound'] => ({ id, label, volume, used_by: [] })

/** A mocked sound library; PUT and DELETE change it, POST adds an uploaded `.wav`. */
async function library(page: Page, sounds: Schemas['Sound'][], slots: Record<string, string> = {}) {
  const s = {
    sounds: [...sounds],
    slots: { ...slots },
    puts: [] as unknown[],
    uploads: [] as { name: string | null; bytes: number }[],
  }
  await page.route('**/api/admin/sounds**', (r) => {
    const req = r.request()
    const url = new URL(req.url())
    const [, , , , id, slot] = url.pathname.split('/')
    if (req.method() === 'GET') return r.fulfill({ json: { sounds: s.sounds, slots: s.slots } })
    if (id === 'slots') {
      const chosen = req.postDataJSON().sound
      if (chosen === null) delete s.slots[decodeURIComponent(slot)]
      else s.slots[decodeURIComponent(slot)] = chosen
      return r.fulfill({ status: 204 })
    }
    if (req.method() === 'POST') {
      const name = url.searchParams.get('name')
      s.uploads.push({ name, bytes: req.postDataBuffer()?.length ?? 0 })
      if (!name?.endsWith('.wav')) return r.fulfill({ status: 400, json: { code: 'sound.unsupported_type' } })
      const added = sound(`s${s.sounds.length + 1}`, name.replace(/\.wav$/, ''), 100)
      s.sounds.push(added)
      return r.fulfill({ json: added })
    }
    if (req.method() === 'PUT') {
      s.puts.push(req.postDataJSON())
      s.sounds = s.sounds.map((x) => (x.id === id ? { ...x, ...req.postDataJSON() } : x))
      return r.fulfill({ json: s.sounds.find((x) => x.id === id) })
    }
    s.sounds = s.sounds.filter((x) => x.id !== id)
    return r.fulfill({ status: 204 })
  })
  return s
}

test('the tile counts sounds; its drawer lists them as text', async ({ page }) => {
  await mockAdmin(page, { modules })
  await library(page, [sound('s1', HTML)])
  await page.goto('/admin/')
  const tile = page.locator('[data-drawer-tile="sound"]')
  await expect(page.getByRole('list', { name: zh.home.groups.features }).locator(tile)).toContainText(
    fill(zh.sound.count, { n: 1 }),
  )
  await tile.click()
  const drawer = page.locator('#drawer-sound')
  await expect(drawer.getByText(HTML)).toBeVisible()
  await expect(drawer.locator('img')).toHaveCount(0)
  await drawer.getByRole('link', { name: zh.drawer.fullPage }).click()
  await expect(page).toHaveURL(/\/admin\/sounds$/)
  await expect(page.getByRole('heading', { name: zh.modules.names.sound, exact: true })).toBeVisible()
  expect(await page.evaluate(() => '__xss' in window)).toBe(false)
})

test('the page imports, renames, changes the volume and deletes sounds', async ({ page }) => {
  await mockAdmin(page, { modules })
  const s = await library(page, [sound('s1', HTML)])
  await page.goto('/admin/sounds')
  await expect(page.getByLabel(zh.sound.label).first()).toHaveValue(HTML)
  expect(await page.evaluate(() => '__xss' in window)).toBe(false)

  const file = page.getByLabel(zh.sound.file)
  await file.setInputFiles({ name: 'page.html', mimeType: 'text/html', buffer: Buffer.from('<b>') })
  await expect(page.getByRole('alert')).toHaveText(zh.errors.sound.unsupported_type)
  // Too large is refused before anything is sent.
  await file.setInputFiles({ name: 'big.wav', mimeType: 'audio/wav', buffer: Buffer.alloc(10 * 1024 * 1024 + 1) })
  await expect(page.getByRole('alert')).toHaveText(zh.errors.sound.too_large)
  expect(s.uploads.map((u) => u.name)).toEqual(['page.html'])
  await file.setInputFiles({ name: 'bell.wav', mimeType: 'audio/wav', buffer: Buffer.from('RIFF....WAVE') })
  await expect(page.getByLabel(zh.sound.label).nth(1)).toHaveValue('bell')
  expect(s.uploads.at(-1)).toEqual({ name: 'bell.wav', bytes: 12 })

  const name = page.getByLabel(zh.sound.label).nth(1)
  await name.fill('Ding')
  await page.getByLabel(zh.sound.volume).nth(1).fill('40')
  await name.focus()
  // Leaving the page saves what was typed without Enter.
  await page.getByRole('button', { name: zh.app.home }).click()
  await expect(page).toHaveURL(/\/admin\/$/)
  expect(s.puts).toEqual([
    { label: 'Ding', volume: 100 },
    { label: 'Ding', volume: 40 },
  ])

  await page.goto('/admin/sounds')
  const row = page.locator('li.sound').nth(1)
  await expect(row.getByLabel(zh.sound.label)).toHaveValue('Ding')
  await row.getByRole('button', { name: zh.sound.delete }).click()
  await expect(row.getByText(zh.sound.notUsed)).toBeVisible()
  await expect(row.getByRole('button', { name: zh.sound.confirmDelete })).toBeFocused()
  await page.keyboard.press('Tab')
  await page.keyboard.press('Enter')
  await expect(row.getByRole('button', { name: zh.sound.delete })).toBeFocused()
  await row.getByRole('button', { name: zh.sound.delete }).click()
  await row.getByRole('button', { name: zh.sound.confirmDelete }).click()
  await expect(page.getByLabel(zh.sound.label)).toHaveCount(1)
  expect(s.sounds.map((x) => x.id)).toEqual(['s1'])
})

test('the page has no sideways scroll at 1280, 640 and 320 px', async ({ page }) => {
  await mockAdmin(page, { modules })
  await library(page, [{ ...sound('s1', 'x'.repeat(100)), used_by: ['alert.follow', 'checkin.full'] }])
  await page.goto('/admin/sounds')
  await page.getByRole('button', { name: zh.sound.delete }).click()
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }
})

test('alerts and check-ins each choose a sound, and deleting a used sound lists where it plays', async ({ page }) => {
  await mockAdmin(page, { modules: [{ id: 'alerts', enabled: true, closed: false }, ...modules] })
  const s = await library(page, [sound('s1', 'Ding')], { 'alert.raid': 'silent' })
  await page.goto('/admin/alerts')
  const follow = page.getByRole('combobox', { name: zh.alerts.kinds.follow })
  const raid = page.getByRole('combobox', { name: zh.alerts.kinds.raid })
  await expect(follow).toHaveText(zh.sound.builtIn)
  await expect(raid).toHaveText(zh.sound.silent)
  await follow.click()
  await page.getByRole('option', { name: 'Ding' }).click()
  await expect.poll(() => s.slots['alert.follow']).toBe('s1')
  await expect(page.getByRole('listbox')).toHaveCount(0)
  await raid.click()
  await page.getByRole('option', { name: zh.sound.builtIn }).click()
  await expect.poll(() => 'alert.raid' in s.slots).toBe(false)

  await page.goto('/admin/settings')
  await expect(page.getByRole('combobox', { name: zh.sound.checkinNormal })).toHaveText(zh.sound.silent)
  await page.getByRole('combobox', { name: zh.sound.checkinFull }).click()
  await page.getByRole('option', { name: 'Ding' }).click()
  await expect.poll(() => s.slots['checkin.full']).toBe('s1')

  s.sounds[0].used_by = ['alert.follow', 'checkin.full']
  await page.goto('/admin/sounds')
  await page.getByRole('button', { name: zh.sound.delete }).click()
  await expect(page.locator('li.sound').getByRole('listitem')).toHaveText([
    fill(zh.sound.where.alert, { kind: zh.alerts.kinds.follow }),
    zh.sound.where.checkinFull,
  ])
})

test('simulated alerts and check-ins on a real Host play the chosen sound at its volume', async ({ page, request }) => {
  test.setTimeout(60_000)
  const wav = Buffer.concat([Buffer.from('RIFF'), Buffer.alloc(4), Buffer.from('WAVEfmt ')])
  const res = await request.post(`${HOST}/api/admin/sounds?name=e2e.wav`, {
    headers: { ...admin, 'Content-Type': 'application/octet-stream' },
    data: wav,
  })
  expect(res.status()).toBe(200)
  const { id } = await res.json()
  const put = (path: string, data: unknown) => request.put(`${HOST}${path}`, { headers: admin, data })
  expect((await put(`/api/admin/sounds/${id}`, { label: 'e2e', volume: 40 })).status()).toBe(200)
  for (const slot of ['alert.raid', 'checkin.full']) {
    expect((await put(`/api/admin/sounds/slots/${slot}`, { sound: id })).status()).toBe(204)
  }
  const file = await request.get(`${HOST}/sounds/${id}`)
  expect(file.headers()['content-type']).toBe('audio/wav')
  expect(file.headers()['x-content-type-options']).toBe('nosniff')

  await page.addInitScript(() => {
    const w = window as unknown as { played: [string, number][] }
    w.played = []
    // Headless Chromium refuses autoplay; record the attempts instead.
    HTMLMediaElement.prototype.play = function () {
      w.played.push([new URL(this.src).pathname, this.volume])
      return Promise.resolve()
    }
  })
  const played = () => page.evaluate(() => (window as unknown as { played: [string, number][] }).played)
  try {
    await page.goto(`${HOST}/overlay/checkin/`)
    await expect(page.locator('#app')).toBeAttached()
    const simulate = (stamps: number) =>
      request.post(`${HOST}/api/admin/simulate/checkin`, { headers: admin, data: { userName: 'e2e', stamps } })
    // A normal check-in stays silent; a full card plays the chosen sound.
    expect((await simulate(3)).status()).toBe(204)
    expect((await simulate(10)).status()).toBe(204)
    await expect.poll(played, { timeout: 15_000 }).toEqual([[`/sounds/${id}`, 0.4]])

    // Alerts other tests queued on the shared Host may play first; only this raid counts.
    const name = `e2e-${randomUUID().slice(0, 8)}`
    await page.goto(`${HOST}/overlay/alerts/`)
    await expect(page.locator('[data-alert-stage]')).toBeAttached()
    const raid = await request.post(`${HOST}/api/admin/simulate/alert`, {
      headers: admin,
      data: { kind: 'raid', amount: 3, userName: name },
    })
    expect(raid.status()).toBe(204)
    await expect(page.getByText(name)).toBeVisible({ timeout: 40_000 })
    expect((await played()).at(-1)).toEqual([`/sounds/${id}`, 0.4])
  } finally {
    // The Host is shared: deleting the sound puts its slots back to their defaults.
    expect((await request.delete(`${HOST}/api/admin/sounds/${id}`, { headers: admin })).status()).toBe(204)
  }
})
