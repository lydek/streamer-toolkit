import { ApiError, type Schemas, api, client } from './api'

/** The Host's limit (crates/host/src/image_sizes.rs); checked here first so a larger file is never sent. */
export const IMAGE_SIZES_MAX_BYTES = 10 * 1024 * 1024
export const IMAGE_SIZES_ACCEPT = '.png,.jpg,.jpeg,.gif,.webp,image/png,image/jpeg,image/gif,image/webp'

export type ImageKind = Schemas['ImageKind']
export type Fit = Schemas['Fit']

function post(file: File, kind: ImageKind, fit: Fit, zip: boolean) {
  if (file.size > IMAGE_SIZES_MAX_BYTES) return Promise.reject(new ApiError('image_sizes.too_large'))
  return client.POST('/api/admin/image-sizes', {
    params: { query: { kind, fit, zip } },
    // The body is the file itself, not JSON.
    body: file as unknown as string,
    bodySerializer: (b: unknown) => b as BodyInit,
    headers: { 'Content-Type': 'application/octet-stream' },
    parseAs: zip ? 'blob' : 'json',
  })
}

/** Every size of `kind` with a preview, worked out on this computer by the Host. */
export const makeSizes = (file: File, kind: ImageKind, fit: Fit) =>
  api(post(file, kind, fit, false)) as Promise<Schemas['SizedImages']>

/** Every size of `kind` as one zip of PNG files. */
export const sizesZip = async (file: File, kind: ImageKind, fit: Fit) =>
  (await api(post(file, kind, fit, true))) as unknown as Blob

/** `name.png` → `name-badge.zip`. */
export const zipName = (file: string, kind: ImageKind) => `${file.replace(/\.[^.]*$/, '') || 'image'}-${kind}.zip`
