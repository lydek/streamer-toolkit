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

  // A group goes to its first built page (tabs.test.ts covers skipping coming-soon ones).
  await tab(page, 'chat').click()
  await expect(page).toHaveURL(/\/admin\/chat-log$/)
  await expect(tab(page, 'chat')).toHaveAttribute('aria-current', 'true')
  await expect(subTab(page, zh.pages.chatLog.name)).toHaveAttribute('aria-current', 'page')

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
  await expect(page).toHaveURL(/\/admin\/chat-log$/)
})

test('the app bar stays in view while the page scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 360 })
  await openAdmin(page)
  // The feature list is the longest settings section.
  await page.goto('/admin/settings/features')
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
  // The last item of "More" is always 「編輯分頁」.
  const items = page.locator('#tabs-more').getByRole('menuitem').filter({ hasNotText: zh.nav.edit })
  expect(visible + (await items.count())).toBe(Object.keys(zh.nav.groups).length)
  await items.last().click()
  await expect(page).toHaveURL(/\/admin\/viewers$/)
  await expect(more(page)).toHaveClass(/current/)
})

test('away from home, a status light goes to its section of settings', async ({ page }) => {
  await openAdmin(page)
  await tab(page, 'chat').click()
  await page.getByRole('button', { name: /^OBS：/ }).click()
  await expect(page).toHaveURL(/\/admin\/settings\/connections#obs$/)
  await expect(page.getByRole('heading', { name: 'OBS' })).toBeFocused()

  // Already in that section: Twitch, then OBS again, each lands on its own heading.
  await page.getByRole('button', { name: /^Twitch：/ }).click()
  await expect(page).toHaveURL(/#twitch$/)
  await expect(page.getByRole('heading', { name: 'Twitch', exact: true })).toBeFocused()
  await page.getByRole('button', { name: /^OBS：/ }).click()
  await expect(page).toHaveURL(/#obs$/)
  await expect(page.getByRole('heading', { name: 'OBS' })).toBeFocused()
})

test('the app bar says when OBS is streaming or recording, and says nothing while it is idle', async ({ page }) => {
  const banner = page.getByRole('banner')
  await openAdmin(page)
  await expect(banner.getByText(zh.nav.streaming)).toHaveCount(0)
  await expect(banner.getByText(zh.nav.recording)).toHaveCount(0)

  const state = { scene: 'Live', scenes: [], inputs: [], streaming: true, recording: true }
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [],
          'obs.status': [],
          'obs.state': [{ id: 's', type: 'obs.state', version: 1, seq: 1, occurredAt: '', source: 'obs', payload: state }],
          'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [],
        },
      }),
    ),
  )
  await page.reload()
  await expect(banner.getByText(zh.nav.streaming)).toBeVisible()
  await expect(banner.getByText(zh.nav.recording)).toBeVisible()
})

/** Opens home with the live state `stream` in the snapshot. */
async function openWithStream(page: Page, stream: unknown) {
  await mockAdmin(page)
  const status = { id: 'l', type: 'stream.status', version: 1, seq: 1, occurredAt: '', source: 'twitch', payload: stream }
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [status], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
      }),
    ),
  )
  await page.goto('/admin/')
}

test('the app bar says how long the stream has run and for how many viewers', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const startedAt = new Date(Date.now() - (3600 + 2 * 60 + 5) * 1000).toISOString()
  await openWithStream(page, { state: 'live', startedAt, viewers: 42 })
  const pill = page.getByRole('banner').getByText(zh.nav.live)
  await expect(pill).toBeVisible()
  await expect(pill).toContainText(/1:02:0\d · 42 位觀眾/)
})

test('after the stream the app bar says it ended, and before Twitch answers it says nothing', async ({ page }) => {
  await openWithStream(page, { state: 'offline' })
  await expect(page.getByRole('banner').getByText(zh.nav.offline)).toBeVisible()
  await openWithStream(page, { state: 'unknown' })
  await expect(page.getByRole('banner').getByText(zh.nav.live)).toHaveCount(0)
  await expect(page.getByRole('banner').getByText(zh.nav.offline)).toHaveCount(0)
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
  for (const [group, url] of [['song', /\/admin\/songs$/], ['chat', /\/admin\/chat-log$/], ['interact', /\/admin\/alerts$/], ['home', /\/admin\/$/]] as const) {
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
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload: { 'chat.message': [], 'obs.status': [obs], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] } })),
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

/** Opens home with the saved tab bar `saved`; returns the bodies of every save. */
async function openWithTabBar(page: Page, saved: unknown = null) {
  await mockAdmin(page)
  const puts: unknown[] = []
  await page.route('**/api/admin/tab-bar', (r) => {
    if (r.request().method() === 'GET') return r.fulfill({ json: saved })
    saved = r.request().postDataJSON()
    puts.push(saved)
    return r.fulfill({ status: 204 })
  })
  await page.goto('/admin/')
  await page.locator('[data-drawer-tile], [data-drawer-arrange]').first().waitFor()
  return puts
}

test('with nothing in "More", 編輯分頁 is in the bar; pinning and order are saved and shown', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const puts = await openWithTabBar(page)
  await expect(more(page)).toBeHidden()
  await tabs(page).getByRole('button', { name: zh.nav.edit }).click()
  const editor = page.getByRole('dialog', { name: zh.nav.editor.title })
  await editor.getByRole('checkbox', { name: zh.nav.groups.viewers, exact: true }).uncheck()
  await editor.getByRole('button', { name: `${zh.arrange.earlier}：${zh.nav.groups.media}` }).click()
  await editor.getByRole('button', { name: zh.nav.editor.save }).click()
  await expect(editor).toBeHidden()
  await expect.poll(() => puts).toEqual([{ groups: ['home', 'song', 'chat', 'media', 'interact'], position: 'top' }])

  const order = await tabs(page).getByRole('link').allTextContents()
  expect(order.map((t) => t.trim())).toEqual(['home', 'song', 'chat', 'media', 'interact'].map((g) => zh.nav.groups[g]))
  await expect(more(page)).toBeVisible()
  await more(page).click()
  const items = page.locator('#tabs-more').getByRole('menuitem')
  await expect(items).toHaveText([zh.nav.groups.viewers, zh.nav.edit])
})

/** How far the last shown item ("More" or 編輯分頁) reaches past the tab bar's content box, in pixels. */
const lastItemOverflow = (page: Page) =>
  tabs(page).evaluate((nav) => {
    const right = nav.getBoundingClientRect().right - parseFloat(getComputedStyle(nav).paddingRight)
    const items = [...nav.children].filter((el) => el.matches('a, button') && (el as HTMLElement).offsetWidth > 0)
    return items.at(-1)!.getBoundingClientRect().right - right
  })

for (const position of ['top', 'bottom']) {
  test(`at any width the ${position} tab bar keeps "More" or 編輯分頁 whole`, async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 800 })
    await openWithTabBar(page, { groups: ['home', 'song', 'chat', 'interact', 'media', 'viewers'], position })
    for (let width = 1400; width >= 600; width -= 5) {
      await page.setViewportSize({ width, height: 800 })
      await expect.poll(() => lastItemOverflow(page), `${width}px`).toBeLessThanOrEqual(0.5)
    }
  })
}

test('the tab bar can sit at the bottom, also after a restart, and restoring the defaults puts it back', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const puts = await openWithTabBar(page, { groups: ['home', 'song', 'chat', 'interact', 'media', 'viewers'], position: 'bottom' })
  const box = (await tabs(page).boundingBox())!
  expect(Math.round(box.y + box.height)).toBe(800)
  await tab(page, 'chat').click()
  await expect(page).toHaveURL(/\/admin\/chat-log$/)

  await tabs(page).getByRole('button', { name: zh.nav.edit }).click()
  await page.getByRole('dialog', { name: zh.nav.editor.title }).getByRole('button', { name: zh.nav.editor.restore }).click()
  await expect.poll(() => puts).toEqual([null])
  expect(Math.round((await tabs(page).boundingBox())!.y)).toBeLessThan(200)
})
