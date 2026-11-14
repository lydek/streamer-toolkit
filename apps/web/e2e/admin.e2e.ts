// The admin home in a real browser with the admin API and IPC mocked (docs/dev.md 測試底線).
import { type Page, expect, test } from '@playwright/test'
import { emit, invokes, openAdmin, scrollsSideways, zh } from './fixtures'

const chatTile = (page: Page) => page.locator('[data-drawer-tile="chat"]')

test('never scrolls sideways at 1280px, 640px and 320px, with the drawer closed or open', async ({ page }) => {
  await openAdmin(page)
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px, closed`).toBe(false)
    await chatTile(page).click()
    await expect(page.locator('#drawer-chat')).toBeVisible()
    expect(await scrollsSideways(page), `${width}px, open`).toBe(false)
    await page.keyboard.press('Escape')
    await expect(page.locator('#drawer-chat')).toHaveCount(0)
  }
})

for (const width of [1280, 800]) {
  test(`keyboard only at ${width}px: skip link, tile, drawer, simulate, close`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    await openAdmin(page)
    await page.route('**/api/admin/simulate/chat', (r) => r.fulfill({ status: 204 }))

    await page.keyboard.press('Tab')
    const skip = page.getByRole('link', { name: zh.home.skip })
    await expect(skip).toBeFocused()
    await expect(skip).toBeInViewport()
    await page.keyboard.press('Enter')
    await page.keyboard.press('Tab')
    await expect(chatTile(page)).toBeFocused()

    await page.keyboard.press('Enter')
    await expect(page.getByRole('button', { name: zh.drawer.close })).toBeFocused()
    await page.keyboard.press('Tab')
    await expect(page.getByLabel(zh.simulate.userName)).toBeFocused()
    await page.keyboard.press('Tab')
    await page.keyboard.type('hello from the keyboard')
    const posted = page.waitForRequest('**/api/admin/simulate/chat')
    await page.keyboard.press('Enter')
    expect((await posted).postDataJSON()).toEqual({ userName: '', text: 'hello from the keyboard' })
    await expect(page.getByText(zh.simulate.sent)).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(page.locator('#drawer-chat')).toHaveCount(0)
    await expect(chatTile(page)).toBeFocused()
  })
}

for (const width of [1280, 800]) {
  test(`a status light at ${width}px opens the connections drawer and gets focus back`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    await openAdmin(page)
    const obs = page.getByRole('button', { name: /^OBS：/ })
    await obs.click()
    await expect(page.locator('#drawer-connections')).toBeVisible()
    await expect(obs).toHaveAttribute('aria-expanded', 'true')
    await page.getByRole('button', { name: zh.drawer.close }).click()
    await expect(page.locator('#drawer-connections')).toHaveCount(0)
    await expect(obs).toBeFocused()
  })
}

test('a bottom-sheet drawer shows a failed action inside it, with its buttons in reach', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 })
  await openAdmin(page)
  await page.route('**/api/admin/simulate/chat', (r) =>
    r.fulfill({ status: 400, json: { code: 'simulate.invalid_text' } }),
  )
  await chatTile(page).click()
  const sheet = page.locator('dialog.drawer-dialog[open]')
  await sheet.getByLabel(zh.simulate.text).fill('x')
  const send = sheet.getByRole('button', { name: zh.simulate.send })
  await send.click()
  await expect(sheet.getByRole('alert')).toBeInViewport()
  await expect(send).toBeInViewport()
  await expect(sheet.getByRole('button', { name: zh.drawer.close })).toBeInViewport()
})

test('close hint: Esc cancels, "Got it" hides, "Don\'t show again" is sent', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 })
  await openAdmin(page)
  const hint = page.getByRole('dialog', { name: zh.closeHint.message })
  const ok = hint.getByRole('button', { name: zh.closeHint.ok })
  const dontAsk = hint.getByLabel(zh.closeHint.dontAskAgain)
  const actions = async () => (await invokes(page, 'close_hint')).map((i) => i.args.action)

  // The listener is registered after mount; the event only reaches the page once it is.
  await expect.poll(async () => (await invokes(page, 'plugin:event|listen')).length).toBeGreaterThan(0)
  await emit(page, 'close-hint', true)
  await expect(hint).toBeInViewport()
  await expect(ok).toBeFocused()
  expect(await scrollsSideways(page)).toBe(false)
  await page.keyboard.press('Escape')
  await expect(hint).toBeHidden()
  await expect.poll(actions).toEqual(['shown', 'cancel'])

  await emit(page, 'close-hint', true)
  await ok.click()
  await expect(hint).toBeHidden()
  await expect.poll(actions).toEqual(['shown', 'cancel', 'shown', { hide: { dont_ask_again: false } }])

  // The choice starts unchecked each time the hint opens.
  await emit(page, 'close-hint', true)
  await expect(dontAsk).not.toBeChecked()
  await hint.getByText(zh.closeHint.dontAskAgain).click()
  await expect(dontAsk).toBeChecked()
  await ok.click()
  await expect.poll(async () => (await actions()).at(-1)).toEqual({ hide: { dont_ask_again: true } })
})
