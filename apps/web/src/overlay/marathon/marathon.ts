import type { components } from '../../api/schema'

export type Marathon = components['schemas']['MarathonPayload']

/** The remaining time at `now`: the Host's at `occurredAt`, counted down since while it runs, never below 0. */
export function remainingMs(m: Marathon, occurredAt: string, now = Date.now()): number {
  const elapsed = m.running ? Math.max(0, now - Date.parse(occurredAt)) : 0
  return Math.max(0, m.remainingMs - elapsed)
}
