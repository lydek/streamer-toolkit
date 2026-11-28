// Home drawers' quick controls, with the admin API and the live socket mocked.
import { type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import { mockAdmin, toLiveHome, zh } from './fixtures'

/** Home with `extra` in the live snapshot (each a list of one event of that type); `send` pushes a later live message. */
async function openHome(page: Page, modules: string[], extra: Record<string, unknown>, homeTiles: string[] | null = null) {
  await mockAdmin(page, { modules: modules.map((id) => ({ id, enabled: true, closed: false })), homeTiles })
  const payload: Record<string, unknown[]> = { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] }
  for (const [type, p] of Object.entries(extra)) payload[type] = [{ id: type, type, version: 1, seq: 1, occurredAt: '', source: 'e2e', payload: p }]
  let socket: WebSocketRoute | undefined
  await page.routeWebSocket(/\/ws\/admin$/, (ws) => {
    socket = ws
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload }))
  })
  await page.goto('/admin/')
  await toLiveHome(page)
  return { send: (msg: object) => socket!.send(JSON.stringify(msg)) }
}

test('the counter drawer counts down and up, as the counter page does', async ({ page }) => {
  await openHome(page, ['counter'], { 'counter.values': { counters: [{ id: 'c1', name: 'Deaths', value: 3 }] } })
  const steps: unknown[] = []
  await page.route('**/api/admin/counters/*/add', (r) => {
    steps.push([new URL(r.request().url()).pathname, r.request().postDataJSON()])
    return r.fulfill({ json: { id: 'c1', name: 'Deaths', value: 2 } })
  })
  await page.locator('[data-drawer-tile="counter"]').click()
  const drawer = page.locator('#drawer-counter')
  await drawer.getByRole('button', { name: zh.counter.minusOneFor.replace('{name}', 'Deaths') }).click()
  await drawer.getByRole('button', { name: `${zh.counter.plusOne}：Deaths` }).click()
  await expect.poll(() => steps).toEqual([
    ['/api/admin/counters/c1/add', { delta: -1 }],
    ['/api/admin/counters/c1/add', { delta: 1 }],
  ])
})

test('the scene drawer shows or hides the scene on air’s items, as the scene page does', async ({ page }) => {
  const state = {
    scene: 'Game',
    scenes: [
      { name: 'Opening', items: [] },
      { name: 'Game', items: [{ id: 4, source: 'Chat overlay', kind: 'browser_source', enabled: false, locked: false }] },
    ],
    inputs: [],
    streaming: false,
    recording: false,
  }
  await openHome(page, ['scene'], { 'obs.state': state })
  const puts: unknown[] = []
  await page.route('**/api/admin/obs/item', (r) => {
    puts.push(r.request().postDataJSON())
    return r.fulfill({ status: 204 })
  })
  await page.locator('[data-drawer-tile="scene"]').click()
  const drawer = page.locator('#drawer-scene')
  await drawer.getByRole('switch', { name: zh.scene.showItem.replace('{item}', 'Chat overlay') }).click()
  await expect.poll(() => puts).toEqual([{ scene: 'Game', item: 4, enabled: true }])
})

const reward = (id: string, title: string, manageable: boolean, more: object = {}) => ({
  id, title, prompt: '', cost: 100, background_color: '#9146FF', is_enabled: true, is_paused: false, is_user_input_required: false,
  skip_queue: false, cooldown_seconds: null, max_per_stream: null, max_per_user_per_stream: null, manageable, ...more,
})

test('the rewards drawer pauses the toolkit’s own rewards only, and 新增獎勵 opens the add dialog on the page', async ({ page }) => {
  await openHome(page, ['rewards'], {}, ['rewards'])
  const list = [reward('own', 'Hydrate', true), reward('off', 'Old', true, { is_enabled: false }), reward('dash', 'Dashboard one', false)]
  await page.route('**/api/admin/rewards', (r) => r.fulfill({ json: list }))
  const patches: unknown[] = []
  await page.route('**/api/admin/rewards/*', (r) => {
    patches.push([new URL(r.request().url()).pathname, r.request().postDataJSON()])
    return r.fulfill({ json: list[0] })
  })
  await page.locator('[data-drawer-tile="rewards"]').click()
  const drawer = page.locator('#drawer-rewards')
  const pause = (name: string) => drawer.getByRole('switch', { name: zh.rewards.pauseLabel.replace('{name}', name) })
  await expect(pause('Old')).toBeDisabled()
  await expect(pause('Dashboard one')).toHaveCount(0)
  await expect(drawer.locator('[data-reward="dash"]')).toContainText(zh.rewards.dashboardOnly)
  await pause('Hydrate').click()
  await expect.poll(() => patches).toEqual([['/api/admin/rewards/own', { is_paused: true }]])

  await drawer.getByRole('link', { name: zh.rewards.add }).click()
  await expect(page).toHaveURL(/\/admin\/rewards$/)
  const dialog = page.locator('dialog.reward-dialog')
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(page.getByRole('button', { name: zh.rewards.add, exact: true })).toBeFocused()
})

test('a refused pause in the rewards drawer puts the switch back', async ({ page }) => {
  await openHome(page, ['rewards'], {}, ['rewards'])
  await page.route('**/api/admin/rewards', (r) => r.fulfill({ json: [reward('own', 'Hydrate', true)] }))
  await page.route('**/api/admin/rewards/*', (r) => r.fulfill({ status: 500, json: { code: 'internal' } }))
  await page.locator('[data-drawer-tile="rewards"]').click()
  const pause = page.locator('#drawer-rewards').getByRole('switch', { name: zh.rewards.pauseLabel.replace('{name}', 'Hydrate') })
  await pause.click()
  await expect(page.locator('#drawer-rewards').getByRole('alert')).toBeVisible()
  await expect(pause).not.toBeChecked()
})

test('the chat bot drawer switches timers and greetings as their pages do', async ({ page }) => {
  await openHome(page, ['bot'], {}, ['commands'])
  const greet = (enabled: boolean) => ({ enabled, message: 'hi' })
  let greetings = {
    follow: greet(true), first_chat: greet(false), returning: { ...greet(true), days: 7, post: false }, sub: greet(true), raid: greet(true),
    bits: { enabled: true, small: 'a', medium: 'b', large: 'c' },
  }
  const timers = [{ id: 't1', name: 'Discord', message: 'join', every_minutes: 15, min_chat_lines: 5, enabled: false }]
  const puts: unknown[] = []
  await page.route('**/api/admin/bot/timers', (r) => r.fulfill({ json: timers }))
  await page.route('**/api/admin/bot/timers/*', (r) => {
    puts.push([new URL(r.request().url()).pathname, r.request().postDataJSON().enabled])
    return r.fulfill({ json: timers[0] })
  })
  await page.route('**/api/admin/bot/greetings', (r) => {
    if (r.request().method() === 'PUT') {
      greetings = r.request().postDataJSON()
      puts.push(['greetings', greetings.follow.enabled])
    }
    return r.fulfill({ json: greetings })
  })
  await page.locator('[data-drawer-tile="commands"]').click()
  const drawer = page.locator('#drawer-commands')
  await drawer.getByRole('switch', { name: zh.bot.timers.enableLabel.replace('{name}', 'Discord') }).click()
  await expect.poll(() => puts).toEqual([['/api/admin/bot/timers/t1', true]])
  await drawer.getByRole('switch', { name: zh.bot.greetings.enabled.replace('{name}', zh.bot.greetings.follow.name) }).click()
  await expect.poll(() => puts.at(-1)).toEqual(['greetings', false])
})

test('a refused switch in the chat bot drawer goes back', async ({ page }) => {
  await openHome(page, ['bot'], {}, ['commands'])
  const timers = [{ id: 't1', name: 'Discord', message: 'join', every_minutes: 15, min_chat_lines: 5, enabled: false }]
  await page.route('**/api/admin/bot/timers', (r) => r.fulfill({ json: timers }))
  await page.route('**/api/admin/bot/timers/*', (r) => r.fulfill({ status: 500, json: { code: 'internal' } }))
  await page.route('**/api/admin/bot/greetings', (r) => r.fulfill({ json: Object.fromEntries(['follow', 'first_chat', 'returning', 'sub', 'bits', 'raid'].map((k) => [k, { enabled: true, message: 'hi' }])) }))
  await page.locator('[data-drawer-tile="commands"]').click()
  const drawer = page.locator('#drawer-commands')
  const timer = drawer.getByRole('switch', { name: zh.bot.timers.enableLabel.replace('{name}', 'Discord') })
  await timer.click()
  await expect(drawer.getByRole('alert')).toBeVisible()
  await expect(timer).not.toBeChecked()
})

test('the chat bot drawer says why it cannot read the bot, as the pages do', async ({ page }) => {
  await openHome(page, ['bot'], {}, ['commands'])
  await page.route('**/api/admin/bot/timers', (r) => r.fulfill({ status: 403, json: { code: 'twitch.scope_required' } }))
  await page.route('**/api/admin/bot/greetings', (r) => r.fulfill({ json: {} }))
  await page.locator('[data-drawer-tile="commands"]').click()
  await expect(page.locator('#drawer-commands').getByRole('alert')).toHaveText(zh.errors.twitch.scope_required)
})

test('the song drawer shows the song being sung and the next three, and 唱完了 works as on the page', async ({ page }) => {
  await openHome(page, ['songs'], {}, ['songs'])
  const req = (id: string, title: string) => ({ id, song_id: `s-${id}`, title, artist: '', viewer: null, by_hand: false, requested_at: '2026-11-28T12:00:00.000Z', finished_at: null })
  let board = { singing: req('r1', '晴天'), queued: [req('r2', '小幸運'), req('r3', '告白氣球'), req('r4', '突然好想你'), req('r5', '倒數')], sung: [] as unknown[] }
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: board }))
  const finished: string[] = []
  await page.route('**/api/admin/song-requests/finish', (r) => {
    finished.push('finish')
    board = { singing: null as never, queued: board.queued, sung: [] }
    return r.fulfill({ status: 204 })
  })
  await page.locator('[data-drawer-tile="songs"]').click()
  const drawer = page.locator('#drawer-songs')
  await expect(drawer.locator('[data-singing]')).toContainText('晴天')
  await expect(drawer.locator('.next li')).toHaveText(['小幸運', '告白氣球', '突然好想你'])
  await drawer.getByRole('button', { name: zh.songs.finish }).click()
  await expect.poll(() => finished).toEqual(['finish'])
  await expect(drawer.getByText(zh.songs.nothingNow)).toBeVisible()
})

test('the open song drawer follows a request changed elsewhere', async ({ page }) => {
  const home = await openHome(page, ['songs'], {}, ['songs'])
  const req = (id: string, title: string) => ({ id, song_id: `s-${id}`, title, artist: '', viewer: null, by_hand: false, requested_at: '2026-11-28T12:00:00.000Z', finished_at: null })
  let board = { singing: req('r1', 'Song A'), queued: [] as unknown[], sung: [] as unknown[] }
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: board }))
  await page.locator('[data-drawer-tile="songs"]').click()
  const drawer = page.locator('#drawer-songs')
  await expect(drawer.locator('[data-singing]')).toContainText('Song A')
  board = { ...board, singing: req('r2', 'Song B') }
  home.send({ id: 'c1', type: 'songs.changed', version: 1, seq: 2, occurredAt: '', source: 'e2e', payload: {} })
  await expect(drawer.locator('[data-singing]')).toContainText('Song B')
})
