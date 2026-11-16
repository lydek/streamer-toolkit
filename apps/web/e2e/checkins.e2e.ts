// Check-ins: the home tile and drawer, and the full page's search, pages, manual check-ins and live refresh, with the
// admin API and `/ws/overlay` mocked (the rules themselves are tested on real SQLite in crates/modules/checkin).
import { type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

type Schemas = components['schemas']

const HTML = '<img src=x onerror="window.__xss=1">'
const modules = [{ id: 'checkin', enabled: true, closed: false }]
const fill = (text: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((s, [k, v]) => s.replace(`{${k}}`, String(v)), text)

const viewer = (n: number, over: Partial<Schemas['CheckinViewer']> = {}): Schemas['CheckinViewer'] => ({
  id: `v${n}`,
  user_login: `viewer${n}`,
  user_name: `Viewer${n}`,
  total: 13,
  stamps: 3,
  card_slots: 10,
  last_check_in: '2026-11-15',
  ...over,
})

/** `/ws/overlay` with a handle to push `checkin.changed` later. */
async function liveSocket(page: Page) {
  let socket: WebSocketRoute | undefined
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) => {
    socket = ws
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 0, payload: { 'chat.message': [], 'obs.status': [] } }))
  })
  return async (seq: number) => {
    await expect.poll(() => socket).toBeDefined()
    socket!.send(
      JSON.stringify({
        id: `e${seq}`,
        type: 'checkin.changed',
        version: 1,
        seq,
        occurredAt: '2026-11-16T10:00:00.000Z',
        source: 'twitch',
        payload: { viewer_id: 'v9' },
      }),
    )
  }
}

test('the tile shows today’s count; its drawer lists recent check-ins as text and follows changes', async ({ page }) => {
  await mockAdmin(page, { modules })
  let today: Schemas['CheckinToday'] = {
    business_day: '2026-11-16',
    count: 1,
    recent: [{ viewer_id: 'v1', user_name: HTML, occurred_at: '2026-11-16T10:00:00.000Z' }],
  }
  await page.route('**/api/admin/checkin/today', (r) => r.fulfill({ json: today }))
  const changed = await liveSocket(page)
  await page.goto('/admin/')

  const tile = page.locator('[data-drawer-tile="checkin"]')
  const features = page.getByRole('list', { name: zh.home.groups.features })
  await expect(features.locator('[data-drawer-tile="checkin"]')).toContainText(fill(zh.checkin.today, { n: 1 }))
  await tile.click()
  const drawer = page.locator('#drawer-checkin')
  await expect(drawer.getByText(HTML)).toBeVisible()
  await expect(drawer.locator('img')).toHaveCount(0)
  expect(await page.evaluate(() => '__xss' in window)).toBe(false)

  today = { ...today, count: 2, recent: [{ viewer_id: 'v9', user_name: 'Newest', occurred_at: '2026-11-16T10:05:00.000Z' }, ...today.recent] }
  await changed(1)
  await expect(drawer.getByText('Newest')).toBeVisible()
  await expect(tile).toContainText(fill(zh.checkin.today, { n: 2 }))

  await drawer.getByRole('link', { name: zh.drawer.fullPage }).click()
  await expect(page).toHaveURL(/\/admin\/checkin$/)
  await expect(page.getByRole('heading', { name: zh.modules.names.checkin })).toBeVisible()
})

test('the full page searches, pages, registers and back-fills, and reloads on changes', async ({ page }) => {
  await mockAdmin(page, { modules })
  let total = 120
  const queries: URLSearchParams[] = []
  await page.route('**/api/admin/checkin/viewers?*', (r) => {
    queries.push(new URL(r.request().url()).searchParams)
    return r.fulfill({ json: { items: [viewer(1, { user_name: HTML })], total } })
  })
  const posts: unknown[] = []
  let answer: Schemas['ManualCheckInResult'] = 'checked_in'
  await page.route('**/api/admin/checkin/viewers/v1/check-ins', (r) => {
    posts.push(r.request().postDataJSON())
    return r.fulfill({ json: { result: answer } })
  })
  const changed = await liveSocket(page)
  await page.goto('/admin/checkin')

  await expect(page.getByText(fill(zh.checkin.page.total, { n: 120 }))).toBeVisible()
  const row = page.getByRole('listitem').filter({ hasText: HTML })
  await expect(row).toContainText(fill(zh.checkin.page.card, { stamps: 3, slots: 10 }))
  await expect(row).toContainText(fill(zh.checkin.page.last, { date: '2026-11-15' }))
  await expect(page.locator('main img')).toHaveCount(0)

  await page.getByLabel(zh.checkin.page.search, { exact: true }).fill(' Ann ')
  await page.getByRole('button', { name: zh.redemption.apply, exact: true }).click()
  await expect.poll(() => queries.at(-1)?.get('search')).toBe('Ann')
  await page.getByRole('button', { name: zh.redemption.next }).click()
  await expect.poll(() => queries.at(-1)?.get('offset')).toBe('50')
  expect(queries.at(-1)!.get('search')).toBe('Ann')

  // Today, then a repeat the Host refuses without an error.
  await row.getByRole('button', { name: zh.checkin.page.registerToday }).click()
  await expect.poll(() => posts).toEqual([{ date: null }])
  answer = 'already_checked_in'
  await row.getByRole('button', { name: zh.checkin.page.registerToday }).click()
  await expect(page.getByText(HTML + ' 在', { exact: false }).first()).toBeVisible()

  // Back-fill a past day.
  answer = 'checked_in'
  await page.getByLabel(zh.checkin.page.date).fill('2026-11-01')
  await row.getByRole('button', { name: fill(zh.checkin.page.registerDay, { date: '2026-11-01' }) }).click()
  await expect.poll(() => posts.at(-1)).toEqual({ date: '2026-11-01' })

  const before = queries.length
  total = 121
  await changed(1)
  await expect(page.getByText(fill(zh.checkin.page.total, { n: 121 }))).toBeVisible()
  expect(queries.length).toBeGreaterThan(before)
  expect(queries.at(-1)!.get('offset')).toBe('50')
})

test('the full page has no sideways scroll at 1280, 640 and 320 px', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/checkin/viewers?*', (r) =>
    r.fulfill({ json: { items: [viewer(1, { user_name: 'A'.repeat(60) }), viewer(2)], total: 2 } }),
  )
  await page.goto('/admin/checkin')
  await expect(page.getByText(fill(zh.checkin.page.total, { n: 2 }))).toBeVisible()
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }
})
