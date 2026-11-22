/** Chromium's File System Access API; not in TypeScript's DOM types. */
interface SavePicker {
  showSaveFilePicker?: (options: {
    suggestedName: string
    types: { description: string; accept: Record<string, string[]> }[]
  }) => Promise<FileSystemFileHandle>
}

/** What a save dialog offers, e.g. `{ description: 'CSV', mime: 'text/csv', extension: '.csv' }`. */
export interface FileType {
  description: string
  mime: string
  extension: string
}

/**
 * Asks where to save first (the save dialog needs the click's user activation), then writes what `content` gives
 * there. Resolves the saved file's name (the one picked in the dialog), or `false` when the dialog is cancelled.
 * Without the save dialog API the browser downloads the file under `name`.
 */
export async function saveFile(
  name: string,
  type: FileType,
  content: () => Promise<Blob | ReadableStream<Uint8Array>>,
): Promise<string | false> {
  const picker = (globalThis as SavePicker).showSaveFilePicker
  let file: FileSystemFileHandle | undefined
  if (picker) {
    try {
      file = await picker({
        suggestedName: name,
        types: [{ description: type.description, accept: { [type.mime]: [type.extension] } }],
      })
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return false
      throw e
    }
  }
  const data = await content()
  if (file) {
    const stream = data instanceof Blob ? data.stream() : data
    await stream.pipeTo(await file.createWritable())
    return file.name
  }
  const url = URL.createObjectURL(data instanceof Blob ? data : await new Response(data).blob())
  Object.assign(document.createElement('a'), { href: url, download: name }).click()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
  return name
}
