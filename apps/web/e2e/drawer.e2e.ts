// AppDrawer in a real browser (docs/dev.md 測試底線): native <dialog>, media queries, Teleport and focus, on the
// harness page (e2e/harness/). Submit and focus-order rules themselves are unit-tested in src/admin/drawer.test.ts.
import { type Page, expect, test } from '@playwright/test'
import type { Controls } from './harness/controls'
import { zh } from './fixtures'

declare global {
  interface Window {
    e2e: Controls
  }
}

const SIDE = { width: 1280, height: 800 }
const MODAL = { width: 800, height: 800 }
const SHEET = { width: 320, height: 640 }

const drawerA = (page: Page) => page.locator('#drawer-a')
const dialog = (page: Page) => page.locator('dialog.drawer-dialog[open]')
const aside = (page: Page) => page.locator('aside.drawer-aside:not([hidden])')
const closeButton = (page: Page) => page.getByRole('button', { name: zh.drawer.close })
const saved = (page: Page) => page.evaluate(() => window.e2e.saved)
const setControls = (page: Page, values: Partial<Controls>) =>
  page.evaluate((values) => Object.assign(window.e2e, values), values)

async function openHarness(page: Page, size: { width: number; height: number }) {
  await page.setViewportSize(size)
  await page.goto('/e2e/harness/index.html')
  await page.getByRole('button', { name: 'Tile A' }).click()
  await expect(drawerA(page)).toBeVisible()
}

test('lays out side by side above 1000px, as a modal up to 1000px and as a bottom sheet below 600px', async ({ page }) => {
  await openHarness(page, SIDE)
  await expect(aside(page)).toBeVisible()
  await expect(dialog(page)).toHaveCount(0)
  expect((await aside(page).boundingBox())!.width).toBe(380)
  // The tiles stay usable beside it.
  await page.getByRole('button', { name: 'Tile B' }).click()
  await expect(page.locator('#drawer-b')).toBeVisible()

  await openHarness(page, MODAL)
  await expect(dialog(page)).toBeVisible()
  const box = (await dialog(page).boundingBox())!
  expect([box.x + box.width, box.width, box.height]).toEqual([MODAL.width, 380, MODAL.height])
  await page.mouse.click(20, 400) // backdrop
  await expect(dialog(page)).toHaveCount(0)
  await page.getByRole('button', { name: 'Tile A' }).click()
  await expect(dialog(page)).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog(page)).toHaveCount(0)

  await openHarness(page, SHEET)
  const sheet = (await dialog(page).boundingBox())!
  expect([sheet.x, sheet.width, sheet.y + sheet.height]).toEqual([0, SHEET.width, SHEET.height])
})

test('crossing 1000px keeps the drawer open with focus, typed value and scroll position', async ({ page }) => {
  await openHarness(page, SIDE)
  const name = page.getByLabel('Name')
  await name.fill('typed, not saved')
  const content = drawerA(page)
  await content.evaluate((el) => (el.scrollTop = 200))

  for (const size of [MODAL, SIDE]) {
    await page.setViewportSize(size)
    await expect(size === MODAL ? dialog(page) : aside(page)).toContainText('Drawer A')
    await expect(name).toBeFocused()
    await expect(name).toHaveValue('typed, not saved')
    await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBe(200)
  }
})

for (const [mode, size] of [
  ['side by side', SIDE],
  ['modal', MODAL],
] as const) {
  test(`${mode}: focus goes to ✕ on open, back to the opener on close, else to its tile`, async ({ page }) => {
    await page.setViewportSize(size)
    await page.goto('/e2e/harness/index.html')
    const pending = page.getByRole('button', { name: 'Pending item' })
    await pending.click()
    await expect(closeButton(page)).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(drawerA(page)).toHaveCount(0)
    await expect(pending).toBeFocused()

    // The opener is gone by the time the drawer closes: its tile gets focus (the native dialog must not win).
    await pending.click()
    await setControls(page, { pending: false })
    await closeButton(page).click()
    await expect(page.getByRole('button', { name: 'Tile A' })).toBeFocused()
  })

  test(`${mode}: a failed save keeps the drawer open, restores the old value and shows the error`, async ({ page }) => {
    await openHarness(page, size)
    await setControls(page, { fail: true })
    const name = page.getByLabel('Name')
    const error = drawerA(page).getByRole('alert')

    // Closing submits the typed value; it fails, so the drawer stays.
    await name.fill('rejected')
    await page.keyboard.press('Escape')
    await expect(error).toBeVisible()
    await expect(name).toHaveValue('old')
    await expect(drawerA(page)).toBeVisible()

    // A failure on blur (nobody waiting for it) also keeps the drawer for the first ✕; the second one closes it.
    await name.fill('rejected on blur')
    await page.keyboard.press('Tab')
    await expect(name).toHaveValue('old')
    await closeButton(page).click()
    await expect(drawerA(page)).toBeVisible()
    // The error and the drawer's buttons are visible and usable, also in the modal.
    await expect(error).toBeInViewport()
    await page.getByRole('button', { name: 'Action' }).click({ trial: true })
    await closeButton(page).click()
    await expect(drawerA(page)).toHaveCount(0)
    expect(await saved(page)).toEqual([])
  })

  test(`${mode}: Esc on an open dropdown closes only the dropdown`, async ({ page }) => {
    await openHarness(page, size)
    await page.locator('#color').click()
    const listbox = page.getByRole('listbox')
    await expect(listbox).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(listbox).toHaveCount(0)
    await expect(drawerA(page)).toBeVisible()
  })
}

// Typing without Enter, then leaving the drawer any way, saves the value exactly once.
const leaves: [string, typeof SIDE, (page: Page) => Promise<unknown>, (page: Page) => Promise<void>][] = [
  ['✕', SIDE, (p) => closeButton(p).click(), closed],
  ['Esc', SIDE, (p) => p.keyboard.press('Escape'), closed],
  ['open full page', SIDE, (p) => p.getByRole('link', { name: zh.drawer.fullPage }).click(), onFullPage],
  ['another tile', SIDE, (p) => p.getByRole('button', { name: 'Tile B' }).click(), (p) => expect(p.locator('#drawer-b')).toBeVisible()],
  ['✕', MODAL, (p) => closeButton(p).click(), closed],
  ['Esc', MODAL, (p) => p.keyboard.press('Escape'), closed],
  ['open full page', MODAL, (p) => p.getByRole('link', { name: zh.drawer.fullPage }).click(), onFullPage],
  ['the backdrop', MODAL, (p) => p.mouse.click(20, 400), closed],
]

async function closed(page: Page) {
  await expect(drawerA(page)).toHaveCount(0)
}

async function onFullPage(page: Page) {
  await expect(page.getByRole('heading', { name: 'Full page' })).toBeVisible()
  await closed(page)
}

for (const [how, size, leave, check] of leaves) {
  test(`${size === SIDE ? 'side by side' : 'modal'}: leaving by ${how} saves the typed value first`, async ({ page }) => {
    await openHarness(page, size)
    await page.getByLabel('Name').fill('typed')
    await leave(page)
    await check(page)
    expect(await saved(page)).toEqual(['typed'])
  })
}
