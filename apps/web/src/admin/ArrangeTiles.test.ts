// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { type Mock, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import ArrangeTiles from './ArrangeTiles.vue'
import { TILES, type Tile } from './home'

const extra: Tile[] = [{ id: 'badge', group: 'features', icon: '' }]
const names: Record<string, string> = { chat: '聊天室', alerts: '快訊', badge: '徽章' }

// What the page's save does: shows `next` at once, puts the previous order back on failure (`outcome` false),
// changes nothing while busy (`outcome` undefined).
let outcome: boolean | undefined
let save: Mock<(next: string[] | null) => Promise<boolean> | undefined>

function arrange(saved: string[] | null, modules = [{ id: 'chat', enabled: true, closed: false }]) {
  save = vi.fn((next: string[] | null) => {
    const ok = outcome
    if (ok === undefined) return undefined
    const previous = wrapper.props('saved')
    wrapper.setProps({ saved: next })
    return Promise.resolve().then(async () => {
      if (!ok) await wrapper.setProps({ saved: previous })
      return ok
    })
  })
  const wrapper = mount(ArrangeTiles, {
    props: { saved, save: (next: string[] | null) => save(next), modules, name: (id: string) => names[id] ?? id },
    global: { plugins: [i18n, PrimeVue] },
    attachTo: document.body,
  })
  return wrapper
}
const button = (tile: string, action: string) =>
  document.querySelector<HTMLButtonElement>(`[data-tile="${tile}"] [data-action="${action}"]`)!
const live = () => document.querySelector('[aria-live="polite"]')!.textContent

beforeEach(() => {
  // A tile of a later feature, to test adding one that was never arranged.
  TILES.push(...extra)
  outcome = true
})

enableAutoUnmount(afterEach)
afterEach(() => {
  TILES.splice(TILES.length - extra.length)
})

describe('ArrangeTiles', () => {
  it('moves within the group, keeps focus on the move button (or the other one at the edge) and announces', async () => {
    const wrapper = arrange(null)
    expect(button('chat', 'earlier').disabled).toBe(true)
    button('chat', 'later').focus()
    button('chat', 'later').click()
    await flushPromises()
    expect(save).toHaveBeenCalledWith(['alerts', 'chat', 'redemption', 'checkin', 'sound', 'scene', 'bgm', 'counter', 'badge'])
    expect(wrapper.props('saved')).toEqual(['alerts', 'chat', 'redemption', 'checkin', 'sound', 'scene', 'bgm', 'counter', 'badge'])
    expect(document.activeElement).toBe(button('chat', 'earlier'))
    expect(live()).toBe('聊天室移到直播中第 2 個')
  })

  it('puts focus back and announces nothing when the save fails', async () => {
    outcome = false
    const wrapper = arrange(['chat', 'alerts'])
    button('chat', 'remove').click()
    await flushPromises()
    expect(wrapper.props('saved')).toEqual(['chat', 'alerts'])
    expect(document.activeElement).toBe(button('chat', 'remove'))
    expect(live()).toBe('')
  })

  it('leaves focus alone while another save runs', async () => {
    outcome = undefined
    arrange(['chat', 'alerts'])
    button('chat', 'remove').focus()
    button('chat', 'remove').click()
    await flushPromises()
    expect(document.activeElement).toBe(button('chat', 'remove'))
  })

  it('after Remove focuses the next Remove, else the previous, else the Available heading', async () => {
    const wrapper = arrange(['chat', 'alerts'])
    button('chat', 'remove').click()
    await flushPromises()
    expect(document.activeElement).toBe(button('alerts', 'remove'))
    expect(live()).toBe('已移除聊天室')

    await wrapper.setProps({ saved: ['chat', 'alerts'] })
    button('alerts', 'remove').click()
    await flushPromises()
    expect(document.activeElement).toBe(button('chat', 'remove'))

    button('chat', 'remove').click()
    await flushPromises()
    expect(wrapper.props('saved')).toEqual([])
    expect(document.activeElement?.id).toBe('available-heading')
  })

  it('adds to the end of its group and focuses the first enabled button of the new tile', async () => {
    const wrapper = arrange(['chat'])
    document.querySelector<HTMLButtonElement>('[data-available="badge"] [data-action="add"]')!.click()
    await flushPromises()
    expect(wrapper.props('saved')).toEqual(['chat', 'badge'])
    // Alone in its group: both move buttons are disabled.
    expect(document.activeElement).toBe(button('badge', 'remove'))
    expect(live()).toBe('徽章已加到功能第 1 個')

    document.querySelector<HTMLButtonElement>('[data-available="alerts"] [data-action="add"]')!.click()
    await flushPromises()
    expect(wrapper.props('saved')).toEqual(['chat', 'badge', 'alerts'])
    expect(document.activeElement).toBe(button('alerts', 'earlier'))
  })

  it('restores the defaults by saving null', async () => {
    const wrapper = arrange([])
    wrapper.get('button').trigger('click')
    await flushPromises()
    expect(save).toHaveBeenCalledWith(null)
    expect(wrapper.props('saved')).toBeNull()
    expect(live()).toBe('已還原預設功能磚')
  })

  it('shows disabled modules as text and tiles as list items with separate buttons', () => {
    arrange(['chat'], [{ id: 'chat', enabled: false, closed: false }])
    const item = document.querySelector('[data-tile="chat"]')!
    expect(item.tagName).toBe('LI')
    expect(item.textContent).toContain('未啟用 · 到設定開啟')
    expect(item.querySelector('button button')).toBeNull()
    expect(button('chat', 'remove').getAttribute('aria-label')).toBe('移除：聊天室')
  })
})
