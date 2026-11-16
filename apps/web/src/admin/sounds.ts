import type { ComposerTranslation } from 'vue-i18n'
import { ApiError, api, client } from './api'

/** The Host's limit (crates/modules/sound); checked here first so a large file is never sent. */
export const SOUND_MAX_BYTES = 10 * 1024 * 1024

/** Uploads `file` as-is; the Host checks its extension and content. */
export function importSound(file: File) {
  if (file.size > SOUND_MAX_BYTES) return Promise.reject(new ApiError('sound.too_large'))
  return api(
    client.POST('/api/admin/sounds', {
      params: { query: { name: file.name } },
      // The body is the file itself, not JSON.
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}

/** Where a slot is, for the "used by" list: e.g. `alert.follow` → 特效：追隨. `rewards`: reward ID → title. */
export function slotLabel(slot: string, t: ComposerTranslation, rewards = new Map<string, string>()): string {
  const [area, rest] = slot.split(/\.(.*)/)
  if (area === 'alert') return t('sound.where.alert', { kind: t(`alerts.kinds.${rest}`) })
  if (area === 'reward') return t('sound.where.reward', { title: rewards.get(rest) ?? rest })
  if (slot === 'checkin') return t('sound.where.checkin')
  if (slot === 'checkin.full') return t('sound.where.checkinFull')
  return slot
}
