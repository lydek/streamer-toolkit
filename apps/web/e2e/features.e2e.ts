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
  await expect(page.getByRole('heading', { level: 3, name: zh.nav.groups.media })).toBeVisible()
}

const sw = (page: Page, feature: string) => page.getByRole('switch', { name: zh.features[feature].name, exact: true })
const subTab = (page: Page, name: string) => page.getByRole('navigation', { name: zh.nav.sub }).getByRole('link', { name })
const tabs = (page: Page) => page.getByRole('navigation', { name: zh.nav.label })

test('turning a built feature off hides its tabs, also after a reload, and on brings them back', async ({ page }) => {
  await openFeatures(page)
  await expect(page.getByRole('status').filter({ hasText: '已開啟' })).toHaveText(
    zh.features.count.replace('{on}', '5').replace('{all}', '26'),
  )

  await sw(page, 'sound').click()
  await expect(sw(page, 'sound')).not.toBeChecked()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(page).toHaveURL(/\/admin\/scenes$/)
  await expect(subTab(page, zh.modules.names.sound)).toHaveCount(0)

  await page.reload()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(subTab(page, zh.pages.scenes.name)).toBeVisible()
  await expect(subTab(page, zh.modules.names.sound)).toHaveCount(0)

  await page.getByRole('button', { name: zh.settings.title, exact: true }).click()
  await sw(page, 'sound').click()
  await expect(sw(page, 'sound')).toBeChecked()
  await tabs(page).getByRole('link', { name: zh.nav.groups.media }).click()
  await expect(subTab(page, zh.modules.names.sound)).toBeVisible()
})

test("a feature's page link opens its first built page, not a coming-soon one", async ({ page }) => {
  await openFeatures(page)
  // Chat's first page (/chat-log) is not built yet; /chat is.
  await page.getByRole('link', { name: `：${zh.features.chat.name}` }).click()
  await expect(page).toHaveURL(/\/admin\/chat$/)
})

test('features not built yet say so and cannot be switched; home is always on', async ({ page }) => {
  await openFeatures(page)
  await expect(sw(page, 'songs')).toBeDisabled()
  await expect(page.getByText(zh.features.alwaysOn)).toBeVisible()
  await expect(sw(page, 'home')).toHaveCount(0)
})

test('search finds features by name or description', async ({ page }) => {
  await openFeatures(page)
  const search = page.getByRole('searchbox', { name: zh.features.search })
  await search.fill('換裝')
  await expect(page.getByText(zh.features.avatar.name, { exact: true })).toBeVisible()
  await expect(sw(page, 'chat')).toHaveCount(0)
  await search.fill('沒有這個功能')
  await expect(page.getByText(zh.features.empty)).toBeVisible()
})
