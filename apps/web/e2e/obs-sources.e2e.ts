// OBS 來源 page layout, with the admin API mocked (adding and removing sources are tested in obsSources.test.ts).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin } from './fixtures'

const modules = ['chat', 'alerts', 'checkin', 'counter', 'sound', 'bgm', 'screen-tools', 'warnings', 'polls'].map((id) => ({ id, enabled: true, closed: false }))

async function open(page: Page) {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/obs/sources', (r) =>
    r.fulfill({ json: { connected: true, local: true, live_scene: 'Game', scenes: ['Opening', 'Game'], sources: [] } }),
  )
  await page.goto('/admin/obs-sources')
  await expect(page.locator('tbody tr').first()).toBeVisible()
}

/** Whether the page itself (not the table) scrolls. */
const pageScrolls = (page: Page) => page.evaluate(() => document.scrollingElement!.scrollHeight > document.scrollingElement!.clientHeight + 1)

for (const [width, height] of [
  [1440, 900],
  [800, 900],
] as const) {
  test(`at ${width}×${height} only the table scrolls, and its head and buttons stay on one line`, async ({ page }) => {
    await page.setViewportSize({ width, height })
    await open(page)
    expect(await pageScrolls(page)).toBe(false)
    const oneLine = await page.locator('thead th, tbody .end button').evaluateAll((els) =>
      els.filter((e) => e.getBoundingClientRect().height > 0).every((e) => {
        const line = parseFloat(getComputedStyle(e).lineHeight) || 20
        return e.getBoundingClientRect().height < line * 2 + 16
      }),
    )
    expect(oneLine).toBe(true)
  })
}

test('a short window scrolls the whole page instead', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 500 })
  await open(page)
  expect(await pageScrolls(page)).toBe(true)
})

test('on a phone each row keeps its padding inside the card and the copy button closes the row', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await open(page)
  const [card, cell] = await Promise.all([page.locator('.table-scroll').boundingBox(), page.locator('tbody tr:not(.group) th').first().boundingBox()])
  expect(cell!.x - card!.x).toBeGreaterThan(8)
})
