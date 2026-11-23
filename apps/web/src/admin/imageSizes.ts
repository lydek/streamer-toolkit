import { ApiError, type Schemas, api, client } from './api'

/** The Host's limit (crates/host/src/image_sizes.rs); checked here first so a larger file is never sent. */
export const IMAGE_SIZES_MAX_BYTES = 10 * 1024 * 1024
export const IMAGE_SIZES_ACCEPT = '.png,.jpg,.jpeg,.gif,.webp,image/png,image/jpeg,image/gif,image/webp'

export type ImageKind = Schemas['ImageKind']
export type Fit = Schemas['Fit']

/** A rectangle of the picture, in its own pixels, for `fit = manual` (the Host pads it to a square). */
export interface Crop {
  x: number
  y: number
  w: number
  h: number
}

/** Smallest side, where the picture allows it. */
const MIN = 8

/** The largest square in the middle: where the frame starts for each new picture. */
export function centeredCrop(width: number, height: number): Crop {
  const side = Math.min(width, height)
  return { x: Math.floor((width - side) / 2), y: Math.floor((height - side) / 2), w: side, h: side }
}

const clamp = (n: number, low: number, high: number) => Math.min(high, Math.max(low, n))

/**
 * A width and height that fit in `maxW` × `maxH` and are at least 8 px where that allows. With `ratio` (width ÷
 * height) one factor scales both sides, so the minimum and the edge keep the ratio (up to whole pixels); `w` leads.
 */
export function fitSize(w: number, h: number, maxW: number, maxH: number, ratio?: number): { w: number; h: number } {
  if (!ratio) return { w: clamp(Math.round(w), Math.min(MIN, maxW), maxW), h: clamp(Math.round(h), Math.min(MIN, maxH), maxH) }
  let fw = Math.max(1, w)
  let fh = fw / ratio
  const up = Math.max(1, Math.min(MIN, maxW) / fw, Math.min(MIN, maxH) / fh)
  fw *= up
  fh *= up
  const down = Math.min(1, maxW / fw, maxH / fh)
  return { w: Math.max(1, Math.round(fw * down)), h: Math.max(1, Math.round(fh * down)) }
}

/** `c` moved by `dx`, `dy`, kept inside the picture. */
export function moveCrop(c: Crop, dx: number, dy: number, width: number, height: number): Crop {
  return { ...c, x: clamp(c.x + dx, 0, width - c.w), y: clamp(c.y + dy, 0, height - c.h) }
}

export type Corner = 'nw' | 'ne' | 'sw' | 'se'

/**
 * `c` resized by dragging `corner` by `dx`, `dy`; the opposite corner stays put and the rectangle stays inside the
 * picture. With `ratio`, the axis that moved more leads and the other follows.
 */
export function resizeCrop(c: Crop, corner: Corner, dx: number, dy: number, width: number, height: number, ratio?: number): Crop {
  const sx = corner.endsWith('e') ? 1 : -1
  const sy = corner.startsWith('s') ? 1 : -1
  const right = c.x + c.w
  const bottom = c.y + c.h
  const maxW = sx > 0 ? width - c.x : right
  const maxH = sy > 0 ? height - c.y : bottom
  let w = c.w + sx * dx
  let h = c.h + sy * dy
  if (ratio) {
    const byH = h * ratio
    w = Math.abs(byH - c.w) > Math.abs(w - c.w) ? byH : w
    h = w / ratio
  }
  const size = fitSize(w, h, maxW, maxH, ratio)
  return { x: sx > 0 ? c.x : right - size.w, y: sy > 0 ? c.y : bottom - size.h, ...size }
}

/** `c` with one field typed in: X and Y move it (kept inside), W and H resize it from its top-left corner. */
export function setCropField(c: Crop, field: keyof Crop, value: number, width: number, height: number, ratio?: number): Crop {
  if (field === 'x') return { ...c, x: clamp(Math.round(value), 0, width - c.w) }
  if (field === 'y') return { ...c, y: clamp(Math.round(value), 0, height - c.h) }
  const size =
    field === 'w'
      ? fitSize(value, ratio ? value / ratio : c.h, width - c.x, height - c.y, ratio)
      : fitSize(ratio ? value * ratio : c.w, value, width - c.x, height - c.y, ratio)
  return { ...c, ...size }
}

function post(file: File, kind: ImageKind, fit: Fit, zip: boolean, crop?: Crop) {
  if (file.size > IMAGE_SIZES_MAX_BYTES) return Promise.reject(new ApiError('image_sizes.too_large'))
  const square = fit === 'manual' && crop ? { crop_x: crop.x, crop_y: crop.y, crop_w: crop.w, crop_h: crop.h } : {}
  return client.POST('/api/admin/image-sizes', {
    params: { query: { kind, fit, zip, ...square } },
    // The body is the file itself, not JSON.
    body: file as unknown as string,
    bodySerializer: (b: unknown) => b as BodyInit,
    headers: { 'Content-Type': 'application/octet-stream' },
    parseAs: zip ? 'blob' : 'json',
  })
}

/** Every size of `kind` with a preview, worked out on this computer by the Host. */
export const makeSizes = (file: File, kind: ImageKind, fit: Fit, crop?: Crop) =>
  api(post(file, kind, fit, false, crop)) as Promise<Schemas['SizedImages']>

/** Every size of `kind` as one zip of PNG files. */
export const sizesZip = async (file: File, kind: ImageKind, fit: Fit, crop?: Crop) =>
  (await api(post(file, kind, fit, true, crop))) as unknown as Blob

/** `name.png` → `name-badge.zip`. */
export const zipName = (file: string, kind: ImageKind) => `${file.replace(/\.[^.]*$/, '') || 'image'}-${kind}.zip`
