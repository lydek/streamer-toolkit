// Arrange mode of the admin home (ArrangeTiles.vue) with the admin API mocked. Move order, drag and the focus
// rules themselves are unit-tested in src/admin/ArrangeTiles.test.ts; with the alerts module off only chat shows
// on the home.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, scrollsSideways, zh } from './fixtures'

// The header ✎ shares its name with the empty home's "Arrange tiles" button.
const pencil = (page: Page) => page.locator('header').getByRole('button', { name: zh.home.arrange })
const done = (page: Page) => page.getByRole('button', { name: zh.arrange.done })
const chatTile = (page: Page) => page.locator('[data-drawer-tile="chat"]')
const chatAction = (page: Page, key: 'remove' | 'add') =>
  page.getByRole('button', { name: `${zh.arrange[key]}：${zh.modules.names.chat}` })

test('an emptied home stays empty after a reload, and "Arrange tiles" brings the tile back', async ({ page }) => {
  const state = await openAdmin(page, {
    modules: [
      { id: 'chat', enabled: true, closed: false },
      { id: 'alerts', enabled: false, closed: false },
    ],
  })
  await pencil(page).click()
  await expect(done(page)).toBeFocused()
  await chatAction(page, 'remove').click()
  // Arrange mode lists the alerts tile too, marked as not enabled; focus moves to it.
  const removeAlerts = page.getByRole('button', { name: `${zh.arrange.remove}：${zh.modules.names.alerts}` })
  await expect(removeAlerts).toBeFocused()
  await removeAlerts.click()
  // The redemption, check-in and sound tiles (features group, modules off in this test) go too, so the home ends up
  // empty.
  await expect.poll(() => state.homeTiles).toEqual(['redemption', 'checkin', 'sound'])
  await page.getByRole('button', { name: `${zh.arrange.remove}：${zh.modules.names.redemption}` }).click()
  await expect.poll(() => state.homeTiles).toEqual(['checkin', 'sound'])
  await page.getByRole('button', { name: `${zh.arrange.remove}：${zh.modules.names.checkin}` }).click()
  await expect.poll(() => state.homeTiles).toEqual(['sound'])
  await page.getByRole('button', { name: `${zh.arrange.remove}：${zh.modules.names.sound}` }).click()
  await expect.poll(() => state.homeTiles).toEqual([])
  await expect(page.getByRole('heading', { name: zh.arrange.available })).toBeFocused()
  await done(page).click()
  await expect(pencil(page)).toBeFocused()

  await page.reload()
  const arrange = page.locator('[data-drawer-arrange]')
  await expect(arrange).toBeVisible()
  await expect(chatTile(page)).toHaveCount(0)
  await arrange.click()
  await expect(done(page)).toBeFocused()
  await chatAction(page, 'add').click()
  await expect.poll(() => state.homeTiles).toEqual(['chat'])
  await expect(chatAction(page, 'remove')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(chatTile(page)).toBeVisible()
  await expect(pencil(page)).toBeFocused()
})

for (const width of [1280, 800]) {
  test(`at ${width}px Esc first closes a drawer opened in arrange mode, then leaves arrange mode`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    await openAdmin(page)
    await pencil(page).click()
    const obs = page.getByRole('button', { name: /^OBS：/ })
    await obs.click()
    await expect(page.locator('#drawer-connections')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('#drawer-connections')).toHaveCount(0)
    await expect(obs).toBeFocused()
    await expect(done(page)).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(done(page)).toHaveCount(0)
    await expect(pencil(page)).toBeFocused()
  })
}

test('arrange mode never scrolls sideways at 1280px, 640px and 320px', async ({ page }) => {
  await openAdmin(page)
  await pencil(page).click()
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
    await expect(chatAction(page, 'remove')).toBeInViewport()
  }
})

test('a failed save puts the tile back and shows the error', async ({ page }) => {
  const state = await openAdmin(page)
  await page.route('**/api/admin/home-tiles', (r) =>
    r.request().method() === 'PUT' ? r.fulfill({ status: 500, json: { code: 'settings.save_failed' } }) : r.fallback(),
  )
  await pencil(page).click()
  await chatAction(page, 'remove').click()
  await expect(page.getByText(zh.errors.settings.save_failed)).toBeVisible()
  await expect(chatAction(page, 'remove')).toBeFocused()
  expect(state.homeTiles).toBeNull()
})
