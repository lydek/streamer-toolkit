import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { saveFile } from './save'

/** What one restore upload request carries; the Host takes parts of up to 16 MB. */
export const PART_BYTES = 8 * 1024 * 1024

export const sizeUnit = (bytes: number) => (bytes >= 1024 ** 3 ? 'GB' : 'MB')

/** `1.2 GB`, `350.0 MB`: one decimal, in GB from 1 GB up. */
export function formatSize(bytes: number, unit = sizeUnit(bytes)) {
  return `${(bytes / 1024 ** (unit === 'GB' ? 3 : 2)).toFixed(1)} ${unit}`
}

/** A backup error in words; `backup.no_space` says how much free space is needed. */
export function backupError(e: unknown) {
  if (!(e instanceof ApiError)) return String(e)
  const needed = e.params.needed
  return errorMessage(e.code, typeof needed === 'number' ? { needed: formatSize(needed) } : {})
}

const pad = (n: number) => String(n).padStart(2, '0')

/** What to back up: both kinds, or only the data (settings and records) or only the media. */
export type BackupChoice = 'all' | Schemas['Content']

/**
 * e.g. `streamer-toolkit-20261117-0930.stbackup` for both kinds, `…-0930-data.stbackup` or `…-0930-media.stbackup`
 * for one, in local time, so exports of different kinds never share a name.
 */
export const backupFileName = (choice: BackupChoice = 'all', d = new Date()) =>
  `streamer-toolkit-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${choice === 'all' ? '' : `-${choice}`}.stbackup`

/**
 * Asks where to save first, then streams the backup there. Resolves `false` when the dialog is cancelled; without
 * the save dialog API the browser downloads the file.
 */
export const saveBackup = (choice: BackupChoice = 'all') =>
  saveFile(backupFileName(choice), { description: 'Streamer Toolkit', mime: 'application/octet-stream', extension: '.stbackup' }, async () => {
    const query = choice === 'all' ? {} : { contents: choice }
    const backup = await api(client.GET('/api/admin/backup', { params: { query }, parseAs: 'stream' }))
    if (!backup) throw new Error('empty backup')
    return backup
  })

export interface RestoreProgress {
  /** `upload`: `sent` of `total` bytes have arrived; `check`: the Host checks and stages the file. */
  phase: 'upload' | 'check'
  sent: number
  total: number
}

/**
 * Uploads a backup file in parts, then has the Host check it and stage it for a restore. `signal` cancels; on any
 * failure the Host drops what arrived. The Host refuses at the start when the disk lacks room.
 */
export async function previewRestore(file: File, onProgress: (p: RestoreProgress) => void = () => {}, signal?: AbortSignal) {
  const { id } = await api(client.POST('/api/admin/backup/restore/upload', { body: { size: file.size }, signal }))
  const path = { id }
  try {
    for (let offset = 0; offset < file.size; offset += PART_BYTES) {
      onProgress({ phase: 'upload', sent: offset, total: file.size })
      await api(
        client.PUT('/api/admin/backup/restore/upload/{id}', {
          params: { path, query: { offset } },
          // The body is the part itself, not JSON.
          body: file.slice(offset, offset + PART_BYTES) as unknown as string,
          bodySerializer: (b: unknown) => b as BodyInit,
          headers: { 'Content-Type': 'application/octet-stream' },
          signal,
        }),
      )
    }
    onProgress({ phase: 'check', sent: file.size, total: file.size })
    return await api(client.POST('/api/admin/backup/restore/upload/{id}/check', { params: { path }, signal }))
  } catch (e) {
    // Gone already after a check; otherwise the partial file goes now rather than at the next start.
    void client.DELETE('/api/admin/backup/restore/upload/{id}', { params: { path } }).catch(() => {})
    throw e
  }
}

/** How large the data and the media are now. */
export const backupSizes = () => api(client.GET('/api/admin/backup/sizes'))

/** Restores the staged backup: the app restarts and applies it before the database opens. */
export const confirmRestore = () => api(client.POST('/api/admin/backup/restore'))
