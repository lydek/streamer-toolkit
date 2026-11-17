import { ApiError, api, client } from './api'

/** The Host's restore limit (crates/host/src/backup.rs); checked here first so a larger file is never sent. */
export const BACKUP_MAX_BYTES = 512 * 1024 * 1024

/** Chromium's File System Access API; not in TypeScript's DOM types. */
interface SavePicker {
  showSaveFilePicker?: (options: {
    suggestedName: string
    types: { description: string; accept: Record<string, string[]> }[]
  }) => Promise<FileSystemFileHandle>
}

const pad = (n: number) => String(n).padStart(2, '0')

/** e.g. `streamer-toolkit-20261117-0930.stbackup`, in local time. */
export const backupFileName = (d = new Date()) =>
  `streamer-toolkit-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}.stbackup`

/**
 * Asks where to save first (the save dialog needs the click's user activation), then streams the backup there.
 * Resolves `false` when the dialog is cancelled. Without the save dialog API the browser downloads the file.
 */
export async function saveBackup(): Promise<boolean> {
  const picker = (window as SavePicker).showSaveFilePicker
  const name = backupFileName()
  let file: FileSystemFileHandle | undefined
  if (picker) {
    try {
      file = await picker({
        suggestedName: name,
        types: [{ description: 'Streamer Toolkit', accept: { 'application/octet-stream': ['.stbackup'] } }],
      })
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return false
      throw e
    }
  }
  const backup = await api(client.GET('/api/admin/backup', { parseAs: 'stream' }))
  if (!backup) throw new Error('empty backup')
  if (file) {
    await backup.pipeTo(await file.createWritable())
    return true
  }
  const url = URL.createObjectURL(await new Response(backup).blob())
  Object.assign(document.createElement('a'), { href: url, download: name }).click()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
  return true
}

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
