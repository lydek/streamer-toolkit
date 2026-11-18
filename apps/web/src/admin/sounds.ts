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

/** How long the sound at `url` plays, in ms, read from its metadata; `undefined` when it cannot be read. */
export function measureSound(url: string): Promise<number | undefined> {
  return new Promise((resolve) => {
    const audio = new Audio()
    audio.preload = 'metadata'
    audio.onloadedmetadata = () => {
      const ms = Math.round(audio.duration * 1000)
      resolve(Number.isFinite(ms) && ms > 0 ? ms : undefined)
    }
    audio.onerror = () => resolve(undefined)
    audio.src = url
  })
}

/**
 * Measures every library sound that has no length yet and saves it, so reward sounds hold the presentation queue for
 * as long as they play. Runs when the admin window opens (sounds imported before lengths were kept) and after an
 * import. A sound that cannot be measured is tried again next time.
 */
export async function measureUnmeasured() {
  const library = await api(client.GET('/api/admin/sounds'))
  for (const s of library.sounds.filter((s) => s.duration_ms === null)) {
    // One sound that cannot be measured or saved never holds up the others.
    try {
      const ms = await measureSound(`/sounds/${s.id}`)
      if (ms !== undefined) {
        await api(client.PUT('/api/admin/sounds/{id}/duration', { params: { path: { id: s.id } }, body: { duration_ms: ms } }))
      }
    } catch {
      // Tried again the next time the window opens.
    }
  }
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
