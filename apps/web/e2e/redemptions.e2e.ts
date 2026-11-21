// Redemption records: the home tile and drawer, and the full page's filters, pages and live refresh, with the admin
// API and `/ws/overlay` mocked (filtering itself is tested on real SQLite in crates/modules/redemption).
import { type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, openAdmin, scrollsSideways, zh } from './fixtures'

type Schemas = components['schemas']

const HTML = '<img src=x onerror="window.__xss=1">'

const record = (n: number, over: Partial<Schemas['RedemptionRecord']> = {}): Schemas['RedemptionRecord'] => ({
  id: `r${n}`,
  user_login: `viewer${n}`,
  user_name: `Viewer${n}`,
  reward_title: 'Hydrate',
  reward_cost: 100,
  user_input: '',
  status: 'unfulfilled',
  redeemed_at: '2026-11-16T10:00:00.000Z',
  updated_at: '2026-11-16T10:00:00.000Z',
  ...over,
})

/** Answers the list endpoint with `page()` and records each request's query. */
async function mockRecords(page: Page, answer: () => Schemas['RedemptionPage']) {
  const queries: URLSearchParams[] = []
  await page.route('**/api/admin/redemptions*', (r) => {
    queries.push(new URL(r.request().url()).searchParams)
    return r.fulfill({ json: answer() })
  })
  return queries
}

/** `/ws/overlay` with a handle to push events later. */
async function liveSocket(page: Page) {
  let socket: WebSocketRoute | undefined
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) => {
    socket = ws
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 0, payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] } }))
  })
  return {
    changed: async (seq: number) => {
      await expect.poll(() => socket).toBeDefined()
      socket!.send(
        JSON.stringify({
          id: `e${seq}`,
          type: 'redemption.changed',
          version: 1,
          seq,
          occurredAt: '2026-11-16T10:00:00.000Z',
          source: 'twitch',
          payload: { id: 'r9' },
        }),
      )
    },
  }
}

const modules = [{ id: 'redemption', enabled: true, closed: false }]
const tile = (page: Page) => page.locator('[data-drawer-tile="redemption"]')

test('the tile shows the latest redemption; its drawer lists recent ones as text', async ({ page }) => {
  await mockAdmin(page, { modules })
  let items = [record(2, { user_input: HTML }), record(1)]
  const queries = await mockRecords(page, () => ({ items, total: items.length }))
  const live = await liveSocket(page)
  await page.goto('/admin/')

  const features = page.getByRole('region', { name: zh.home.tiles })
  await expect(features.locator('[data-drawer-tile="redemption"]')).toContainText('Viewer2：Hydrate')
  expect(queries[0].get('limit')).toBe('5')

  await tile(page).click()
  const drawer = page.locator('#drawer-redemption')
  await expect(drawer.getByText(HTML)).toBeVisible()
  await expect(drawer.locator('img')).toHaveCount(0)
  expect(await page.evaluate(() => 'xss' in window || '__xss' in window)).toBe(false)

  // A new record shows without reloading.
  items = [record(9, { user_name: 'Newest', reward_title: 'Song' }), ...items]
  await live.changed(1)
  await expect(drawer.getByText('Newest')).toBeVisible()
  await expect(tile(page)).toContainText('Newest：Song')

  await drawer.getByRole('link', { name: zh.drawer.fullPage }).click()
  await expect(page).toHaveURL(/\/admin\/redemptions$/)
  await expect(page.getByRole('heading', { name: zh.modules.names.redemption })).toBeVisible()
})

test('no records: the drawer and the full page say so', async ({ page }) => {
  await mockAdmin(page, { modules })
  await mockRecords(page, () => ({ items: [], total: 0 }))
  await page.goto('/admin/')
  await expect(tile(page)).toContainText(zh.redemption.empty)
  await tile(page).click()
  await expect(page.locator('#drawer-redemption').getByText(zh.redemption.empty)).toBeVisible()

  await page.goto('/admin/redemptions')
  await expect(page.getByText(zh.redemption.empty)).toBeVisible()
  await page.getByLabel(zh.redemption.viewer).fill('nobody')
  await page.getByRole('button', { name: zh.redemption.apply, exact: true }).click()
  await expect(page.getByText(zh.redemption.noMatch)).toBeVisible()
})

test('the full page sends the filters, pages through and reloads on changes', async ({ page }) => {
  await mockAdmin(page, { modules })
  let total = 120
  const queries = await mockRecords(page, () => ({ items: [record(1)], total }))
  const live = await liveSocket(page)
  await page.goto('/admin/redemptions')
  await expect(page.getByText(zh.redemption.total.replace('{n}', '120'))).toBeVisible()
  await expect(page.getByText('第 1 / 3 頁')).toBeVisible()

  await page.getByLabel(zh.redemption.from).fill('2026-11-01')
  await page.getByLabel(zh.redemption.to).fill('2026-11-16')
  await page.getByLabel(zh.redemption.viewer).fill(' Ali ')
  await page.getByLabel(zh.redemption.reward).fill('song')
  await page.getByRole('combobox', { name: zh.redemption.statusLabel }).click()
  await page.getByRole('option', { name: zh.redemption.status.canceled }).click()
  await page.getByRole('button', { name: zh.redemption.apply, exact: true }).click()
  await expect.poll(() => queries.at(-1)?.get('status')).toBe('canceled')
  const q = queries.at(-1)!
  expect(q.get('viewer')).toBe('Ali')
  expect(q.get('reward')).toBe('song')
  expect(q.get('offset')).toBe('0')
  // Local dates, the end date included (the Host's `to` is exclusive).
  const local = (y: number, m: number, d: number) => new Date(y, m - 1, d).toISOString()
  expect([q.get('from'), q.get('to')]).toEqual([local(2026, 11, 1), local(2026, 11, 17)])

  await page.getByRole('button', { name: zh.redemption.next }).click()
  await expect.poll(() => queries.at(-1)?.get('offset')).toBe('50')
  await expect(page.getByText('第 2 / 3 頁')).toBeVisible()
  expect(queries.at(-1)!.get('status')).toBe('canceled')

  // A change on Twitch reloads the same page with the same filters.
  const before = queries.length
  total = 121
  await live.changed(1)
  await expect(page.getByText(zh.redemption.total.replace('{n}', '121'))).toBeVisible()
  expect(queries.length).toBeGreaterThan(before)
  expect(queries.at(-1)!.get('offset')).toBe('50')

  await page.getByRole('button', { name: zh.redemption.clear }).click()
  await expect.poll(() => queries.at(-1)?.get('status')).toBeNull()
  expect(queries.at(-1)!.get('offset')).toBe('0')
  await expect(page.getByLabel(zh.redemption.viewer)).toHaveValue('')
})

test('a rejected filter shows why', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/redemptions*', (r) =>
    r.fulfill({ status: 400, json: { code: 'redemption.invalid_filter' } }),
  )
  await page.goto('/admin/redemptions')
  await expect(page.getByRole('alert')).toHaveText(zh.errors.redemption.invalid_filter)
})

for (const width of [1280, 640, 320]) {
  test(`the drawer and the full page have no sideways scroll at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    const long = 'x'.repeat(200)
    await openAdmin(page, { modules })
    await mockRecords(page, () => ({
      items: [record(1, { user_name: long, reward_title: long, user_input: long }), record(2)],
      total: 120,
    }))
    await page.reload()
    await tile(page).click()
    const drawer = page.locator('#drawer-redemption')
    await expect(drawer.getByText(long).first()).toBeVisible()
    expect(await scrollsSideways(page), 'page with the drawer').toBe(false)
    expect(await scrollsSideways(drawer), 'drawer').toBe(false)

    await page.goto('/admin/redemptions')
    await expect(page.getByText(long).first()).toBeVisible()
    expect(await scrollsSideways(page), 'full page').toBe(false)
  })
}
