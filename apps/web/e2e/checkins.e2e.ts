// Check-ins: the home tile and drawer, and the full page's search, pages, manual check-ins and live refresh, with the
// admin API and `/ws/admin` mocked (the rules themselves are tested on real SQLite in crates/modules/checkin).
import { type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, scrollsSideways, toLiveHome, zh } from './fixtures'

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

/** `/ws/admin` with a handle to push `checkin.changed` later. */
async function liveSocket(page: Page) {
  let socket: WebSocketRoute | undefined
  await page.routeWebSocket(/\/ws\/admin$/, (ws) => {
    socket = ws
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 0, payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] } }))
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
    full_today: 0,
    recent: [{ viewer_id: 'v1', user_name: HTML, occurred_at: '2026-11-16T10:00:00.000Z', stamps: 3, card_slots: 10 }],
  }
  await page.route('**/api/admin/checkin/today', (r) => r.fulfill({ json: today }))
  const changed = await liveSocket(page)
  await page.goto('/admin/')
  await toLiveHome(page)

  const tile = page.locator('[data-drawer-tile="checkin"]')
  const features = page.getByRole('region', { name: zh.home.tiles })
  await expect(features.locator('[data-drawer-tile="checkin"]')).toContainText(`1${zh.home.units.checkin}`)
  await tile.click()
  const drawer = page.locator('#drawer-checkin')
  await expect(drawer.getByText(HTML)).toBeVisible()
  await expect(drawer.locator('img')).toHaveCount(0)
  expect(await page.evaluate(() => '__xss' in window)).toBe(false)

  today = { ...today, count: 2, recent: [{ viewer_id: 'v9', user_name: 'Newest', occurred_at: '2026-11-16T10:05:00.000Z', stamps: 1, card_slots: 10 }, ...today.recent] }
  await changed(1)
  await expect(drawer.getByText('Newest')).toBeVisible()
  await expect(tile).toContainText(`2${zh.home.units.checkin}`)

  await drawer.getByRole('link', { name: zh.drawer.fullPage }).click()
  await expect(page).toHaveURL(/\/admin\/checkin$/)
  await expect(page.getByRole('heading', { name: zh.modules.names.checkin, exact: true })).toBeVisible()
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
  const row = page.getByRole('row').filter({ hasText: HTML })
  await expect(row).toContainText('3/10')
  await expect(row).toContainText('2026-11-15')
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

test('importing previews the file, says what is skipped and why, then imports the same content', async ({ page }) => {
  await mockAdmin(page, { modules })
  let listed = 0
  await page.route('**/api/admin/checkin/viewers?*', (r) => {
    listed++
    return r.fulfill({ json: { items: [], total: 0 } })
  })
  const bodies: Record<string, unknown[]> = { preview: [], apply: [] }
  const summary = { added: 1, updated: 2, skipped: [{ key: HTML, reason: 'invalid_id' }] }
  await page.route('**/api/admin/checkin/import/preview', (r) => {
    bodies.preview.push(r.request().postDataJSON())
    return r.fulfill({ json: summary })
  })
  await page.route('**/api/admin/checkin/import', (r) => {
    bodies.apply.push(r.request().postDataJSON())
    return r.fulfill({ json: summary })
  })
  await page.goto('/admin/checkin')
  await expect(page.getByText(zh.checkin.page.empty)).toBeVisible()

  // The import is a dialog, fresh each time it opens.
  const opener = page.getByRole('button', { name: zh.checkin.import.heading })
  await opener.click()
  const file = page.locator('#checkin-import-file')
  await file.setInputFiles({ name: 'bad.json', mimeType: 'application/json', buffer: Buffer.from('{oops') })
  await expect(page.getByText(zh.errors.checkin.import_invalid)).toBeVisible()
  expect(bodies.preview).toEqual([])

  const content = { '123': { totalCheckIns: 12, lastName: 'Ann' }, [HTML]: { totalCheckIns: 1 } }
  await file.setInputFiles({ name: 'checkins.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(content)) })
  await expect(page.getByText(fill(zh.checkin.import.summary, { added: 1, updated: 2, skipped: 1 }))).toBeVisible()
  await expect(page.getByText(`${HTML}：${zh.checkin.import.reasons.invalid_id}`)).toBeVisible()
  await expect(page.locator('main img')).toHaveCount(0)
  expect(bodies.preview).toEqual([content])

  const before = listed
  await page.getByRole('button', { name: zh.checkin.import.confirm }).click()
  await expect(page.getByText(fill(zh.checkin.import.done, { added: 1, updated: 2 }))).toBeVisible()
  expect(bodies.apply).toEqual([content])
  await expect.poll(() => listed).toBeGreaterThan(before)
  await expect(page.getByRole('button', { name: zh.checkin.import.confirm })).toHaveCount(0)
  // Done, the dialog closes and focus goes back to the button that opened it.
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(opener).toBeFocused()
})

test('異動紀錄 opens from the header and gives focus back when it closes', async ({ page }) => {
  await mockAdmin(page, { modules: [{ id: 'checkin', enabled: true, closed: false }] })
  await page.route('**/api/admin/checkin/viewers*', (r) => r.fulfill({ json: { items: [viewer(1)], total: 1 } }))
  await page.route('**/api/admin/checkin/deductions', (r) =>
    r.fulfill({ json: [{ id: 'd1', viewer_id: 'v1', user_name: 'Viewer1', amount: 2, reason: null, deducted_at: '2026-11-20T12:00:00.000Z' }] }),
  )
  await page.goto('/admin/checkin')
  const open = page.getByRole('button', { name: zh.checkin.deduct.log })
  await open.focus()
  await page.keyboard.press('Enter')
  const dialog = page.getByRole('dialog', { name: zh.checkin.deduct.log })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText(fill(zh.checkin.deduct.logLine, { name: 'Viewer1', n: 2 }))
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(open).toBeFocused()
})

test('the card dots keep ten to a row on a narrow window', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/checkin/viewers*', (r) => r.fulfill({ json: { items: [viewer(1, { stamps: 3, card_slots: 10 })], total: 1 } }))
  for (const width of [800, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/admin/checkin')
    const dots = page.locator('.dots i')
    await expect(dots).toHaveCount(10)
    const tops = new Set(await dots.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().top))))
    expect(tops.size, `${width}px`).toBe(1)
  }
})

test('the drawer lists redemptions that kept failing, retries or skips them as in settings, and shows avatars and cards', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/checkin/today', (r) =>
    r.fulfill({
      json: {
        business_day: '2026-11-16',
        count: 1,
        full_today: 1,
        recent: [{ viewer_id: 'v1', user_name: '阿凱', occurred_at: '2026-11-16T10:00:00.000Z', stamps: 10, card_slots: 10 }],
      },
    }),
  )
  let dead = [{ event_id: 'e1', occurred_at: '2026-11-16T09:00:00.000Z', user_name: '小魚', reward_title: '每日打卡', error: 'database is locked' }]
  await page.route('**/api/admin/checkin/outbox', (r) => r.fulfill({ json: { pending: 0, oldest_pending_at: null, dead_letters: dead } }))
  const acts: string[] = []
  await page.route('**/api/admin/checkin/outbox/*/*', (r) => {
    acts.push(new URL(r.request().url()).pathname)
    dead = []
    return r.fulfill({ status: 204 })
  })
  await liveSocket(page)
  await page.goto('/admin/')
  await toLiveHome(page)
  await page.locator('[data-drawer-tile="checkin"]').click()
  const drawer = page.locator('#drawer-checkin')
  await expect(drawer.locator('.recent-checkins .avatar')).toHaveText('阿')
  await expect(drawer.locator('.recent-checkins li')).toContainText('10/10')
  await expect(drawer.getByText(zh.checkin.fullToday.replace('{n}', '1'))).toBeVisible()
  await expect(drawer.getByText('database is locked')).toBeVisible()
  await drawer.getByRole('button', { name: zh.checkin.outbox.retry }).click()
  await expect.poll(() => acts).toEqual(['/api/admin/checkin/outbox/e1/retry'])
  await expect(drawer.getByText('database is locked')).toHaveCount(0)
  await expect(drawer.getByRole('heading', { name: zh.checkin.outbox.deadLetters })).toHaveCount(0)
})

test('home 待處理 shows check-in redemptions that kept failing, without opening the drawer, and follows them', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/checkin/today', (r) => r.fulfill({ json: { business_day: '2026-11-16', count: 0, full_today: 0, recent: [] } }))
  const letter = (id: string) => ({ event_id: id, occurred_at: '2026-11-16T09:00:00.000Z', user_name: '小魚', reward_title: '每日打卡', error: 'database is locked' })
  let dead = [letter('e1'), letter('e2')]
  await page.route('**/api/admin/checkin/outbox', (r) => r.fulfill({ json: { pending: 0, oldest_pending_at: null, dead_letters: dead } }))
  await page.route('**/api/admin/checkin/outbox/*/retry', (r) => {
    dead = dead.filter((d) => !r.request().url().includes(`/${d.event_id}/`))
    return r.fulfill({ status: 204 })
  })
  await liveSocket(page)
  await page.goto('/admin/')
  await toLiveHome(page)
  const item = (n: number) => page.getByRole('button', { name: zh.home.pendingCheckin.replace('{n}', String(n)) })
  await expect(item(2)).toBeVisible()
  // The drawer is closed: the count still follows the outbox.
  dead = [letter('e1')]
  await expect(item(1)).toBeVisible()

  // Still there after closing: focus goes back to it.
  await item(1).click()
  const drawer = page.locator('#drawer-checkin')
  await expect(drawer).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(item(1)).toBeFocused()

  // Handled: the item goes, and closing the drawer puts focus on the check-in tile instead.
  await item(1).click()
  await drawer.getByRole('button', { name: zh.checkin.outbox.retry }).click()
  await expect(item(1)).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(page.locator('[data-drawer-tile="checkin"]')).toBeFocused()
})

test('home 待處理 says nothing about check-ins while the check-in module is off', async ({ page }) => {
  await mockAdmin(page, { modules: [{ id: 'checkin', enabled: false, closed: false }] })
  const reads: string[] = []
  await page.route('**/api/admin/checkin/outbox', (r) => {
    reads.push('outbox')
    return r.fulfill({ json: { pending: 0, oldest_pending_at: null, dead_letters: [{ event_id: 'e1', occurred_at: '2026-11-16T09:00:00.000Z', user_name: 'x', reward_title: 'y', error: 'z' }] } })
  })
  await page.goto('/admin/')
  await toLiveHome(page)
  await expect(page.getByText(zh.home.pendingCheckin.replace('{n}', '1'))).toHaveCount(0)
  expect(reads).toEqual([])
})
