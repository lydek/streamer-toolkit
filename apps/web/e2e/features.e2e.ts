// Settings › features: every feature by group; a built one switches its module, and its tabs follow (API mocked).
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'

const ALL_ON = ['chat', 'alerts', 'checkin', 'redemption', 'sound'].map((id) => ({ id, enabled: true, closed: false }))

/** Opens settings with the module switches answering like the Host: a PUT changes what GET returns. */
async function openFeatures(page: Page) {
  const s = await openAdmin(page, { modules: structuredClone(ALL_ON) })
  await page.route('**/api/admin/modules/*', (r) => {
    const id = new URL(r.request().url()).pathname.split('/').pop()!
    const { enabled } = r.request().postDataJSON()
    s.modules = s.modules.map((m) => (m.id === id ? { ...m, enabled } : m))
    return r.fulfill({ status: 204 })
  })
  await page.getByRole('button', { name: zh.settings.title, exact: true }).click()
  await page.getByRole('navigation', { name: zh.settings.sections }).getByRole('link', { name: zh.settings.menu.features }).click()
  await expect(page.getByRole('heading', { level: 3, name: zh.nav.groups.media })).toBeVisible()
}

const sw = (page: Page, feature: string) => page.getByRole('switch', { name: zh.features[feature].name, exact: true })
const subTab = (page: Page, name: string) => page.getByRole('navigation', { name: zh.nav.sub }).getByRole('link', { name })
const tabs = (page: Page) => page.getByRole('navigation', { name: zh.nav.label })

test('turning a built feature off hides its tabs, also after a reload, and on brings them back', async ({ page }) => {
  await openFeatures(page)
  // 31 features can be switched (features.ts, without the always-on ones).
  await expect(page.getByRole('status').filter({ hasText: '已開啟' })).toHaveText(
    zh.features.count.replace('{on}', '5').replace('{all}', '31'),
  )

  await sw(page, 'sound').click()
  await expect(sw(page, 'sound')).not.toBeChecked()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(page).toHaveURL(/\/admin\/scenes$/)
  await expect(subTab(page, zh.nav.categories.sound)).toHaveCount(0)

  await page.reload()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(subTab(page, zh.nav.categories.obs)).toBeVisible()
  await expect(subTab(page, zh.nav.categories.sound)).toHaveCount(0)

  await page.goto('/admin/settings/features')
  await sw(page, 'sound').click()
  await expect(sw(page, 'sound')).toBeChecked()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(subTab(page, zh.nav.categories.sound)).toBeVisible()
})

test("a feature's page link opens its first built page", async ({ page }) => {
  await openFeatures(page)
  await page.getByRole('link', { name: `：${zh.features.chat.name}` }).click()
  await expect(page).toHaveURL(/\/admin\/chat-log$/)
})

test('features not built yet say so and cannot be switched; home is always on', async ({ page }) => {
  await openFeatures(page)
  await expect(sw(page, 'singingBooth')).toBeDisabled()
  const home = page.getByRole('listitem').filter({ has: page.getByText(zh.features.home.name, { exact: true }) })
  await expect(home.getByText(zh.features.alwaysOn)).toBeVisible()
  await expect(sw(page, 'home')).toHaveCount(0)
})

test('search finds features by name or description', async ({ page }) => {
  await openFeatures(page)
  const search = page.getByRole('searchbox', { name: zh.features.search })
  // Found by its description only.
  await search.fill('VSeeFace')
  await expect(page.getByText(zh.features.avatar.name, { exact: true })).toBeVisible()
  await expect(sw(page, 'chat')).toHaveCount(0)
  await search.fill('沒有這個功能')
  await expect(page.getByText(zh.features.empty)).toBeVisible()
})
