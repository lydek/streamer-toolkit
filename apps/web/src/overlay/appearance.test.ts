// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { applyAppearance, followAppearance } from './appearance'

describe('applyAppearance', () => {
  it('sets the theme and text size as variables, and dark at 100 % as none', () => {
    const root = document.createElement('div')
    root.style.setProperty('--chat-text-size', '32px')
    applyAppearance(root, { theme: 'cream', textSize: 120 })
    expect(root.style.getPropertyValue('--ov-fg')).toBe('#3b2a1a')
    expect(root.style.getPropertyValue('--ov-scale')).toBe('1.2')
    applyAppearance(root, { theme: 'dark', textSize: 100 })
    expect(root.style.getPropertyValue('--ov-fg')).toBe('')
    expect(root.style.getPropertyValue('--ov-scale')).toBe('')
    expect(root.style.getPropertyValue('--chat-text-size')).toBe('32px')
  })
})

describe('followAppearance', () => {
  it('takes the snapshot, then only newer events', () => {
    const root = document.createElement('div')
    const follow = followAppearance(root)
    const event = (seq: number, theme: 'cream' | 'contrast') =>
      ({ id: '', type: 'screen.appearance', version: 1, seq, occurredAt: '', source: 'system', payload: { theme, textSize: 100 } }) as const
    follow({ type: 'snapshot', version: 1, seq: 5, payload: { 'screen.appearance': [event(5, 'contrast')] } } as never)
    expect(root.style.getPropertyValue('--ov-bg')).toBe('#000')
    follow(event(4, 'cream'))
    expect(root.style.getPropertyValue('--ov-bg')).toBe('#000')
    follow(event(6, 'cream'))
    expect(root.style.getPropertyValue('--ov-bg')).toBe('rgb(251 246 238 / 92%)')
  })
})
