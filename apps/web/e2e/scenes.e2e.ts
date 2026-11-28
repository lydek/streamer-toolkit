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

test('on a phone each item is two lines, its switches and more beside the name; wider, it stays a table', async ({ page }) => {
  await mockAdmin(page)
  const items = [
    { id: 1, source: 'Starting Background', kind: 'image_source', enabled: true, locked: false, x: 1920, y: 1080, width: 1920, height: 1080 },
    { id: 2, source: 'Chat overlay', kind: 'browser_source', enabled: true, locked: true },
  ]
  const state = { scene: 'Game', scenes: [{ name: 'Game', items }], inputs: [], streaming: false, recording: false }
  const obs = [{ id: 's', type: 'obs.state', version: 1, seq: 1, occurredAt: '', source: 'obs', payload: state }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) =>
    ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload: { 'chat.message': [], 'obs.status': [], 'obs.state': obs, 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] } })),
  )
  await page.goto('/admin/scenes')
  const rows = page.locator('tbody tr[data-item]')
  await expect(rows).toHaveCount(2)
  // Each row's visible parts by line, top to bottom; each line names what is on it.
  const lines = () =>
    rows.evaluateAll((trs) =>
      trs.map((tr) => {
        const parts = [
          ['grip', tr.querySelector('.grip')],
          ['name', tr.querySelector('.item-name')],
          ['kind', tr.querySelector('.kind-cell')],
          ['geo', tr.querySelector('.geo')],
          ['show', tr.querySelector('.p-toggleswitch')],
          ...[...tr.querySelectorAll('.p-button')].map((b) => [b.getAttribute('aria-label')!.split(/[：:「]/)[0], b] as const),
        ] as [string, Element][]
        const placed = parts.map(([name, el]) => ({ name, box: el.getBoundingClientRect() })).sort((a, b) => a.box.top - b.box.top)
        const out: { names: string[]; mid: number }[] = []
        for (const p of placed) {
          const mid = p.box.top + p.box.height / 2
          const line = out.find((l) => Math.abs(l.mid - mid) < 12)
          if (line) line.names.push(p.name)
          else out.push({ names: [p.name], mid })
        }
        return { lines: out.map((l) => l.names), right: Math.max(...placed.map((p) => p.box.right)) }
      }),
    )
  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 })
    for (const row of await lines()) {
      expect(row.lines, `${width}px`).toHaveLength(2)
      expect(row.lines[0], `${width}px`).toEqual(expect.arrayContaining(['grip', 'name', 'show']))
      expect(row.lines[0]).toHaveLength(5)
      expect(row.lines[1], `${width}px`).toEqual(expect.arrayContaining(['kind', 'geo']))
      expect(row.right, `${width}px`).toBeLessThanOrEqual(width)
    }
    await expect(page.locator('tbody .num').first()).toBeHidden()
  }
  for (const width of [800, 1280]) {
    await page.setViewportSize({ width, height: 900 })
    for (const row of await lines()) expect(row.lines, `${width}px`).toHaveLength(1)
    await expect(page.locator('tbody .num').first()).toBeVisible()
  }
})
