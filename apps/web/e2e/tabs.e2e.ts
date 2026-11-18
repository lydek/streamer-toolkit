// The app bar: sticky top, group tabs, sub-tabs and "More" (docs/decisions.md 管理 UI 版面), with the API mocked.
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, openAdmin, scrollsSideways, zh } from './fixtures'

const tabs = (page: Page) => page.getByRole('navigation', { name: zh.nav.label })
const tab = (page: Page, group: string) => tabs(page).getByRole('link', { name: zh.nav.groups[group] })
const subTab = (page: Page, name: string) => page.getByRole('navigation', { name: zh.nav.sub }).getByRole('link', { name })
const more = (page: Page) => tabs(page).getByRole('button', { name: zh.nav.more })

test('every group and sub-page is one click away, and the current one is marked', async ({ page }) => {
  await openAdmin(page)
  // Every group has sub-tabs: the group tab marks the current group, the sub-tab the current page.
  await expect(tab(page, 'home')).toHaveAttribute('aria-current', 'true')
  await expect(subTab(page, zh.nav.groups.home)).toHaveAttribute('aria-current', 'page')

  // A group goes to its first built page: the chat log is not built yet, the overlay templates are.
  await tab(page, 'chat').click()
  await expect(page).toHaveURL(/\/admin\/chat$/)
  await expect(tab(page, 'chat')).toHaveAttribute('aria-current', 'true')
  await expect(subTab(page, zh.pages.chatTemplates.name)).toHaveAttribute('aria-current', 'page')

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
  await page.setViewportSize({ width: 1280, height: 640 })
  await openAdmin(page)
  await expect(more(page)).toBeHidden()

  // Large text: the groups no longer fit in one row at 640px.
  await page.setViewportSize({ width: 640, height: 640 })
  await page.evaluate(() => (document.documentElement.style.fontSize = '32px'))
  await expect(more(page)).toBeVisible()
  expect(await scrollsSideways(page)).toBe(false)
  const visible = await tabs(page).getByRole('link').count()
  await more(page).click()
  const items = page.locator('#tabs-more').getByRole('menuitem')
  expect(visible + (await items.count())).toBe(Object.keys(zh.nav.groups).length)
  await items.last().click()
  await expect(page).toHaveURL(/\/admin\/viewers$/)
  await expect(more(page)).toHaveClass(/current/)
})

test('away from home, a status light goes to its section of settings', async ({ page }) => {
  await openAdmin(page)
  await tab(page, 'chat').click()
  await page.getByRole('button', { name: /^OBS：/ }).click()
  await expect(page).toHaveURL(/\/admin\/settings#obs$/)
  await expect(page.getByRole('heading', { name: 'OBS' })).toBeFocused()
})

const problem = (page: Page) => page.getByRole('button', { name: new RegExp(`^${zh.nav.problem.split('{')[0]}`) })
const light = (page: Page, name: string) => page.getByRole('button', { name: new RegExp(`^${name}：`) })

test('below 600px the tabs are a bottom bar, and a warning replaces the status lights', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 700 })
  await openAdmin(page)
  const box = await tabs(page).boundingBox()
  expect(Math.round(box!.y + box!.height)).toBe(700)
  await expect(light(page, 'Twitch')).toBeHidden()
  await expect(light(page, 'OBS')).toBeHidden()
  // Twitch is not set up in the mocks: a problem.
  await expect(problem(page)).toBeVisible()
  await expect(problem(page)).toHaveAccessibleName(zh.nav.problem.replace('{names}', 'Twitch'))

  // The first four groups are in the bar; the rest are in "More".
  for (const [group, url] of [['song', /\/admin\/songs$/], ['chat', /\/admin\/chat$/], ['interact', /\/admin\/alerts$/], ['home', /\/admin\/$/]] as const) {
    await tab(page, group).click()
    await expect(page).toHaveURL(url)
    expect(await scrollsSideways(page), group).toBe(false)
  }
  for (const [group, url] of [['media', /\/admin\/sounds$/], ['viewers', /\/admin\/viewers$/]] as const) {
    await more(page).click()
    await page.locator('#tabs-more').getByRole('menuitem', { name: zh.nav.groups[group] }).click()
    await expect(page).toHaveURL(url)
    expect(await scrollsSideways(page), group).toBe(false)
  }
})

test('a narrower bar keeps the lights that have a problem', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 })
  await openAdmin(page)
  // Twitch has a problem, OBS is still loading: both stay.
  await expect(light(page, 'Twitch')).toBeVisible()
  await expect(light(page, 'OBS')).toBeVisible()
  await expect(problem(page)).toBeHidden()
})

test('when every connection is fine, a narrower bar shows one "connected" light', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 })
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  const obs = { id: 'o1', type: 'obs.status', version: 1, seq: 1, occurredAt: '2026-11-18T10:00:00.000Z', source: 'obs', payload: { state: 'connected', reason: null } }
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload: { 'chat.message': [], 'obs.status': [obs] } })),
  )
  await page.goto('/admin/')
  const all = light(page, zh.nav.connections)
  await expect(all).toBeVisible()
  await expect(light(page, 'Twitch')).toBeHidden()
  await expect(light(page, 'OBS')).toBeHidden()
  await all.click()
  await expect(page.locator('#drawer-connections')).toBeVisible()

  // A wide bar shows each light.
  await page.setViewportSize({ width: 1440, height: 700 })
  await expect(light(page, 'Twitch')).toBeVisible()
  await expect(all).toBeHidden()
})

test('sub-tabs stay one row, faded at the edge while more are off to the right', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 700 })
  await openAdmin(page)
  await tab(page, 'interact').click()
  const sub = page.getByRole('navigation', { name: zh.nav.sub })
  await expect(sub).toHaveClass(/at-end/)
  await page.evaluate(() => (document.documentElement.style.fontSize = '64px'))
  await expect(sub).not.toHaveClass(/at-end/)
  const rows = await sub.getByRole('link').evaluateAll((els) => new Set(els.map((e) => Math.round(e.getBoundingClientRect().top))).size)
  expect(rows).toBe(1)
})

test('an unbuilt page says it is not available yet and calls no API beyond the app bar status', async ({ page }) => {
  await openAdmin(page)
  const calls: string[] = []
  page.on('request', (r) => {
    const path = new URL(r.url()).pathname
    if (/^\/(api|templates|sounds)\//.test(path) && path !== '/api/admin/twitch') calls.push(path)
  })
  await tab(page, 'song').click()
  await expect(page).toHaveURL(/\/admin\/songs$/)
  await expect(page.getByRole('heading', { level: 1, name: zh.pages.songs.name })).toBeVisible()
  await expect(page.getByText(zh.pages.soon)).toBeVisible()
  await subTab(page, zh.pages.songList.name).click()
  await expect(page.getByText(zh.pages.songList.desc)).toBeVisible()
  await page.waitForTimeout(500)
  expect(calls).toEqual([])
})

test('a coming-soon home tile opens its page, and going home puts focus back on it', async ({ page }) => {
  await openAdmin(page)
  const tile = page.getByRole('link', { name: new RegExp(zh.pages.scenes.name) })
  await tile.click()
  await expect(page).toHaveURL(/\/admin\/scenes$/)
  await page.getByRole('button', { name: zh.app.home }).click()
  await expect(page).toHaveURL(/\/admin\/$/)
  await expect(tile).toBeFocused()
})
