// The chat full page's template picker, with the admin API and IPC mocked (the overlay side is in overlay.e2e.ts).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

const option = (page: Page, id: keyof typeof zh.preview.templates) =>
  page.getByRole('radio', { name: zh.preview.templates[id] })

async function openChatPage(page: Page) {
  const s = await mockAdmin(page)
  const puts: unknown[] = []
  page.on('request', (r) => {
    if (r.method() === 'PUT' && r.url().endsWith('/api/admin/templates/chat')) puts.push(r.postDataJSON())
  })
  await page.goto('/admin/chat')
  await expect(option(page, 'default')).toBeChecked()
  return { s, puts }
}

test('clicking a preview picks its template; arrow keys move the pick', async ({ page }) => {
  const { s, puts } = await openChatPage(page)
  // A real click on the preview: the iframe ignores the pointer, so its label gets it (what Playwright reports as
  // "intercepts pointer events").
  await page.getByTitle(zh.preview.templates.bubble).click({ force: true })
  await expect(option(page, 'bubble')).toBeChecked()
  await expect.poll(() => s.template.selected).toBe('bubble')

  await option(page, 'bubble').focus()
  await page.keyboard.press('ArrowLeft')
  await expect(option(page, 'compact')).toBeChecked()
  await expect(option(page, 'compact')).toBeFocused()
  await expect.poll(() => puts).toEqual([{ selected: 'bubble' }, { selected: 'compact' }])
})

test('a failed switch keeps the template in use and says why', async ({ page }) => {
  await openChatPage(page)
  await page.route('**/api/admin/templates/chat', (r) =>
    r.request().method() === 'PUT' ? r.fulfill({ status: 500, json: { code: 'settings.save_failed' } }) : r.fallback(),
  )
  await option(page, 'compact').click()
  await expect(page.getByText(zh.errors.settings.save_failed)).toBeVisible()
  await expect(option(page, 'default')).toBeChecked()
  await expect(option(page, 'compact')).not.toBeChecked()
})

for (const width of [1280, 320]) {
  test(`the picker has no sideways scroll at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 })
    await openChatPage(page)
    expect(await scrollsSideways(page)).toBe(false)
  })
}
