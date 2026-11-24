// Backup on the settings page, with the admin API and IPC mocked. The save dialog is faked: a real one is native.
import { type Page, type Route, expect, test } from '@playwright/test'
import { openAdmin, scrollsSideways, zh } from './fixtures'

declare global {
  interface Window {
    __e2eSaved: { name: string; bytes: number[] } | undefined
  }
}

const BACKUP = [0x50, 0x4b, 0x05, 0x06, 1, 2, 3]

/** Replaces the save dialog: `choose` picks a file that collects what is written, otherwise it is cancelled. */
const fakeSaveDialog = (page: Page, choose: boolean) =>
  page.addInitScript((choose) => {
    window.__e2eSaved = undefined
    Object.assign(window, {
      showSaveFilePicker: async (options: { suggestedName: string }) => {
        if (!choose) throw new DOMException('cancelled', 'AbortError')
        const saved = { name: options.suggestedName, bytes: [] as number[] }
        window.__e2eSaved = saved
        return {
          createWritable: async () =>
            new WritableStream<Uint8Array>({
              write: (chunk) => void saved.bytes.push(...chunk),
            }),
        }
      },
    })
  }, choose)

async function openSettings(page: Page) {
  await openAdmin(page)
  let exports = 0
  await page.route('**/api/admin/backup', (r) => {
    exports++
    return r.fulfill({ body: Buffer.from(BACKUP), contentType: 'application/octet-stream' })
  })
  await page.getByRole('button', { name: zh.settings.title, exact: true }).click()
  await page.getByRole('navigation', { name: zh.settings.sections }).getByRole('link', { name: zh.settings.menu.backup }).click()
  return () => exports
}

const exportButton = (page: Page) => page.getByRole('button', { name: zh.backup.export })

test('export asks where to save, then writes the backup there', async ({ page }) => {
  await fakeSaveDialog(page, true)
  await openSettings(page)
  await exportButton(page).click()
  await expect(page.getByText(zh.backup.exported)).toBeVisible()
  const saved = await page.evaluate(() => window.__e2eSaved)
  expect(saved?.name).toMatch(/^streamer-toolkit-\d{8}-\d{4}\.stbackup$/)
  expect(saved?.bytes).toEqual(BACKUP)
})

test('a cancelled save dialog exports nothing', async ({ page }) => {
  await fakeSaveDialog(page, false)
  const exports = await openSettings(page)
  await exportButton(page).click()
  await expect(exportButton(page)).toBeEnabled()
  expect(exports()).toBe(0)
  await expect(page.getByText(zh.backup.exported)).toHaveCount(0)
})

test('without a save dialog the backup is downloaded', async ({ page }) => {
  await page.addInitScript(() => delete (window as { showSaveFilePicker?: unknown }).showSaveFilePicker)
  await openSettings(page)
  const download = page.waitForEvent('download')
  await exportButton(page).click()
  expect((await download).suggestedFilename()).toMatch(/\.stbackup$/)
})

test('a refused export shows why', async ({ page }) => {
  await fakeSaveDialog(page, true)
  await openSettings(page)
  await page.route('**/api/admin/backup', (r) => r.fulfill({ status: 500, json: { code: 'backup.too_large' } }))
  await exportButton(page).click()
  await expect(page.getByText(zh.errors.backup.too_large)).toBeVisible()
})

const PREVIEW = {
  created_at: '2026-11-17T01:30:00Z',
  app_version: '0.1.0',
  redemptions: 12,
  checkins: 34,
  templates: 1,
  sounds: 5,
  music: 3,
  media: 1,
  contents: ['data', 'media'],
}

/** Mocks the parted restore upload; `check` answers the last step. Returns the size of each part that arrived. */
async function mockUpload(page: Page, check: (r: Route) => Promise<void>) {
  const parts: number[] = []
  await page.route('**/api/admin/backup/restore/upload', (r) => r.fulfill({ json: { id: 'u1' } }))
  await page.route('**/api/admin/backup/restore/upload/u1?offset=*', (r) => {
    const size = r.request().postDataBuffer()?.length ?? 0
    parts.push(size)
    return r.fulfill({ json: { received: parts.reduce((a, b) => a + b, 0) } })
  })
  await page.route('**/api/admin/backup/restore/upload/u1/check', check)
  // A failed check cancels the upload.
  await page.route('**/api/admin/backup/restore/upload/u1', (r) => r.fulfill({ status: 204 }))
  return parts
}

test('restore previews the chosen file, then confirms it to restart', async ({ page }) => {
  await openSettings(page)
  let confirms = 0
  const uploads = await mockUpload(page, (r) => r.fulfill({ json: PREVIEW }))
  await page.route('**/api/admin/backup/restore', (r) => {
    confirms++
    return r.fulfill({ status: 202 })
  })
  await page.locator('#restore-file').setInputFiles({
    name: 'old.stbackup',
    mimeType: 'application/octet-stream',
    buffer: Buffer.from(BACKUP),
  })
  await expect(page.getByText(zh.backup.appVersion.replace('{version}', '0.1.0'))).toBeVisible()
  await expect(page.getByText('兌換紀錄 12 筆、打卡紀錄 34 筆')).toBeVisible()
  await expect(page.getByText('樣板 1 個、音效 5 個、背景音樂 3 首、影片與 GIF 1 個')).toBeVisible()
  expect(uploads).toEqual([BACKUP.length])
  expect(confirms).toBe(0)
  for (const width of [640, 320]) {
    await page.setViewportSize({ width, height: 640 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }

  await page.getByRole('button', { name: zh.backup.restore }).click()
  await expect(page.getByRole('status').filter({ hasText: zh.backup.restarting })).toBeVisible()
  expect(confirms).toBe(1)
})

for (const code of ['backup.invalid', 'backup.newer_version']) {
  test(`a refused backup (${code}) shows why and offers no restore`, async ({ page }) => {
    await openSettings(page)
    await mockUpload(page, (r) => r.fulfill({ status: 400, json: { code } }))
    await page.locator('#restore-file').setInputFiles({
      name: 'bad.stbackup',
      mimeType: 'application/octet-stream',
      buffer: Buffer.from('not a backup'),
    })
    const [area, reason] = code.split('.')
    await expect(page.getByRole('alert')).toHaveText(zh.errors[area][reason])
    await expect(page.getByRole('button', { name: zh.backup.restore })).toHaveCount(0)
  })
}
