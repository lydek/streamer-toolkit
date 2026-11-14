// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import ArrangeTiles from './ArrangeTiles.vue'
import { TILES, type Tile } from './home'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const extra: Tile[] = [
  { id: 'alerts', group: 'live', icon: '' },
  { id: 'checkin', group: 'features', icon: '' },
]
const names: Record<string, string> = { chat: '聊天室', alerts: '快訊', checkin: '打卡' }
const ok = () => Promise.resolve({ data: undefined, response: new Response(null, { status: 204 }) })
const failed = () =>
  Promise.resolve({ error: { code: 'settings.save_failed' }, response: new Response(null, { status: 500 }) })

let put: ReturnType<typeof vi.fn>

function arrange(saved: string[] | null, modules = [{ id: 'chat', enabled: true }]) {
  const wrapper = mount(ArrangeTiles, {
    props: {
      modelValue: saved,
      'onUpdate:modelValue': (v: string[] | null) => wrapper.setProps({ modelValue: v }),
      modules,
      name: (id: string) => names[id] ?? id,
    },
    global: { plugins: [i18n, PrimeVue] },
    attachTo: document.body,
  })
  return wrapper
}
const button = (tile: string, action: string) =>
  document.querySelector<HTMLButtonElement>(`[data-tile="${tile}"] [data-action="${action}"]`)!
const live = () => document.querySelector('[aria-live="polite"]')!.textContent

beforeEach(() => {
  // Tests need several tiles in a group; only chat exists so far.
  TILES.push(...extra)
  put = vi.fn(ok)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
})

enableAutoUnmount(afterEach)
afterEach(() => {
  TILES.splice(TILES.length - extra.length)
  vi.restoreAllMocks()
})

describe('ArrangeTiles', () => {
  it('moves within the group, keeps focus on the move button (or the other one at the edge) and announces', async () => {
    const wrapper = arrange(null)
    expect(button('chat', 'earlier').disabled).toBe(true)
    button('chat', 'later').focus()
    button('chat', 'later').click()
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', { body: ['alerts', 'chat', 'checkin'] })
    expect(wrapper.props('modelValue')).toEqual(['alerts', 'chat', 'checkin'])
    expect(document.activeElement).toBe(button('chat', 'earlier'))
    expect(live()).toBe('聊天室移到直播中第 2 個')
  })

  it('restores the order, focus and reports the error when the save fails', async () => {
    put.mockImplementation(failed)
    const wrapper = arrange(['chat', 'alerts'])
    button('chat', 'remove').click()
    await flushPromises()
    expect(wrapper.props('modelValue')).toEqual(['chat', 'alerts'])
    expect(wrapper.emitted('failed')).toEqual([['設定儲存失敗。']])
    expect(document.activeElement).toBe(button('chat', 'remove'))
  })

  it('after Remove focuses the next Remove, else the previous, else the Available heading', async () => {
    const wrapper = arrange(['chat', 'alerts'])
    button('chat', 'remove').click()
    await flushPromises()
    expect(document.activeElement).toBe(button('alerts', 'remove'))
    expect(live()).toBe('已移除聊天室')

    await wrapper.setProps({ modelValue: ['chat', 'alerts'] })
    button('alerts', 'remove').click()
    await flushPromises()
    expect(document.activeElement).toBe(button('chat', 'remove'))

    button('chat', 'remove').click()
    await flushPromises()
    expect(wrapper.props('modelValue')).toEqual([])
    expect(document.activeElement?.id).toBe('available-heading')
  })

  it('adds to the end of its group and focuses the first enabled button of the new tile', async () => {
    const wrapper = arrange(['chat'])
    document.querySelector<HTMLButtonElement>('[data-available="checkin"] [data-action="add"]')!.click()
    await flushPromises()
    expect(wrapper.props('modelValue')).toEqual(['chat', 'checkin'])
    // Alone in its group: both move buttons are disabled.
    expect(document.activeElement).toBe(button('checkin', 'remove'))
    expect(live()).toBe('打卡已加到功能第 1 個')

    document.querySelector<HTMLButtonElement>('[data-available="alerts"] [data-action="add"]')!.click()
    await flushPromises()
    expect(wrapper.props('modelValue')).toEqual(['chat', 'checkin', 'alerts'])
    expect(document.activeElement).toBe(button('alerts', 'earlier'))
  })

  it('restores the defaults with a null body', async () => {
    const wrapper = arrange([])
    wrapper.get('button').trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', { body: null })
    expect(wrapper.props('modelValue')).toBeNull()
    expect(live()).toBe('已還原預設功能磚')
  })

  it('shows disabled modules as text and tiles as list items with separate buttons', () => {
    arrange(['chat'], [{ id: 'chat', enabled: false }])
    const item = document.querySelector('[data-tile="chat"]')!
    expect(item.tagName).toBe('LI')
    expect(item.textContent).toContain('未啟用 · 到設定開啟')
    expect(item.querySelector('button button')).toBeNull()
    expect(button('chat', 'remove').getAttribute('aria-label')).toBe('移除：聊天室')
  })
})
