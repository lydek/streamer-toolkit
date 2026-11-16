import { api, client } from './api'

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
