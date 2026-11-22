// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import WarningsOverlay from './WarningsOverlay.vue'

const t = i18n.global.t

let socket: { url: string; onmessage?: (e: { data: string }) => void }
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(Date.parse('2026-11-22T12:00:00.000Z'))
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
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

const send = async (m: unknown) => {
  socket.onmessage!({ data: JSON.stringify(m) })
  await flushPromises()
}
const event = (seq: number, payload: object) => ({
  id: `w${seq}`,
  type: 'screen.warnings',
  version: 1,
  seq,
  occurredAt: '2026-11-22T12:00:00.000Z',
  source: 'system',
  payload: {
    chatOverlay: true,
    twitch: true,
    mic: true,
    micSeconds: 2,
    micInput: 'Mic',
    chatOverlayDown: false,
    twitchDown: false,
    micMutedSince: null,
    micMissing: false,
    ...payload,
  },
})
const shown = (wrapper: ReturnType<typeof mount>) => wrapper.findAll('[data-warning]').map((w) => w.attributes('data-warning'))

it('shows each warning that is down, the mic once it has been muted long enough, and clears them', async () => {
  const wrapper = mount(WarningsOverlay, { global: { plugins: [i18n] } })
  expect(socket.url).toContain('/ws/overlay?kind=warnings')
  await send({ type: 'snapshot', version: 1, seq: 1, payload: { 'screen.warnings': [event(1, { chatOverlayDown: true })] } })
  expect(shown(wrapper)).toEqual(['chatOverlay'])
  expect(wrapper.text()).toContain(t('overlay.warnings.chatOverlay'))

  await send(event(2, { twitchDown: true, micMutedSince: '2026-11-22T12:00:00.000Z' }))
  expect(shown(wrapper)).toEqual(['twitch'])
  await vi.advanceTimersByTimeAsync(2000)
  expect(shown(wrapper)).toEqual(['mic', 'twitch'])

  await send(event(3, {}))
  expect(wrapper.find('[data-warnings]').exists()).toBe(false)
})
