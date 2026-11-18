// The app bar: sticky top, group tabs, sub-tabs and "More" (docs/decisions.md 管理 UI 版面), with the API mocked.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'

const tabs = (page: Page) => page.getByRole('navigation', { name: zh.nav.label })
const tab = (page: Page, group: string) => tabs(page).getByRole('link', { name: zh.nav.groups[group] })
const subTab = (page: Page, name: string) => page.getByRole('navigation', { name: zh.nav.sub }).getByRole('link', { name })
const more = (page: Page) => tabs(page).getByRole('button', { name: zh.nav.more })

test('every group and sub-page is one click away, and the current one is marked', async ({ page }) => {
  await openAdmin(page)
  await expect(tab(page, 'home')).toHaveAttribute('aria-current', 'page')
  await expect(page.getByRole('navigation', { name: zh.nav.sub })).toHaveCount(0)

  await tab(page, 'chat').click()
  await expect(page).toHaveURL(/\/admin\/chat$/)
  await expect(tab(page, 'chat')).toHaveAttribute('aria-current', 'page')

  await tab(page, 'interact').click()
  await expect(page).toHaveURL(/\/admin\/alerts$/)
  await expect(tab(page, 'interact')).toHaveAttribute('aria-current', 'true')
  await expect(subTab(page, zh.modules.names.alerts)).toHaveAttribute('aria-current', 'page')

  await subTab(page, zh.modules.names.checkin).click()
  await expect(page).toHaveURL(/\/admin\/checkin$/)
  await subTab(page, zh.modules.names.redemption).click()
  await expect(page).toHaveURL(/\/admin\/redemptions$/)

  // A group tab goes back to the sub-page last seen in it.
  await tab(page, 'media').click()
  await expect(page).toHaveURL(/\/admin\/sounds$/)
  await tab(page, 'interact').click()
  await expect(page).toHaveURL(/\/admin\/redemptions$/)

  await tab(page, 'home').click()
  await expect(page).toHaveURL(/\/admin\/$/)
})

test('tabs work from the keyboard', async ({ page }) => {
  await openAdmin(page)
  await tab(page, 'chat').focus()
  await page.keyboard.press('Enter')
  await expect(page).toHaveURL(/\/admin\/chat$/)
})

test('the app bar stays in view while the page scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 360 })
  await openAdmin(page)
  await page.getByRole('button', { name: zh.settings.title, exact: true }).click()
  await expect(page).toHaveURL(/\/admin\/settings$/)
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight))
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(0)
  await expect(tabs(page)).toBeInViewport()
  await expect(page.getByRole('button', { name: /^OBS：/ })).toBeInViewport()
})

test('"More" shows only when the groups do not fit, and holds the ones that do not', async ({ page }) => {
  await openAdmin(page)
  await expect(more(page)).toBeHidden()

  await page.setViewportSize({ width: 320, height: 640 })
  await expect(more(page)).toBeVisible()
  const visible = await tabs(page).getByRole('link').count()
  await more(page).click()
  const items = page.locator('#tabs-more').getByRole('menuitem')
  expect(visible + (await items.count())).toBe(Object.keys(zh.nav.groups).length)
  await items.last().click()
  await expect(page).toHaveURL(/\/admin\/sounds$/)
  await expect(more(page)).toHaveClass(/current/)
})

test('away from home, a status light goes to its section of settings', async ({ page }) => {
  await openAdmin(page)
  await tab(page, 'chat').click()
  await page.getByRole('button', { name: /^OBS：/ }).click()
  await expect(page).toHaveURL(/\/admin\/settings#obs$/)
  await expect(page.getByRole('heading', { name: 'OBS' })).toBeFocused()
})
