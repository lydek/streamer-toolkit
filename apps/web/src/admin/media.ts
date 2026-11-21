import { ApiError, type Schemas, api, client } from './api'

/** The Host's limit (crates/modules/overlays); checked here first so a large file is never sent. */
export const MEDIA_MAX_BYTES = 50 * 1024 * 1024
export const MEDIA_ACCEPT = '.gif,.webm,.mp4,image/gif,video/webm,video/mp4'

/** Uploads `file` as-is; the Host checks its extension and content. */
export function importMedia(file: File) {
  if (file.size > MEDIA_MAX_BYTES) return Promise.reject(new ApiError('media.too_large'))
  return api(
    client.POST('/api/admin/screen-tools/media', {
      params: { query: { name: file.name } },
      // The body is the file itself, not JSON.
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}

/** A video's length in ms, read from its metadata; `undefined` when it cannot be read. */
export function measureVideo(url: string): Promise<number | undefined> {
  return new Promise((resolve) => {
    const video = document.createElement('video')
    video.preload = 'metadata'
    video.muted = true
    video.onloadedmetadata = () => {
      const ms = Math.round(video.duration * 1000)
      resolve(Number.isFinite(ms) && ms > 0 ? ms : undefined)
    }
    video.onerror = () => resolve(undefined)
    video.src = url
  })
}

export const saveMedia = (item: Schemas['MediaItem'], patch: Partial<Schemas['MediaUpdate']>) =>
  api(
    client.PUT('/api/admin/screen-tools/media/{id}', {
      params: { path: { id: item.id } },
      body: { title: item.title, duration_ms: null, ...patch },
    }),
  )

/** Reads and saves the length of every video that has none yet; returns whether any was saved. */
export async function measureUnmeasured(items: Schemas['MediaItem'][], measure = measureVideo) {
  let saved = false
  for (const item of items.filter((i) => i.duration_ms == null && !i.missing)) {
    // One video that cannot be read or saved never holds up the others; it is tried again next time.
    try {
      const ms = await measure(item.url)
      if (ms === undefined) continue
      await saveMedia(item, { duration_ms: ms })
      saved = true
    } catch {
      // Tried again the next time the page opens.
    }
  }
  return saved
}
