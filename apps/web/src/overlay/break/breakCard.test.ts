import { describe, expect, it } from 'vitest'
import { type BreakCard, breakLeftMs, countdownText } from './breakCard'

const card = (p: Partial<BreakCard>): BreakCard => ({
  shown: true,
  text: '休息一下',
  endText: '馬上回來',
  endsAt: '2026-11-22T12:10:00.000Z',
  mode: 'minutes',
  minutes: 10,
  untilTime: '20:00',
  ...p,
})

describe('breakLeftMs', () => {
  it('counts to the stored end and stops at 0', () => {
    const now = Date.parse('2026-11-22T12:00:00.000Z')
    expect(breakLeftMs(card({}), now)).toBe(600_000)
    expect(breakLeftMs(card({}), now + 700_000)).toBe(0)
    expect(breakLeftMs(card({ endsAt: null }), now)).toBe(0)
  })
})

describe('countdownText', () => {
  it('shows minutes and seconds, hours only when needed', () => {
    expect(countdownText(582_000)).toBe('09:42')
    expect(countdownText(500)).toBe('00:01')
    expect(countdownText(0)).toBe('00:00')
    expect(countdownText(3_725_000)).toBe('1:02:05')
  })
})
