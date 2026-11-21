import { ApiError, api, client } from './api'

/** The Host's limit (crates/modules/checkin); checked here first so a large file is never sent. */
export const CARD_IMAGE_MAX_BYTES = 5 * 1024 * 1024
/** No SVG: it can carry script. */
export const IMAGE_ACCEPT = '.png,.jpg,.jpeg,.gif,.webp,image/png,image/jpeg,image/gif,image/webp'

/** Each preset's own colors. */
export const PRESET_COLORS = {
  simple: { background: '#181624', stamp: '#d8b7ff' },
  journal: { background: '#fbf6ee', stamp: '#c0392b' },
} as const

/** Uploads `file` as-is; the Host checks its extension and content. */
export function importCardImage(file: File) {
  if (file.size > CARD_IMAGE_MAX_BYTES) return Promise.reject(new ApiError('checkin.image_too_large'))
  return api(
    client.POST('/api/admin/checkin/images', {
      params: { query: { name: file.name } },
      // The body is the file itself, not JSON.
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}
