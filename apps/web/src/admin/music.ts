import { i18n } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { measureSound } from './sounds'

/** The Host's limit (crates/modules/bgm); checked here first so a large file is never sent. */
export const MUSIC_MAX_BYTES = 50 * 1024 * 1024

/** Uploads `file` as-is; the Host checks its extension and content. */
export function importMusic(file: File) {
  if (file.size > MUSIC_MAX_BYTES) return Promise.reject(new ApiError('music.too_large'))
  return api(
    client.POST('/api/admin/music', {
      params: { query: { name: file.name } },
      // The body is the file itself, not JSON.
      body: file as unknown as string,
      bodySerializer: (b: unknown) => b as BodyInit,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  )
}

/** Measures every track that has no length yet and saves it; returns whether any was saved. */
export async function measureUnmeasured(tracks: { id: string; duration_ms: number | null; missing: boolean }[]) {
  let saved = false
  for (const track of tracks.filter((t) => t.duration_ms === null && !t.missing)) {
    // One track that cannot be measured or saved never holds up the others; it is tried again next time.
    try {
      const ms = await measureSound(`/music/${track.id}`)
      if (ms === undefined) continue
      await api(client.PUT('/api/admin/music/{id}/duration', { params: { path: { id: track.id } }, body: { duration_ms: ms } }))
      saved = true
    } catch {
      // Tried again the next time the page opens.
    }
  }
  return saved
}

/** `m:ss`, or `h:mm:ss` from an hour up. */
export function formatLength(ms: number) {
  const s = Math.round(ms / 1000)
  const [h, m, sec] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60]
  const pad = (n: number) => String(n).padStart(2, '0')
  return h ? `${h}:${pad(m)}:${pad(sec)}` : `${m}:${pad(sec)}`
}

/** What the player is doing, in a line: the off-air warning while it would play without a music overlay in OBS. */
export function playerLine(p: Schemas['BgmPlayerPayload']): string {
  const { t } = i18n.global
  if (!p.track || p.state === 'stopped') return t('music.player.stopped')
  if (!p.onAir) return t('music.player.offAir')
  return t(p.state === 'playing' ? 'music.player.playing' : 'music.player.paused', { title: p.track.title })
}
