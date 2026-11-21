import { describe, expect, it } from 'vitest'
import { type Marathon, remainingMs } from './marathon'

const AT = '2026-11-21T10:00:00.000Z'
const m = (p: Partial<Marathon>): Marathon => ({
  remainingMs: 60_000,
  running: true,
  paused: false,
  live: true,
  subMinutes: 5,
  bitsMinutes: 2,
  maxHours: 24,
  ...p,
})

describe('remainingMs', () => {
  it('counts down from the Host time only while running, never below 0', () => {
    const now = Date.parse(AT) + 20_000
    expect(remainingMs(m({}), AT, now)).toBe(40_000)
    expect(remainingMs(m({ running: false }), AT, now)).toBe(60_000)
    expect(remainingMs(m({}), AT, now + 60_000)).toBe(0)
  })
})
