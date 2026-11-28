// Phones and narrow windows: the phone layout applies on a phone, and the app bar keeps to two rows, API mocked.
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

/** Opens home with Twitch live and OBS streaming and recording, so every status pill shows. */
async function openLive(page: Page) {
  await mockAdmin(page)
  const startedAt = new Date(Date.now() - 3600_000).toISOString()
  const event = (type: string, source: string, payload: unknown) => [{ id: type, type, version: 1, seq: 1, occurredAt: '', source, payload }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [], 'obs.status': [], 'presentation.queue': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [],
          'obs.state': event('obs.state', 'obs', { scene: 'Live', scenes: [], inputs: [], streaming: true, recording: true }),
          'stream.status': event('stream.status', 'twitch', { state: 'live', startedAt, viewers: 42 }),
        },
      }),
    ),
  )
  await page.goto('/admin/')
  await expect(page.getByRole('banner').locator('.live-pill')).toBeVisible()
}

/** How many rows the app bar's top line takes: its visible items grouped by where they start. */
const barRows = (page: Page) =>
  page.locator('.bar-top').evaluate((top) => {
    const items = [...top.querySelectorAll(':scope > .brand, :scope > .bar-status > *, :scope > .bar-tools > *')]
    const tops = items.filter((e) => (e as HTMLElement).offsetParent).map((e) => e.getBoundingClientRect().top)
    return tops.sort((a, b) => a - b).filter((t, i, all) => i === 0 || t - all[i - 1] > 12).length
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

for (const width of [800, 390, 320]) {
  test(`at ${width}px the app bar keeps to two rows, the tools beside the name`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    await openLive(page)
    expect(await barRows(page)).toBeLessThanOrEqual(2)
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
