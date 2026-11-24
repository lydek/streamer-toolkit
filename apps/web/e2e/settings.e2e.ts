// The settings page, a full page (page.ts, FullPage.vue), with the admin API and IPC mocked.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, scrollsSideways, zh } from './fixtures'

const gear = (page: Page) => page.getByRole('button', { name: zh.settings.title, exact: true })
const homeTab = (page: Page) =>
  page.getByRole('navigation', { name: zh.nav.label }).getByRole('link', { name: zh.nav.groups.home })
const title = (page: Page) => page.getByRole('heading', { level: 1, name: zh.settings.title })
const host = (page: Page) => page.getByLabel(zh.settings.obs.host, { exact: true })
const editObs = (page: Page) => page.getByRole('button', { name: zh.settings.obs.edit, exact: true })
const section = (page: Page, name: string) => page.getByRole('navigation', { name: zh.settings.sections }).getByRole('link', { name })
/** vue-router's previous entry: null when home is the first entry, so Back (Alt+←) on home goes nowhere. */
const previousEntry = (page: Page) => page.evaluate(() => (history.state as { back: string | null }).back)

/** Leaves the settings page with the 首頁 tab or the browser's Back (what Alt+← does in the admin window). */
const leave = (page: Page, how: string) => (how === 'tab' ? homeTab(page).click() : page.goBack())

for (const how of ['tab', 'back']) {
  test(`⚙ beside an open drawer opens settings; ${how} returns there with focus on ⚙`, async ({ page }) => {
    await openAdmin(page)
    await page.locator('[data-drawer-tile="chat"]').click()
    await expect(page.locator('#drawer-chat')).toBeVisible()
    await gear(page).click()
    await expect(page).toHaveURL(/\/admin\/settings\/general$/)
    await expect(title(page)).toBeFocused()

    await leave(page, how)
    await expect(page).toHaveURL(/\/admin\/$/)
    await expect(gear(page)).toBeFocused()
    await expect(page.locator('#drawer-chat')).toHaveCount(0)
    // Back returns to the entry before, so history does not grow.
    if (how === 'back') expect(await previousEntry(page)).toBeNull()
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
    await section(page, zh.settings.menu.connections).click()
    await editObs(page).click()
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
  await section(page, zh.settings.menu.connections).click()
  await editObs(page).click()
  await host(page).fill('bad host')
  // No blur: the router guard's submit is the save that fails, and the router puts the settings entry back.
  await page.goBack()
  await expect(page).toHaveURL(/\/admin\/settings\/connections$/)
  await expect(page.getByRole('alert')).toHaveText(zh.errors.obs.invalid_address)
  await expect(title(page)).toBeVisible()
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
  await section(page, zh.settings.menu.features).click()
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

test('every settings section is in the menu and has its own URL; later ones say they are not available', async ({ page }) => {
  await openAdmin(page)
  await gear(page).click()
  for (const s of Object.keys(zh.settings.menu)) {
    await section(page, zh.settings.menu[s]).click()
    await expect(page).toHaveURL(new RegExp(`/admin/settings/${s}$`))
    await expect(section(page, zh.settings.menu[s])).toHaveAttribute('aria-current', 'page')
  }
  await page.goto('/admin/settings/hotkeys')
  await expect(page.getByRole('heading', { level: 2, name: zh.settings.menu.hotkeys })).toBeVisible()
  // Every later section is mounted; only the shown one is visible.
  await expect(page.getByText(zh.pages.soon).filter({ visible: true })).toHaveCount(1)
  await page.goto('/admin/settings/connections')
  await expect(editObs(page)).toBeVisible()
  await expect(page.getByLabel(zh.settings.askBeforeClose)).toBeHidden()
})

test('switching sections saves a field still being edited, and a failed save keeps the section', async ({ page }) => {
  await openAdmin(page)
  await page.route('**/api/admin/obs', (r) =>
    r.request().method() === 'PUT' ? r.fulfill({ status: 400, json: { code: 'obs.invalid_address' } }) : r.fallback(),
  )
  await page.goto('/admin/settings/connections')
  await editObs(page).click()
  await host(page).fill('bad host')
  await section(page, zh.settings.menu.general).click()
  await expect(page).toHaveURL(/\/admin\/settings\/connections$/)
  await expect(page.getByRole('alert')).toHaveText(zh.errors.obs.invalid_address)
})

test("check-in's row in features links to its rules page, which holds its settings", async ({ page }) => {
  await openAdmin(page, { modules: [{ id: 'checkin', enabled: true, closed: false }] })
  await page.goto('/admin/settings/features')
  await page.getByRole('link', { name: `${zh.features.rules}：${zh.features.checkin.name}` }).click()
  await expect(page).toHaveURL(/\/admin\/checkin-rules$/)
  await expect(page.getByRole('combobox', { name: zh.sound.checkinNormal })).toBeVisible()
})

test('the settings sections never scroll sideways at 1280px, 820px and 390px', async ({ page }) => {
  await openAdmin(page)
  for (const width of [1280, 820, 390]) {
    await page.setViewportSize({ width, height: 800 })
    for (const s of Object.keys(zh.settings.menu)) {
      await page.goto(`/admin/settings/${s}`)
      expect(await scrollsSideways(page), `${s} at ${width}px`).toBe(false)
    }
  }
})
