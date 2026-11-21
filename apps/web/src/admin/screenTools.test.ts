import { describe, expect, it } from 'vitest'
import { countdownOf, nextUntil } from './screenTools'

describe('nextUntil', () => {
  it('is later today, or tomorrow once the time has passed', () => {
    const now = new Date(2026, 10, 22, 19, 30)
    expect(nextUntil('20:00', now)).toEqual(new Date(2026, 10, 22, 20, 0))
    expect(nextUntil('19:30', now)).toEqual(new Date(2026, 10, 23, 19, 30))
    expect(nextUntil('08:15', now)).toEqual(new Date(2026, 10, 23, 8, 15))
  })
})

describe('countdownOf', () => {
  it('sends the minutes, or the next moment of the time of day', () => {
    const now = new Date(2026, 10, 22, 19, 30)
    expect(countdownOf({ mode: 'minutes', minutes: 10, untilTime: '20:00' }, now)).toEqual({ minutes: 10 })
    expect(countdownOf({ mode: 'until', minutes: 10, untilTime: '20:00' }, now)).toEqual({
      until: new Date(2026, 10, 22, 20, 0).toISOString(),
    })
  })
})
