import type { components } from '../../api/schema'

export type BgmPlayer = components['schemas']['BgmPlayerPayload']

/** The audio only seeks when it is further off than this, so a volume change does not make it skip. */
const DRIFT_MS = 1000

/** Where the track is at `now`: the Host's position at `occurredAt`, plus the time since while its clock runs. */
export function positionMs(p: BgmPlayer, occurredAt: string, now = Date.now()): number {
  const running = p.state === 'playing' && p.onAir
  const at = p.positionMs + (running ? Math.max(0, now - Date.parse(occurredAt)) : 0)
  return Math.min(at, p.track?.durationMs ?? 0)
}

/** Makes `audio` play what the Host's player says, from where it says. */
export function follow(audio: HTMLAudioElement, p: BgmPlayer, occurredAt: string) {
  audio.volume = Math.min(Math.max(p.volume, 0), 100) / 100
  if (!p.track || p.state === 'stopped') {
    audio.pause()
    audio.removeAttribute('src')
    return
  }
  // The attribute, not `audio.src`, which is the absolute URL.
  if (audio.getAttribute('src') !== p.track.url) audio.src = p.track.url
  const target = positionMs(p, occurredAt) / 1000
  if (Math.abs(audio.currentTime - target) * 1000 > DRIFT_MS) audio.currentTime = target
  // OBS lets browser sources play without a click; elsewhere a refusal only means silence.
  if (p.state === 'playing') audio.play()?.catch(() => {})
  else audio.pause()
}
