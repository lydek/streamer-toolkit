// Every admin page reflows to the content width without scrolling sideways (WCAG 1.4.10), API mocked.
import { expect, test } from '@playwright/test'
import { emit, invokes, mockAdmin, openAdmin, scrollsSideways, zh } from './fixtures'

const ROUTES = ['/', '/chat-log', '/chat', '/alerts', '/checkin', '/redemptions', '/sounds', '/settings', '/music', '/counters', '/scenes']
const SIZES = [
  [1600, 900],
  [1280, 800],
  [1024, 768],
  [820, 1180],
  [390, 844],
  [360, 700],
] as const

for (const path of ROUTES) {
  test(`${path} never scrolls sideways from 1600px down to 360px`, async ({ page }) => {
    await mockAdmin(page)
    await page.goto(`/admin${path}`)
    await page.locator('main h1').first().waitFor({ state: 'attached' })
    for (const [width, height] of SIZES) {
      await page.setViewportSize({ width, height })
      await expect.poll(() => scrollsSideways(page), `${path} at ${width}px`).toBe(false)
    }
  })
}

test('a narrow redemption page folds its filters behind a toggle, and long names wrap', async ({ page }) => {
  await mockAdmin(page)
  const long = 'Averyveryverylongviewername_without_any_break_at_all'
  await page.route('**/api/admin/redemptions*', (r) =>
    r.fulfill({
      json: {
        total: 1,
        failed_writes: 0,
        items: [
          {
            id: 'r1',
            user_login: long.toLowerCase(),
            user_name: long,
            reward_title: 'A reward title that is long enough to wrap over two lines on a phone',
            reward_cost: 100000,
            user_input: 'x'.repeat(120),
            status: 'unfulfilled',
            redeemed_at: '2026-11-16T10:00:00.000Z',
            updated_at: '2026-11-16T10:00:00.000Z',
          },
        ],
      },
    }),
  )
  await page.goto('/admin/redemptions')
  const toggle = page.getByRole('button', { name: zh.redemption.filtersToggle })
  const form = page.getByRole('form', { name: zh.redemption.filters })
  await expect(toggle).toBeHidden()
  await expect(form).toBeVisible()

  await page.setViewportSize({ width: 360, height: 700 })
  await expect(page.getByText(long)).toBeVisible()
  expect(await scrollsSideways(page)).toBe(false)
  await expect(form).toBeHidden()
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')
  await toggle.click()
  await expect(toggle).toHaveAttribute('aria-expanded', 'true')
  await expect(form).toBeVisible()
  expect(await scrollsSideways(page)).toBe(false)
})

test('below 600px a dialog is a full-width sheet at the bottom', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 700 })
  await openAdmin(page)
  const hint = page.getByRole('dialog', { name: zh.closeHint.message })
  // The listener is registered after mount; the event only reaches the page once it is.
  await expect.poll(async () => (await invokes(page, 'plugin:event|listen')).length).toBeGreaterThan(0)
  await emit(page, 'close-hint', true)
  await expect(hint).toBeVisible()
  // Measured once the slide-up has finished.
  await expect.poll(async () => { const b = (await hint.boundingBox())!; return [Math.round(b.width), Math.round(b.y + b.height)] }).toEqual([360, 700])
  const ok = hint.getByRole('button', { name: zh.closeHint.ok })
  expect((await ok.boundingBox())!.height).toBeGreaterThanOrEqual(44)
})
