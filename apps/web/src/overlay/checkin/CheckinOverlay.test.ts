// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { i18n } from '../../i18n'
import CheckinOverlay from './CheckinOverlay.vue'
import { twitchImage } from './checkin'

const played = vi.hoisted(() => [] as [string, number][])
vi.mock('../sound', () => ({ playSound: (url: string, volume: number) => played.push([url, volume]), stopSounds: () => {} }))

const AVATAR = 'https://static-cdn.jtvnw.net/jtv_user_pictures/x-profile_image-300x300.png'

function stamped(seq: number, over: Record<string, unknown> = {}) {
  return {
    id: `e${seq}`,
    type: 'checkin.stamped' as const,
    version: 1,
    seq,
    occurredAt: '2026-11-16T10:00:00.000Z',
    source: 'twitch' as const,
    payload: {
      userName: 'Ann',
      avatarUrl: AVATAR,
      stamps: 3,
      slots: 10,
      total: 13,
      full: false,
      durationMs: 5000,
      sound: null,
      ...over,
    },
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

  it('shows the card style: texts as text, colors, row size, preset and only the Host’s own images', async () => {
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    vm.receive({ type: 'snapshot', version: 1, seq: 1, payload: {} })
    vm.receive(
      stamped(2, {
        title: '<b>Ann</b> 打卡',
        description: '累計 13 次',
        style: {
          preset: 'journal',
          backgroundColor: '#fbf6ee',
          backgroundUrl: 'https://evil.example/x.png',
          stampColor: '#c0392b',
          stampUrl: '/checkin/0190a1b2-0000-7000-8000-000000000001.png',
          perRow: 5,
          animation: 'pop',
        },
      }),
    )
    await nextTick()
    const card = wrapper.get('[data-checkin-card]')
    expect(card.get('.name').text()).toBe('<b>Ann</b> 打卡')
    expect(card.find('.name b').exists()).toBe(false)
    expect(card.get('.total').text()).toBe('累計 13 次')
    expect(card.classes()).toEqual(expect.arrayContaining(['preset-journal', 'anim-pop']))
    const style = (card.element as HTMLElement).style
    expect(style.getPropertyValue('--per-row')).toBe('5')
    expect(style.getPropertyValue('--stamp')).toBe('#c0392b')
    expect(style.getPropertyValue('--card-image')).toBe('none')
    expect(style.getPropertyValue('--stamp-image')).toContain('/checkin/')
    // 手帳's paper is light, so its text is dark.
    expect(style.getPropertyValue('--card-fg')).toBe('#1f1a17')
  })

  it('in preview, takes only messages from the parent on the same origin and keeps the card', async () => {
    const socket = vi.fn()
    vi.stubGlobal('WebSocket', socket)
    history.replaceState(null, '', '/overlay/checkin/?preview')
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    expect(socket).not.toHaveBeenCalled()
    const post = (data: unknown, init: MessageEventInit = {}) =>
      window.dispatchEvent(new MessageEvent('message', { data, source: window.parent, origin: location.origin, ...init }))
    post({ type: 'snapshot', version: 1, seq: 1, payload: {} })
    post(stamped(2, { userName: 'Evil' }), { origin: 'https://evil.example' })
    post(stamped(3, { userName: 'Other' }), { source: null })
    await nextTick()
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(false)
    post(stamped(4, { title: '小魚' }))
    await nextTick()
    expect(wrapper.get('.name').text()).toBe('小魚')
    vi.advanceTimersByTime(60_000)
    await nextTick()
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(true)
    // The CSS leave animation waits for the duration too, so the card does not fade out either.
    expect(wrapper.get<HTMLElement>('[data-checkin-card]').element.style.getPropertyValue('--duration')).toBe('86400000ms')
    wrapper.unmount()
    history.replaceState(null, '', '/')
  })

  it('shows a pack’s card when the style names a pack, and the built-in card when the pack is gone', async () => {
    const PACK = '0190a1b2-0000-7000-8000-000000000001'
    const html =
      '<style>.pack-card { color: red }</style><div data-card-stage></div><template data-card><p class="pack-card" data-field="title"></p></template>'
    let gone = false
    vi.stubGlobal('fetch', vi.fn(async () => (gone ? new Response('', { status: 404 }) : new Response(html))))
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    const style = { preset: `pack:${PACK}`, backgroundColor: '#000000', backgroundUrl: null, stampColor: '#ffffff', stampUrl: null, perRow: 5, animation: 'none' }
    vm.receive({ type: 'snapshot', version: 1, seq: 1, payload: {} })
    vm.receive(stamped(2, { title: 'Pack Ann', style }))
    await flushPromises()
    await flushPromises()
    await nextTick()
    expect(wrapper.find('.pack-card').text()).toBe('Pack Ann')
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(false)
    expect(vi.mocked(fetch).mock.calls[0][0].toString()).toContain(`/templates/checkin/${PACK}/index.html`)

    gone = true
    vm.receive(stamped(3, { title: 'Back to built-in', style }))
    await flushPromises()
    await flushPromises()
    await nextTick()
    expect(wrapper.find('.pack-card').exists()).toBe(false)
    expect(wrapper.get('[data-checkin-card] .name').text()).toBe('Back to built-in')
    expect(wrapper.get('[data-checkin-pack]').element.childElementCount, "the pack's styles are gone too").toBe(0)
  })

  it('takes the card away at once on 隱藏畫面卡片', async () => {
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    vm.receive({ type: 'snapshot', version: 1, seq: 1, payload: {} })
    vm.receive(stamped(2))
    await nextTick()
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(true)
    vm.receive({ id: 'h', type: 'checkin.hidden', version: 1, seq: 3, occurredAt: '', source: 'system', payload: {} })
    await nextTick()
    expect(wrapper.find('[data-checkin-card]').exists()).toBe(false)
  })

  it('plays the card’s sound at its volume, and nothing without one', () => {
    played.length = 0
    const wrapper = mount(CheckinOverlay, { global: { plugins: [i18n] } })
    const vm = wrapper.vm as unknown as { receive: (m: unknown) => void }
    vm.receive(stamped(1))
    vm.receive(stamped(2, { full: true, sound: { url: '/sounds/s1', volume: 40 } }))
    expect(played).toEqual([['/sounds/s1', 40]])
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
