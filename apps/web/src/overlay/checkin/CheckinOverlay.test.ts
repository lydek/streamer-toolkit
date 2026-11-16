// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { i18n } from '../../i18n'
import CheckinOverlay from './CheckinOverlay.vue'
import { twitchImage } from './checkin'

const AVATAR = 'https://static-cdn.jtvnw.net/jtv_user_pictures/x-profile_image-300x300.png'

function stamped(seq: number, over: Record<string, unknown> = {}) {
  return {
    id: `e${seq}`,
    type: 'checkin.stamped' as const,
    version: 1,
    seq,
    occurredAt: '2026-11-16T10:00:00.000Z',
    source: 'twitch' as const,
    payload: { userName: 'Ann', avatarUrl: AVATAR, stamps: 3, slots: 10, total: 13, full: false, durationMs: 5000, ...over },
  }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('WebSocket', class {
    close() {}
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('CheckinOverlay', () => {
  it('shows a card as text for its duration, then hides it', async () => {
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    vm.receive({ type: 'snapshot', version: 1, seq: 4, payload: {} })
    vm.receive(stamped(5, { userName: '<img src=x onerror=alert(1)>' }))
    await nextTick()
    const card = wrapper.get('[data-checkin-card]')
    expect(card.get('.name').text()).toBe('<img src=x onerror=alert(1)>')
    expect(card.findAll('img')).toHaveLength(1) // the avatar only
    expect(card.get('.avatar').attributes('src')).toBe(AVATAR)
    expect(card.findAll('.slots li.stamped')).toHaveLength(3)
    expect(card.findAll('.slots li')).toHaveLength(10)
    expect(card.classes()).not.toContain('full')

    vm.receive(stamped(5, { userName: 'Again' })) // an old seq is ignored
    await nextTick()
    expect(wrapper.get('.name').text()).toBe('<img src=x onerror=alert(1)>')

    vi.advanceTimersByTime(5000)
    await nextTick()
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(false)
  })

  it('marks a full card and drops images from anywhere but Twitch', async () => {
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    vm.receive(stamped(1, { stamps: 10, full: true, avatarUrl: 'https://evil.example/x.png' }))
    await nextTick()
    const card = wrapper.get('[data-checkin-card]')
    expect(card.classes()).toContain('full')
    expect(card.text()).toContain(i18n.global.t('overlay.checkin.full'))
    expect(card.find('.avatar').exists()).toBe(false)
  })

  it('accepts only https images on static-cdn.jtvnw.net', () => {
    expect(twitchImage(AVATAR)).toBe(AVATAR)
    for (const bad of ['http://static-cdn.jtvnw.net/x', 'https://static-cdn.jtvnw.net.evil/x', 'javascript:alert(1)', 'nope']) {
      expect(twitchImage(bad), bad).toBeUndefined()
    }
  })
})
