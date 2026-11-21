// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import LeaderboardOverlay from './LeaderboardOverlay.vue'

let socket: { url: string; onmessage?: (e: { data: string }) => void }
beforeEach(() => {
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor(public url: string) {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    },
  )
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.unstubAllGlobals()
  history.replaceState(null, '', '/')
})

const update = (seq: number, name: string) => ({
  id: `l${seq}`,
  type: 'leaderboard.update',
  version: 1,
  seq,
  occurredAt: '',
  source: 'system',
  payload: {
    boards: [
      { board: 'bits', period: 'month', entries: [{ name, value: 1500 }] },
      { board: 'chat', period: 'stream', entries: [{ name: 'Other', value: 3 }] },
    ],
  },
})

it('shows the board and period in its URL, as text, and follows updates', async () => {
  history.replaceState(null, '', '/overlay/leaderboard/?board=bits&period=month')
  const wrapper = mount(LeaderboardOverlay, { global: { plugins: [i18n] } })
  expect(socket.url).toContain('/ws/overlay?kind=leaderboard')
  socket.onmessage!({ data: JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload: { 'leaderboard.update': [update(1, '<b>小魚</b>')] } }) })
  await flushPromises()
  expect(wrapper.get('.title').text()).toBe('小奇點')
  expect(wrapper.get('.name').text()).toBe('<b>小魚</b>')
  expect(wrapper.find('.name b').exists()).toBe(false)
  socket.onmessage!({ data: JSON.stringify(update(2, '莓莓')) })
  await flushPromises()
  expect(wrapper.get('.name').text()).toBe('莓莓')
})
