// 場景 page layout, with the admin API and the live socket mocked (OBS commands are tested in ScenesPage.test.ts).
import { expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

test('the scene list lines up with its heading at 1440, 1024 and 800px', async ({ page }) => {
  await mockAdmin(page)
  const state = {
    scene: 'Game',
    scenes: [
      { name: 'Opening', items: [] },
      { name: 'Game', items: [{ id: 1, source: 'Chat overlay', kind: 'browser_source', enabled: true, locked: false }] },
    ],
    inputs: [],
    streaming: false,
    recording: false,
  }
  const obs = [{ id: 's', type: 'obs.state', version: 1, seq: 1, occurredAt: '', source: 'obs', payload: state }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: { 'chat.message': [], 'obs.status': [], 'obs.state': obs, 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] },
      }),
    ),
  )
  await page.goto('/admin/scenes')
  const heading = page.getByRole('heading', { level: 2, name: zh.scene.scenes, exact: true })
  const first = page.getByRole('button', { name: 'Opening' })
  await expect(first).toBeVisible()
  for (const width of [1440, 1024, 800]) {
    await page.setViewportSize({ width, height: 900 })
    const [h, b] = await Promise.all([heading.boundingBox(), first.boundingBox()])
    // A list left with the browser's own indent sat 40px to the right of its heading.
    expect(Math.abs(b!.x - h!.x), `${width}px`).toBeLessThan(2)
  }
})

test('the item table keeps short cells on one line, and the scene list fills its card on narrow windows', async ({ page }) => {
  await mockAdmin(page)
  const items = [
    { id: 1, source: 'Starting Background', kind: 'image_source', enabled: true, locked: false, x: 0, y: 0, width: 300, height: 100 },
    { id: 2, source: 'Chat overlay', kind: 'browser_source', enabled: true, locked: false },
  ]
  const state = { scene: 'Game', scenes: [{ name: 'Opening', items: [] }, { name: 'Game', items }], inputs: [], streaming: false, recording: false }
  const obs = [{ id: 's', type: 'obs.state', version: 1, seq: 1, occurredAt: '', source: 'obs', payload: state }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) =>
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload: { 'chat.message': [], 'obs.status': [], 'obs.state': obs, 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] } })),
  )
  await page.goto('/admin/scenes')
  const oneLine = (loc: ReturnType<typeof page.locator>) =>
    loc.evaluateAll((els) => els.every((e) => e.getBoundingClientRect().height < parseFloat(getComputedStyle(e).lineHeight) * 1.8 + 16))
  for (const width of [1440, 800, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expect(page.locator('.kind-cell').first()).toBeVisible()
    expect(await oneLine(page.locator('.kind-cell, tbody th, caption')), `${width}px`).toBe(true)
    const [card, list] = await Promise.all([page.locator('.pane').first().boundingBox(), page.locator('.scene-list').boundingBox()])
    expect(list!.width, `${width}px`).toBeGreaterThan(card!.width - 48)
  }
})
