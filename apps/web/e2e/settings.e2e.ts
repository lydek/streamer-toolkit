// The settings page, a full page (page.ts, FullPage.vue), with the admin API and IPC mocked.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, scrollsSideways, zh } from './fixtures'

const gear = (page: Page) => page.getByRole('button', { name: zh.settings.title, exact: true })
const homeButton = (page: Page) => page.getByRole('button', { name: zh.app.home })
const host = (page: Page) => page.getByLabel(zh.settings.obs.host, { exact: true })
/** vue-router's previous entry: null when home is the first entry, so Back (Alt+←) on home goes nowhere. */
const previousEntry = (page: Page) => page.evaluate(() => (history.state as { back: string | null }).back)

/** Leaves the settings page with "← Home" or the browser's Back (what Alt+← does in the admin window). */
const leave = (page: Page, how: string) => (how === 'home' ? homeButton(page).click() : page.goBack())

for (const how of ['home', 'back']) {
  test(`⚙ beside an open drawer opens settings; ${how} returns there with focus on ⚙`, async ({ page }) => {
    await openAdmin(page)
    await page.locator('[data-drawer-tile="chat"]').click()
    await expect(page.locator('#drawer-chat')).toBeVisible()
    await gear(page).click()
    await expect(page).toHaveURL(/\/admin\/settings$/)
    await expect(homeButton(page)).toBeFocused()

    await leave(page, how)
    await expect(page).toHaveURL(/\/admin\/$/)
    await expect(gear(page)).toBeFocused()
    await expect(page.locator('#drawer-chat')).toHaveCount(0)
    expect(await previousEntry(page)).toBeNull()
  })

  test(`a field typed into without Enter is saved when leaving with ${how}`, async ({ page }) => {
    await openAdmin(page)
    const puts: unknown[] = []
    await page.route('**/api/admin/obs', (r) => {
      if (r.request().method() !== 'PUT') return r.fallback()
      puts.push(r.request().postDataJSON())
      return r.fulfill({ status: 204 })
    })
    await gear(page).click()
    await host(page).fill('obs.local')
    await leave(page, how)
    await expect(page).toHaveURL(/\/admin\/$/)
    expect(puts).toEqual([{ host: 'obs.local', port: 4455 }])
  })
}

test('a save that fails on leaving keeps the settings page with its error', async ({ page }) => {
  await openAdmin(page)
  await page.route('**/api/admin/obs', (r) =>
    r.request().method() === 'PUT' ? r.fulfill({ status: 400, json: { code: 'obs.invalid_address' } }) : r.fallback(),
  )
  await gear(page).click()
  await host(page).fill('bad host')
  // No blur: the router guard's submit is the save that fails, and the router puts the settings entry back.
  await page.goBack()
  await expect(page).toHaveURL(/\/admin\/settings$/)
  await expect(page.getByRole('alert')).toHaveText(zh.errors.obs.invalid_address)
  await expect(homeButton(page)).toBeVisible()
})

test('the settings page reads "Ask before closing" again when the window gets focus back', async ({ page }) => {
  const state = await openAdmin(page)
  await gear(page).click()
  const ask = page.getByLabel(zh.settings.askBeforeClose)
  await expect(ask).toBeChecked()
  // What the close hint's "Don't show again" does to the setting while the window is hidden.
  state.general = { ask_before_close: false }
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await expect(ask).not.toBeChecked()
})

test('turning check-ins off asks to pause or close; Esc keeps them on', async ({ page }) => {
  await openAdmin(page, { modules: [{ id: 'checkin', enabled: true, closed: false }] })
  const puts: unknown[] = []
  await page.route('**/api/admin/modules/checkin', (r) => {
    puts.push(r.request().postDataJSON())
    return r.fulfill({ status: 204 })
  })
  await gear(page).click()
  const dialog = page.getByRole('dialog', { name: zh.checkin.disable.title })
  const toggle = page.locator('#module-checkin')

  await toggle.click()
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('button', { name: zh.checkin.disable.pauseButton })).toBeFocused()
  await expect(dialog.getByText(zh.checkin.disable.closeWarning)).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  expect(puts).toEqual([])

  await toggle.click()
  await dialog.getByRole('button', { name: zh.checkin.disable.pauseButton }).click()
  await expect(dialog).toBeHidden()
  await expect.poll(() => puts).toEqual([{ enabled: false, close: false }])

  await page.setViewportSize({ width: 320, height: 640 })
  await toggle.click()
  await expect(dialog.getByRole('button', { name: zh.checkin.disable.closeButton })).toBeInViewport()
  expect(await scrollsSideways(dialog)).toBe(false)
  await dialog.getByRole('button', { name: zh.checkin.disable.closeButton }).click()
  await expect.poll(() => puts.length).toBe(2)
  expect(puts[1]).toEqual({ enabled: false, close: true })
})
