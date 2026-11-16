// Sounds: the home tile and drawer, and the full page's import, rename, volume and delete, with the admin API
// mocked (file checks and storage are tested on the real Host in crates/modules/sound).
import { type Page, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

type Schemas = components['schemas']

const HTML = '<img src=x onerror="window.__xss=1">'
const modules = [{ id: 'sound', enabled: true, closed: false }]
const fill = (text: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((s, [k, v]) => s.replace(`{${k}}`, String(v)), text)

/** A mocked sound library; PUT and DELETE change it, POST adds what `imported` gives. */
async function library(page: Page, sounds: Schemas['Sound'][]) {
  const s = { sounds: [...sounds], puts: [] as unknown[], uploads: [] as { name: string | null; bytes: number }[] }
  await page.route('**/api/admin/sounds**', (r) => {
    const req = r.request()
    const url = new URL(req.url())
    const id = url.pathname.split('/')[4]
    if (req.method() === 'GET') return r.fulfill({ json: { sounds: s.sounds } })
    if (req.method() === 'POST') {
      const name = url.searchParams.get('name')
      s.uploads.push({ name, bytes: req.postDataBuffer()?.length ?? 0 })
      if (!name?.endsWith('.wav')) return r.fulfill({ status: 400, json: { code: 'sound.unsupported_type' } })
      const sound = { id: `s${s.sounds.length + 1}`, label: name.replace(/\.wav$/, ''), volume: 100 }
      s.sounds.push(sound)
      return r.fulfill({ json: sound })
    }
    if (req.method() === 'PUT') {
      s.puts.push(req.postDataJSON())
      s.sounds = s.sounds.map((x) => (x.id === id ? { id, ...req.postDataJSON() } : x))
      return r.fulfill({ json: s.sounds.find((x) => x.id === id) })
    }
    s.sounds = s.sounds.filter((x) => x.id !== id)
    return r.fulfill({ status: 204 })
  })
  return s
}

test('the tile counts sounds; its drawer lists them as text', async ({ page }) => {
  await mockAdmin(page, { modules })
  await library(page, [{ id: 's1', label: HTML, volume: 80 }])
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
  const s = await library(page, [{ id: 's1', label: HTML, volume: 80 }])
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
  await library(page, [{ id: 's1', label: 'x'.repeat(100), volume: 80 }])
  await page.goto('/admin/sounds')
  await page.getByRole('button', { name: zh.sound.delete }).click()
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }
})
