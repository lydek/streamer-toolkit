// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import CounterOverlay from './CounterOverlay.vue'

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
afterEach(() => vi.unstubAllGlobals())

const values = (seq: number, counters: { id: string; name: string; value: number }[]) => ({
  id: `e${seq}`,
  type: 'counter.values',
  version: 1,
  seq,
  occurredAt: '',
  source: 'system',
  payload: { counters },
})

it('shows every counter from the snapshot, then each change, with names as text', async () => {
  const wrapper = mount(CounterOverlay, { attachTo: document.body })
  expect(socket.url).toContain('/ws/overlay?kind=counter')
  const send = async (m: unknown) => {
    socket.onmessage!({ data: JSON.stringify(m) })
    await flushPromises()
  }
  await send({ type: 'snapshot', version: 1, seq: 3, payload: { 'counter.values': [values(2, [{ id: 'a', name: '<b>死亡次數</b>', value: 12 }])] } })
  const rows = () => wrapper.findAll('[data-counter]').map((r) => `${r.get('.name').text()} ${r.get('.value').text()}`)
  expect(rows()).toEqual(['<b>死亡次數</b> 12'])
  expect(wrapper.find('b').exists()).toBe(false)

  await send(values(3, [{ id: 'a', name: '死亡次數', value: 99 }]))
  // Already covered by the snapshot.
  expect(rows()).toEqual(['<b>死亡次數</b> 12'])
  await send(values(4, [{ id: 'a', name: '死亡次數', value: 13 }, { id: 'b', name: '被抓包', value: 3 }]))
  expect(rows()).toEqual(['死亡次數 13', '被抓包 3'])
})
