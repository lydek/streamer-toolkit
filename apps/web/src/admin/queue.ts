import type { Schemas } from './api'

/** About how long until everything in the queue has played, in ms, at `now` (the Host runs on this machine). */
export function waitMs(status: Schemas['QueueStatus'] | undefined, now: number): number {
  if (!status) return 0
  const gap = status.gapMs
  const current = status.current
    ? Math.max(0, Date.parse(status.current.startedAt) + status.current.showMs + gap - now)
    : 0
  return status.waiting.reduce((sum, w) => sum + w.showMs + gap, current)
}
