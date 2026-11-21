import type { components } from '../../api/schema'

export type BreakCard = components['schemas']['BreakPayload']

/** Time left on a shown card at `now`; 0 once its end has passed (or when it has none). */
export function breakLeftMs(card: BreakCard, now = Date.now()): number {
  return card.endsAt ? Math.max(0, Date.parse(card.endsAt) - now) : 0
}

/** `MM:SS`, or `H:MM:SS` from an hour up; a started second counts as a whole one. */
export function countdownText(ms: number): string {
  const total = Math.ceil(Math.max(0, ms) / 1000)
  const two = (n: number) => String(n).padStart(2, '0')
  const h = Math.floor(total / 3600)
  const mmss = `${two(Math.floor(total / 60) % 60)}:${two(total % 60)}`
  return h ? `${h}:${mmss}` : mmss
}
