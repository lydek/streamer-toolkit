// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import LeaderboardPage from './LeaderboardPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

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
  await wrapper.findAll('[aria-labelledby="lb-period"] button').find((b) => b.text() === t('leaderboard.periods.all'))!.trigger('click')
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

it('shows every board side by side for the chosen period, and copies one with its board and period', async () => {
  const wrapper = mount(LeaderboardPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  expect(wrapper.findAll('section.board')).toHaveLength(4)
  await wrapper.findAll('[aria-labelledby="lb-period"] button').find((b) => b.text() === t('leaderboard.periods.all'))!.trigger('click')
  const checkins = wrapper.get('section[aria-labelledby="lb-checkins"]')
  expect(checkins.text()).toContain('小魚')
  expect(checkins.text()).toContain(t('leaderboard.allCheckins'))
  await checkins.get('button').trigger('click')
  await flushPromises()
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(`${location.origin}/overlay/leaderboard/?board=checkins&period=all`)
})

it('says which periods of each board are in OBS', async () => {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    Promise.resolve({
      data:
        path === '/api/admin/obs/sources'
          ? { connected: true, local: true, live_scene: 'Game', scenes: ['Game'], sources: [{ name: 'Bits', overlay: '/overlay/leaderboard/?board=bits&period=stream', scenes: ['Game'] }] }
          : { boards: [] },
      response: new Response(null, { status: 200 }),
    })) as never)
  const wrapper = mount(LeaderboardPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  expect(wrapper.get('section[aria-labelledby="lb-bits"] .in-obs').text()).toBe(t('leaderboard.inObs', { periods: t('leaderboard.periods.stream') }))
  expect(wrapper.get('section[aria-labelledby="lb-chat"] .in-obs').text()).toBe(t('obsStatus.missing'))
})
