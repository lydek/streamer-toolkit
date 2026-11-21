import { afterEach, expect, it, vi } from 'vitest'
import { ApiError, client } from './api'
import { PART_BYTES, type RestoreProgress, backupError, backupFileName, formatSize, previewRestore } from './backup'

const ok = (data: unknown) => Promise.resolve({ data, response: new Response(null, { status: 200 }) })

afterEach(() => vi.restoreAllMocks())

it('uploads the file in parts at their offsets, reports progress, then has the Host check it', async () => {
  vi.spyOn(client, 'POST').mockImplementation(((path: string) =>
    ok(path.endsWith('/check') ? { redemptions: 1 } : { id: 'u1' })) as never)
  const put = vi.spyOn(client, 'PUT').mockImplementation((() => ok({ received: 0 })) as never)
  const file = new File([new Uint8Array(PART_BYTES * 2 + 5)], 'big.stbackup')
  const seen: RestoreProgress[] = []
  const preview = await previewRestore(file, (p) => seen.push(p))
  expect(preview).toEqual({ redemptions: 1 })
  expect(client.POST).toHaveBeenNthCalledWith(1, '/api/admin/backup/restore/upload', expect.objectContaining({ body: { size: file.size } }))
  const offsets = (put.mock.calls as unknown as [string, { params: { query: { offset: number } } }][]).map(([, init]) => init.params.query.offset)
  expect(offsets).toEqual([
    0,
    PART_BYTES,
    PART_BYTES * 2,
  ])
  expect(seen.map((p) => [p.phase, p.sent])).toEqual([
    ['upload', 0],
    ['upload', PART_BYTES],
    ['upload', PART_BYTES * 2],
    ['check', file.size],
  ])
})

it('drops the upload on the Host when it fails or is cancelled', async () => {
  vi.spyOn(client, 'POST').mockImplementation((() => ok({ id: 'u1' })) as never)
  vi.spyOn(client, 'PUT').mockRejectedValue(new DOMException('aborted', 'AbortError'))
  const del = vi.spyOn(client, 'DELETE').mockImplementation((() => ok(undefined)) as never)
  await expect(previewRestore(new File(['x'], 'a.stbackup'))).rejects.toThrow('aborted')
  expect(del).toHaveBeenCalledWith('/api/admin/backup/restore/upload/{id}', { params: { path: { id: 'u1' } } })
})

it('says how much free space a restore needs', () => {
  expect(formatSize(3.4 * 1024 ** 3)).toBe('3.4 GB')
  expect(formatSize(350 * 1024 ** 2)).toBe('350.0 MB')
  expect(backupError(new ApiError('backup.no_space', { needed: 2 * 1024 ** 3 }))).toContain('2.0 GB')
})

it('names each kind of backup apart, in local time', () => {
  const d = new Date(2026, 10, 22, 9, 5)
  expect(backupFileName('all', d)).toBe('streamer-toolkit-20261122-0905.stbackup')
  expect(backupFileName('data', d)).toBe('streamer-toolkit-20261122-0905-data.stbackup')
  expect(backupFileName('media', d)).toBe('streamer-toolkit-20261122-0905-media.stbackup')
})
