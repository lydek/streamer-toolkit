// The tabs' waiting counts, API mocked: song requests in the queue and redemptions not yet fulfilled.
import { expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

const modules = ['songs', 'redemption', 'counter'].map((id) => ({ id, enabled: true, closed: false }))
/** A tab's accessible name: its own name, then the screen-reader phrase for `n` waiting items. */
const named = (name: string, n: number) => new RegExp(`^${name}\\s*${zh.nav.pending.replace('{n}', String(n))}$`)

test('opened on another page, the tabs count the whole queue and every unfulfilled redemption', async ({ page }) => {
  await mockAdmin(page, { modules })
  const queued = [1, 2].map((n) => ({ id: `q${n}`, title: `Song ${n}`, artist: '', viewer: null }))
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: { singing: null, queued, sung: [] } }))
  await page.route('**/api/admin/redemptions*', (r) => r.fulfill({ json: { total: 12, failed_writes: 0, items: [] } }))
  await page.goto('/admin/counters')
  const tabs = page.getByRole('navigation', { name: zh.nav.label })
  await expect(tabs.getByRole('link', { name: named(zh.nav.groups.song, 2) })).toBeVisible()
  await expect(tabs.getByRole('link', { name: named(zh.nav.groups.interact, 12) })).toBeVisible()
  // The second row of 互動 shows the count on 頻道點數, the category holding the redemption records.
  const sub = page.getByRole('navigation', { name: zh.nav.sub })
  await expect(sub.getByRole('link', { name: named(zh.nav.categories.points, 12) })).toBeVisible()
})

test('a feature that is off shows no count', async ({ page }) => {
  await mockAdmin(page, { modules: [{ id: 'songs', enabled: true, closed: false }] })
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: { singing: null, queued: [], sung: [] } }))
  await page.goto('/admin/')
  const tabs = page.getByRole('navigation', { name: zh.nav.label })
  await expect(tabs.getByRole('link', { name: zh.nav.groups.song, exact: true })).toBeVisible()
  await expect(tabs.locator('.pending-count')).toHaveCount(0)
})

test('on a phone the count sits on the bottom tab’s icon, not under its name', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: { singing: null, queued: [], sung: [] } }))
  await page.route('**/api/admin/redemptions*', (r) => r.fulfill({ json: { total: 2, failed_writes: 0, items: [] } }))
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/admin/counters')
  const tab = page.getByRole('navigation', { name: zh.nav.label }).getByRole('link', { name: named(zh.nav.groups.interact, 2) })
  const [t, badge, icon] = await Promise.all([tab.boundingBox(), tab.locator('.pending-count').boundingBox(), tab.locator('i').first().boundingBox()])
  expect(badge!.y).toBeLessThan(icon!.y + icon!.height)
  expect(badge!.y + badge!.height).toBeLessThanOrEqual(t!.y + t!.height)
  expect(badge!.x).toBeGreaterThan(icon!.x)
})
