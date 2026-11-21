import { describe, expect, it } from 'vitest'
import { type Warnings, activeWarnings } from './warnings'

const AT = Date.parse('2026-11-22T12:00:00.000Z')
const w = (p: Partial<Warnings>): Warnings => ({
  chatOverlay: true,
  twitch: true,
  mic: true,
  micSeconds: 30,
  micInput: 'Mic',
  chatOverlayDown: false,
  twitchDown: false,
  micMutedSince: null,
  micMissing: false,
  ...p,
})

describe('activeWarnings', () => {
  it('shows each warning that is on and down', () => {
    expect(activeWarnings(w({}), AT)).toEqual([])
    expect(activeWarnings(w({ chatOverlayDown: true, twitchDown: true }), AT)).toEqual(['chatOverlay', 'twitch'])
    expect(activeWarnings(w({ chatOverlay: false, twitch: false, chatOverlayDown: true, twitchDown: true }), AT)).toEqual([])
  })

  it('waits until the mic has been muted for its seconds', () => {
    const muted = w({ micMutedSince: new Date(AT).toISOString() })
    expect(activeWarnings(muted, AT + 29_999)).toEqual([])
    expect(activeWarnings(muted, AT + 30_000)).toEqual(['mic'])
    expect(activeWarnings({ ...muted, mic: false }, AT + 60_000)).toEqual([])
  })
})
