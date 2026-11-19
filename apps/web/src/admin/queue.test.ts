import { describe, expect, it } from 'vitest'
import { waitMs } from './queue'

const start = Date.parse('2026-11-19T10:00:00.000Z')

describe('waitMs', () => {
  it('adds what is left of the one playing to every waiting item and its gap', () => {
    const status = {
      current: {
        stage: 'alerts' as const,
        subject: { kind: 'alert' as const, alert: 'follow', viewer: 'Fan' },
        showMs: 6000,
        startedAt: '2026-11-19T10:00:00.000Z',
        problem: null,
      },
      waiting: [
        { stage: 'checkin' as const, subject: { kind: 'card' as const, viewer: 'Fish', stamps: 7, slots: 10 }, showMs: 5000, onAir: true },
        { stage: 'sound' as const, subject: { kind: 'sound' as const, sound: 'Bell', viewer: 'Kai' }, showMs: 1500, onAir: true },
      ],
      gapMs: 1000,
    }
    // 2 s into the alert: 5 s of it and its gap left, then 6 s and 2.5 s.
    expect(waitMs(status, start + 2000)).toBe(5000 + 6000 + 2500)
    // A late status never counts the one playing below zero.
    expect(waitMs({ ...status, waiting: [] }, start + 60_000)).toBe(0)
  })

  it('is zero for an empty or unknown queue', () => {
    expect(waitMs(undefined, start)).toBe(0)
    expect(waitMs({ current: null, waiting: [], gapMs: 1000 }, start)).toBe(0)
  })
})
