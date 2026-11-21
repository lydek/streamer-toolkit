// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import LeaderboardPage from './LeaderboardPage.vue'
import { testRouter } from './testRouter'

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('WebSocket', class { close() {} })
  vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn(async () => {}) } })
  vi.spyOn(client, 'GET').mockResolvedValue({
    data: { boards: [{ board: 'checkins', period: 'all', entries: [{ name: '小魚', value: 42 }] }] },
    response: new Response(null, { status: 200 }),
  } as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('takes the boards from a reconnect snapshot', async () => {
  const sockets: { onmessage?: (e: { data: string }) => void }[] = []
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        sockets.push(this)
      }
      close() {}
    },
  )
  const wrapper = mount(LeaderboardPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  await wrapper.findAll('[aria-labelledby="lb-period"] button').find((b) => b.text() === '全部')!.trigger('click')
  await wrapper.findAll('[aria-labelledby="lb-board"] button').find((b) => b.text() === '打卡')!.trigger('click')
  const boards = { boards: [{ board: 'checkins', period: 'all', entries: [{ name: '莓莓', value: 50 }] }] }
  const update = { id: 'u', type: 'leaderboard.update', version: 1, seq: 7, occurredAt: '', source: 'system', payload: boards }
  const kinds = ['chat.removed', 'chat.message', 'obs.status', 'obs.state', 'presentation.queue', 'stream.status', 'bgm.player']
  const more = ['counter.values', 'counter.marathon', 'screen.break', 'screen.warnings', 'screen.appearance', 'chat.text_size']
  const payload = { ...Object.fromEntries([...kinds, ...more].map((k) => [k, []])), 'leaderboard.update': [update] }
  sockets[0]!.onmessage!({ data: JSON.stringify({ type: 'snapshot', version: 1, seq: 7, payload }) })
  await flushPromises()
  expect(wrapper.text()).toContain('莓莓')
  expect(wrapper.text()).not.toContain('小魚')
})

it('shows the chosen board and copies its overlay URL with the board and period', async () => {
  const wrapper = mount(LeaderboardPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  const pick = async (label: string, text: string) =>
    wrapper.findAll(`[aria-labelledby="${label}"] button`).find((b) => b.text() === text)!.trigger('click')
  await pick('lb-period', '全部')
  await pick('lb-board', '打卡')
  expect(wrapper.text()).toContain('小魚')
  expect(wrapper.text()).toContain('打卡頁的累計次數')
  await wrapper.findAll('button').find((b) => b.text() === '複製 OBS 網址')!.trigger('click')
  await flushPromises()
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(`${location.origin}/overlay/leaderboard/?board=checkins&period=all`)
})
