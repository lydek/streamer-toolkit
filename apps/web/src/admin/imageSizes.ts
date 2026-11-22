import { ApiError, type Schemas, api, client } from './api'

/** The Host's limit (crates/host/src/image_sizes.rs); checked here first so a larger file is never sent. */
export const IMAGE_SIZES_MAX_BYTES = 10 * 1024 * 1024
export const IMAGE_SIZES_ACCEPT = '.png,.jpg,.jpeg,.gif,.webp,image/png,image/jpeg,image/gif,image/webp'

export type ImageKind = Schemas['ImageKind']
export type Fit = Schemas['Fit']

/** A square of the picture, in its own pixels, for `fit = manual`. */
export interface Crop {
  x: number
  y: number
  side: number
}

/** The largest square in the middle: where the frame starts for each new picture. */
export function centeredCrop(width: number, height: number): Crop {
  const side = Math.min(width, height)
  return { x: Math.floor((width - side) / 2), y: Math.floor((height - side) / 2), side }
}

const clamp = (n: number, low: number, high: number) => Math.min(high, Math.max(low, n))

/** `c` moved by `dx`, `dy`, kept inside the picture. */
export function moveCrop(c: Crop, dx: number, dy: number, width: number, height: number): Crop {
  return { ...c, x: clamp(c.x + dx, 0, width - c.side), y: clamp(c.y + dy, 0, height - c.side) }
}

/** `c` grown by `d` (shrunk when negative) from its top-left corner, at least 8 px and never past the picture. */
export function resizeCrop(c: Crop, d: number, width: number, height: number): Crop {
  return { ...c, side: clamp(c.side + d, Math.min(8, width - c.x, height - c.y), Math.min(width - c.x, height - c.y)) }
}

function post(file: File, kind: ImageKind, fit: Fit, zip: boolean, crop?: Crop) {
  if (file.size > IMAGE_SIZES_MAX_BYTES) return Promise.reject(new ApiError('image_sizes.too_large'))
  const square = fit === 'manual' && crop ? { crop_x: crop.x, crop_y: crop.y, crop_side: crop.side } : {}
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
