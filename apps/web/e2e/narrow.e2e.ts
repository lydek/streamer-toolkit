// Phones and narrow windows: the phone layout applies on a phone, and the app bar keeps to two rows, API mocked.
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

/** Opens home at its fullest: Twitch live, OBS streaming and recording while reconnecting (phones show the warning),
 *  and chat docked (340px) wherever the window is wide enough for it. */
async function openLive(page: Page) {
  await mockAdmin(page)
  await page.route('**/api/admin/chat-dock', (r) => r.fulfill({ json: { open: true, width: 340 } }))
  const startedAt = new Date(Date.now() - 3600_000).toISOString()
  const event = (type: string, source: string, payload: unknown) => [{ id: type, type, version: 1, seq: 1, occurredAt: '', source, payload }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [], 'presentation.queue': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [],
          // OBS reconnecting: phones show the connection warning too.
          'obs.status': event('obs.status', 'obs', { state: 'reconnecting' }),
          'obs.state': event('obs.state', 'obs', { scene: 'Live', scenes: [], inputs: [], streaming: true, recording: true }),
          'stream.status': event('stream.status', 'twitch', { state: 'live', startedAt, viewers: 42 }),
        },
      }),
    ),
  )
  await page.goto('/admin/')
  await expect(page.getByRole('banner').locator('.live-pill')).toBeVisible()
}

/** The app bar's top line: its visible items grouped into rows by where they start, each row left to right. */
const barRows = (page: Page) =>
  page.locator('.bar-top').evaluate((top) => {
    const items = [...top.querySelectorAll<HTMLElement>(':scope > .brand, :scope > .bar-status > *, :scope > .bar-tools > *')]
      .filter((e) => e.offsetParent)
      .map((e) => ({ box: e.getBoundingClientRect(), button: e.matches('button'), stop: e.matches('.estop') }))
      .sort((a, b) => a.box.top - b.box.top)
    const rows: (typeof items)[] = []
    for (const item of items) {
      const row = rows.at(-1)
      if (row && item.box.top - row.at(-1)!.box.top <= 12) row.push(item)
      else rows.push([item])
    }
    return rows.map((row) => row.sort((a, b) => a.box.left - b.box.left).map(({ button, stop }) => ({ button, stop })))
  })

test.describe('on a phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })

  test('the phone layout applies: the group tabs are a bottom bar', async ({ page }) => {
    await mockAdmin(page)
    await page.goto('/admin/')
    const tabs = page.getByRole('navigation', { name: zh.nav.label })
    await expect(tabs).toBeVisible()
    // Without a viewport meta tag the phone lays the page out 980px wide and the bottom bar never applies.
    expect(await page.evaluate(() => document.documentElement.clientWidth)).toBe(390)
    const box = (await tabs.boundingBox())!
    expect(Math.round(box.y + box.height)).toBe(844)
  })
})

for (const [width, height, lines] of [
  [1440, 900, 1],
  [1280, 720, 1],
  [1024, 768, 2],
  [800, 800, 2],
  [390, 844, 2],
  [360, 800, 2],
  [320, 800, 2],
]) {
  test(`at ${width}px the app bar takes ${lines} row(s), the tools beside the name and the stop last`, async ({ page }) => {
    await page.setViewportSize({ width, height })
    await openLive(page)
    if (width >= 1000) await expect(page.getByRole('complementary', { name: zh.dock.title })).toBeVisible()
    if (width < 600) await expect(page.getByRole('banner').locator('.problem')).toBeVisible()
    const rows = await barRows(page)
    expect(rows).toHaveLength(lines)
    expect(rows[0].at(-1)?.stop).toBe(true)
    // No button ends up alone on a row.
    for (const row of rows) if (row.some((i) => i.button)) expect(row.length).toBeGreaterThan(1)
    const settings = page.getByRole('banner').getByRole('button', { name: zh.settings.title, exact: true })
    const brand = page.getByRole('banner').locator('.brand')
    const [s, b] = await Promise.all([settings.boundingBox(), brand.boundingBox()])
    // Settings shares the first row with the name instead of falling to a row of its own.
    expect(Math.abs(s!.y + s!.height / 2 - (b!.y + b!.height / 2))).toBeLessThan(12)
  })
}

test('the logo is the solid square at every width, the name beside it from phone width up', async ({ page }) => {
  await mockAdmin(page)
  await page.goto('/admin/')
  const logo = page.getByRole('banner').locator('.brand .logo')
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 800 })
    await expect(logo).toBeVisible()
    const box = (await logo.boundingBox())!
    expect(Math.round(box.width)).toBe(Math.round(box.height))
  }
})

test('the update banner keeps its icon, text and buttons in one row when wide and wraps without sideways scroll at 320px', async ({ page }) => {
  await mockAdmin(page, { update: { current: '0.1.0', available: '0.2.0', check: true, can_check: true } })
  await page.goto('/admin/')
  const banner = page.getByRole('region', { name: zh.update.available.replace('{version}', '0.2.0') })
  await expect(banner).toBeVisible()
  await page.setViewportSize({ width: 1440, height: 800 })
  const tops = async () =>
    banner.evaluate((el) => [...el.children].map((c) => Math.round(c.getBoundingClientRect().top + c.getBoundingClientRect().height / 2)))
  const wide = await tops()
  expect(Math.max(...wide) - Math.min(...wide)).toBeLessThan(12)
  await page.setViewportSize({ width: 320, height: 800 })
  expect(await scrollsSideways(page)).toBe(false)
})
