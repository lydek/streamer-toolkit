import type { components } from '../../api/schema'

export type Warnings = components['schemas']['WarningsPayload']
export type WarningId = 'chatOverlay' | 'mic' | 'twitch'

/** The warnings to show at `now`: each one that is on and down; the mic once it has been muted `micSeconds`. */
export function activeWarnings(w: Warnings, now = Date.now()): WarningId[] {
  const shown: WarningId[] = []
  if (w.chatOverlay && w.chatOverlayDown) shown.push('chatOverlay')
  if (w.mic && w.micMutedSince && now - Date.parse(w.micMutedSince) >= w.micSeconds * 1000) shown.push('mic')
  if (w.twitch && w.twitchDown) shown.push('twitch')
  return shown
}
