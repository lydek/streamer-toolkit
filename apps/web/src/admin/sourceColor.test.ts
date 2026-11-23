import { describe, expect, it } from 'vitest'
import { colorOf, hexOf } from './sourceColor'

describe('OBS text colors', () => {
  it('turn reversed bytes into #rrggbb and back, with an opaque alpha for color1', () => {
    expect(hexOf(0x0000ff)).toBe('#ff0000')
    expect(hexOf(0xff66d1ff)).toBe('#ffd166')
    expect(colorOf('#ff0000', false)).toBe(0x0000ff)
    expect(colorOf('#ffd166', true)).toBe(0xff66d1ff)
    expect(hexOf(colorOf('#1f2937', true))).toBe('#1f2937')
  })
})
