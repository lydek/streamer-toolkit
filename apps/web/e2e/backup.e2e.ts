// Backup on the settings page, with the admin API and IPC mocked. The save dialog is faked: a real one is native.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'

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
