import { ApiError, type Schemas, api, client } from './api'

/** The Host's limit (crates/modules/checkin); checked here first so a large file is never sent. */
export const CARD_IMAGE_MAX_BYTES = 5 * 1024 * 1024
/** No SVG: it can carry script. */
export const IMAGE_ACCEPT = '.png,.jpg,.jpeg,.gif,.webp,image/png,image/jpeg,image/gif,image/webp'

/** Each preset's own colors. */
export const PRESET_COLORS = {
  simple: { background: '#181624', stamp: '#d8b7ff' },
  journal: { background: '#fbf6ee', stamp: '#c0392b' },
} as const

/** The Host's limit for an animation pack zip (crates/host/src/card_packs.rs). */
export const CARD_PACK_MAX_BYTES = 10 * 1024 * 1024

/** Uploads a pack zip as-is; the Host checks every file in it. */
export function uploadCardPack(file: File) {
  if (file.size > CARD_PACK_MAX_BYTES) return Promise.reject(new ApiError('card_pack.too_large', { detail: file.name }))
  return api(
    client.POST('/api/admin/checkin/packs', {
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/zip' },
    }),
  )
}

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

type Style = Schemas['CardStyleBody']
type Stamped = Schemas['CheckinStampedPayload']

/** The Host's variable filling (crates/modules/checkin), for the preview: the viewer name last. */
export function fillCard(text: string, viewer: string, stamps: number, total: number, slots: number): string {
  return text
    .replaceAll('#章數#', String(stamps))
    .replaceAll('#次數#', String(total))
    .replaceAll('#格數#', String(slots))
    .replaceAll('#觀眾#', viewer)
}

/** A sample card in `style`, as `checkin.stamped` carries it; `repeat` shows the card of a second check-in today. */
export function previewCard(style: Style, slots: number, repeat: boolean): Stamped {
  const viewer = '小魚'
  const total = 12
  const stamps = ((total - 1) % slots) + 1
  const url = (name?: string | null) => (name ? `/checkin/${name}` : null)
  return {
    userName: viewer,
    avatarUrl: null,
    stamps,
    slots,
    total,
    full: !repeat && stamps === slots,
    durationMs: 5000,
    sound: null,
    title: fillCard(style.title, viewer, stamps, total, slots),
    description: fillCard(style.description, viewer, stamps, total, slots),
    style: {
      preset: style.preset,
      backgroundColor: style.background_color,
      backgroundUrl: url(style.background_image),
      stampColor: style.stamp_color,
      stampUrl: url(style.stamp_image),
      perRow: style.per_row,
      animation: style.animation,
    },
  }
}
