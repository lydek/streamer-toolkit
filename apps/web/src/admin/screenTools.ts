import type { BreakCard } from '../overlay/break/breakCard'
import { api, client } from './api'

/** The next moment the local clock shows `hhmm` (`HH:MM`): later today, else tomorrow. */
export function nextUntil(hhmm: string, now = new Date()): Date {
  const [h, m] = hhmm.split(':').map(Number)
  const at = new Date(now)
  at.setHours(h, m, 0, 0)
  if (at <= now) at.setDate(at.getDate() + 1)
  return at
}

/** What starts the countdown: the saved minutes, or the next time the clock shows the saved time. */
export const countdownOf = (card: Pick<BreakCard, 'mode' | 'minutes' | 'untilTime'>, now = new Date()) =>
  card.mode === 'until' ? { until: nextUntil(card.untilTime, now).toISOString() } : { minutes: card.minutes }

/** Shows the break card counting down from now. */
export const showBreak = (body: { minutes: number } | { until: string }) =>
  api(client.POST('/api/admin/screen-tools/break/show', { body }))

export const hideBreak = () => api(client.POST('/api/admin/screen-tools/break/hide'))
