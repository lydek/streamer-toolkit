// Settings › 資料與備份 › 紀錄保存: lengthening saves at once; shortening says how many records go and asks (API mocked).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

const fill = (s: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((text, [k, v]) => text.replaceAll(`{${k}}`, String(v)), s)

/** Opens the section with the saved retention `saved`; returns the bodies of every save. */
async function open(page: Page, saved: unknown = { days: 30 }) {
  await mockAdmin(page)
  const puts: unknown[] = []
  await page.route('**/api/admin/records/retention', (r) => {
    if (r.request().method() === 'GET') return r.fulfill({ json: saved })
    const next = r.request().postDataJSON()
    puts.push(next)
    // Like the Host: only a shorter retention deletes anything.
    const daysOf = (v: unknown) => (v === 'forever' ? Infinity : (v as { days: number }).days)
    const deleted = daysOf(next) < daysOf(saved) ? 12 : 0
    saved = next
    return r.fulfill({ json: { deleted } })
  })
  await page.route('**/api/admin/records/expired?days=*', (r) => r.fulfill({ json: { count: 12 } }))
  await page.goto('/admin/settings/backup')
  return puts
}

const daysInput = (page: Page) => page.getByRole('spinbutton', { name: zh.retention.days })

test('a longer retention saves at once', async ({ page }) => {
  const puts = await open(page)
  await daysInput(page).fill('60')
  await daysInput(page).press('Enter')
  await daysInput(page).blur()
  await expect.poll(() => puts).toEqual([{ days: 60 }])
  await expect(page.getByText(fill(zh.retention.saved, { days: 60 }))).toBeVisible()
})

test('a shorter retention says how many records go and asks; cancelling changes nothing', async ({ page }) => {
  const puts = await open(page)
  await daysInput(page).fill('7')
  await daysInput(page).blur()
  const dialog = page.getByRole('dialog', { name: fill(zh.retention.confirmTitle, { days: 7 }) })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText(fill(zh.retention.confirmText, { days: 7, count: 12 }))
  await dialog.getByRole('button', { name: zh.twitch.cancel }).click()
  await expect(dialog).toBeHidden()
  await expect(daysInput(page)).toHaveValue('30')
  expect(puts).toEqual([])

  await daysInput(page).fill('7')
  await daysInput(page).blur()
  await dialog.getByRole('button', { name: fill(zh.retention.confirmOk, { days: 7 }) }).click()
  await expect.poll(() => puts).toEqual([{ days: 7 }])
  await expect(page.getByText(fill(zh.retention.deleted, { days: 7 }))).toBeVisible()
})

test('forever saves at once and notes that the data keeps growing; back to days asks', async ({ page }) => {
  const puts = await open(page)
  await page.getByRole('radio', { name: zh.retention.forever }).check()
  await expect.poll(() => puts).toEqual(['forever'])
  await expect(page.getByText(zh.retention.foreverNote)).toBeVisible()
  await expect(daysInput(page)).toBeDisabled()

  await page.getByRole('radio', { name: new RegExp(`^${zh.retention.keep}`) }).check()
  await expect(page.getByRole('dialog', { name: fill(zh.retention.confirmTitle, { days: 30 }) })).toBeVisible()
})
