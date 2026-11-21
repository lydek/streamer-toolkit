// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import MarathonOverlay from './MarathonOverlay.vue'

let socket: { url: string; onmessage?: (e: { data: string }) => void }
beforeEach(() => {
  vi.useFakeTimers()
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
const marathon = {
  id: 'm1',
  type: 'counter.marathon',
  version: 1,
  seq: 1,
  occurredAt: new Date().toISOString(),
  source: 'system',
  payload: { remainingMs: 60_000, running: false, paused: false, live: true, subMinutes: 5, bitsMinutes: 2, maxHours: 24 },
}
const test = (seq: number) => ({ id: `t${seq}`, type: 'marathon.test', version: 1, seq, occurredAt: '', source: 'simulation', payload: { remainingMs: 1_800_000 } })

it('shows a fixed test time for 3 s in place of the countdown, restarting on another test', async () => {
  const wrapper = mount(MarathonOverlay, { attachTo: document.body })
  expect(socket.url).toContain('/ws/overlay?kind=marathon')
  await send({ type: 'snapshot', version: 1, seq: 1, payload: { 'counter.marathon': [marathon] } })
  const real = wrapper.get('[data-marathon]').text()

  await send(test(2))
  const shown = () => wrapper.find('[data-marathon-test]')
  const fixed = shown().text()
  expect(fixed).not.toBe(real)
  expect(wrapper.find('[data-marathon]').exists()).toBe(false)

  await vi.advanceTimersByTimeAsync(2000)
  expect(shown().text()).toBe(fixed)
  await send(test(3))
  await vi.advanceTimersByTimeAsync(2000)
  expect(shown().exists()).toBe(true)
  await vi.advanceTimersByTimeAsync(1000)
  expect(shown().exists()).toBe(false)
  expect(wrapper.get('[data-marathon]').text()).toBe(real)
})
