import { ApiError, api, client } from './api'
import { saveFile } from './save'

/** The Host's restore limit (crates/host/src/backup.rs); checked here first so a larger file is never sent. */
export const BACKUP_MAX_BYTES = 512 * 1024 * 1024

const pad = (n: number) => String(n).padStart(2, '0')

/** e.g. `streamer-toolkit-20261117-0930.stbackup`, in local time. */
export const backupFileName = (d = new Date()) =>
  `streamer-toolkit-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}.stbackup`

/**
 * Asks where to save first, then streams the backup there. Resolves `false` when the dialog is cancelled; without
 * the save dialog API the browser downloads the file.
 */
export const saveBackup = () =>
  saveFile(backupFileName(), { description: 'Streamer Toolkit', mime: 'application/octet-stream', extension: '.stbackup' }, async () => {
    const backup = await api(client.GET('/api/admin/backup', { parseAs: 'stream' }))
    if (!backup) throw new Error('empty backup')
    return backup
  })

/** Uploads a backup file as-is; the Host checks it and stages it for a restore. */
export function previewRestore(file: File) {
  if (file.size > BACKUP_MAX_BYTES) return Promise.reject(new ApiError('backup.too_large'))
  return api(
    client.POST('/api/admin/backup/restore/preview', {
      // The body is the file itself, not JSON.
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}

/** Restores the staged backup: the app restarts and applies it before the database opens. */
export const confirmRestore = () => api(client.POST('/api/admin/backup/restore'))
