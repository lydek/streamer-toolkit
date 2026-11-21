// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import BreakOverlay from './BreakOverlay.vue'

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
  id: `b${seq}`,
  type: 'screen.break',
  version: 1,
  seq,
  occurredAt: '2026-11-22T12:00:00.000Z',
  source: 'system',
  payload: {
    shown: true,
    text: '休息一下',
    endText: '馬上回來',
    endsAt: '2026-11-22T12:00:05.000Z',
    mode: 'minutes',
    minutes: 10,
    untilTime: '20:00',
    ...payload,
  },
})

it('counts down to the stored end, then shows the end text until hidden', async () => {
  const wrapper = mount(BreakOverlay)
  expect(socket.url).toContain('/ws/overlay?kind=break')
  await send({ type: 'snapshot', version: 1, seq: 1, payload: { 'screen.break': [event(1, {})] } })
  expect(wrapper.get('[data-break]').text()).toContain('休息一下')
  expect(wrapper.get('[data-break-countdown]').text()).toBe('00:05')

  await vi.advanceTimersByTimeAsync(5000)
  expect(wrapper.find('[data-break-countdown]').exists()).toBe(false)
  expect(wrapper.get('[data-break-end]').text()).toBe('馬上回來')

  await send(event(2, { shown: false, endsAt: null }))
  expect(wrapper.find('[data-break]').exists()).toBe(false)
})

it('starts a card that just arrived at its full length, between clock ticks', async () => {
  const wrapper = mount(BreakOverlay)
  await send({ type: 'snapshot', version: 1, seq: 1, payload: { 'screen.break': [] } })
  // 200 ms after the last tick, a 10-minute card arrives.
  vi.setSystemTime(Date.parse('2026-11-22T12:00:00.200Z'))
  await send(event(2, { endsAt: '2026-11-22T12:10:00.200Z' }))
  expect(wrapper.get('[data-break-countdown]').text()).toBe('10:00')
})

it('shows nothing while hidden and skips replays', async () => {
  const wrapper = mount(BreakOverlay)
  await send({ type: 'snapshot', version: 1, seq: 5, payload: { 'screen.break': [event(5, { shown: false })] } })
  expect(wrapper.find('[data-break]').exists()).toBe(false)
  await send(event(4, {}))
  expect(wrapper.find('[data-break]').exists()).toBe(false)
})

it('takes the theme and text size from 畫面工具 at once', async () => {
  mount(BreakOverlay)
  const appearance = (seq: number, theme: string, textSize: number) => ({
    id: `a${seq}`, type: 'screen.appearance', version: 1, seq, occurredAt: '', source: 'system', payload: { theme, textSize },
  })
  await send({ type: 'snapshot', version: 1, seq: 1, payload: { 'screen.break': [], 'screen.appearance': [appearance(1, 'contrast', 150)] } })
  const root = document.documentElement.style
  expect(root.getPropertyValue('--ov-bg')).toBe('#000')
  expect(root.getPropertyValue('--ov-scale')).toBe('1.5')
  await send(appearance(2, 'dark', 100))
  expect(root.getPropertyValue('--ov-bg')).toBe('')
  expect(root.getPropertyValue('--ov-scale')).toBe('')
})
