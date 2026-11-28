// 點歌 page layout, with the admin API mocked (the queue's behaviour is tested in crates/modules/songs).
import { expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

const modules = [{ id: 'songs', enabled: true, closed: false }]

test('the add-song form stays one control high instead of stretching the song picker', async ({ page }) => {
  await mockAdmin(page, { modules })
  await page.route('**/api/admin/song-requests', (r) => r.fulfill({ json: { singing: null, queued: [], sung: [] } }))
  await page.route('**/api/admin/song-list', (r) => r.fulfill({ json: { songs: [], categories: [] } }))
  await page.goto('/admin/songs')
  const form = page.getByRole('form', { name: zh.songs.add })
  const viewer = form.getByRole('textbox', { name: zh.songs.for })
  const pick = form.locator('.pick')
  await expect(pick).toBeVisible()
  for (const [width, height] of [
    [1440, 900],
    [800, 900],
    [390, 844],
  ] as const) {
    await page.setViewportSize({ width, height })
    const [p, v] = await Promise.all([pick.boundingBox(), viewer.boundingBox()])
    // In a column the picker's 18rem flex basis became its height (about 288px).
    expect(p!.height, `picker at ${width}px`).toBeLessThanOrEqual(v!.height + 8)
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }
})
