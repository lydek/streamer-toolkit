// Text size in settings › general: dragging previews, letting go applies and saves (API mocked).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

/** Mocks the saved text size `saved`; returns the bodies of every save. */
async function mockTextSize(page: Page, saved: number | null = null) {
  const puts: unknown[] = []
  await page.route('**/api/admin/text-size', (r) => {
    if (r.request().method() === 'GET') return r.fulfill({ json: saved })
    saved = r.request().postDataJSON()
    puts.push(saved)
    return r.fulfill({ status: 204 })
  })
  return puts
}
const rootSize = (page: Page) => page.evaluate(() => document.documentElement.style.fontSize)

test('dragging only changes the preview; letting go applies it to the window and saves it', async ({ page }) => {
  await mockAdmin(page)
  const puts = await mockTextSize(page)
  await page.goto('/admin/settings/general')
  const slider = page.getByRole('slider', { name: zh.settings.textSize })
  const preview = page.locator('#text-size-preview')
  const before = await preview.evaluate((el) => getComputedStyle(el).fontSize)

  // While dragging the browser sends input events only.
  await slider.evaluate((el: HTMLInputElement) => {
    el.value = '150'
    el.dispatchEvent(new Event('input', { bubbles: true }))
  })
  await expect.poll(() => preview.evaluate((el) => getComputedStyle(el).fontSize)).not.toBe(before)
  expect(await rootSize(page)).toBe('')
  expect(puts).toEqual([])

  // Letting go sends change.
  await slider.evaluate((el: HTMLInputElement) => el.dispatchEvent(new Event('change', { bubbles: true })))
  await expect.poll(() => puts).toEqual([150])
  await expect.poll(() => rootSize(page)).toBe('150%')
})

test('the saved size applies on start', async ({ page }) => {
  await mockAdmin(page)
  await mockTextSize(page, 130)
  await page.goto('/admin/')
  await expect.poll(() => rootSize(page)).toBe('130%')
})

test('at 150% no page scrolls sideways at 1280px', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mockAdmin(page)
  await mockTextSize(page, 150)
  for (const path of ['/', '/chat', '/alerts', '/checkin', '/redemptions', '/sounds', '/songs', '/settings/general', '/settings/features']) {
    await page.goto(`/admin${path}`)
    await expect.poll(() => rootSize(page)).toBe('150%')
    expect(await scrollsSideways(page), path).toBe(false)
  }
})
