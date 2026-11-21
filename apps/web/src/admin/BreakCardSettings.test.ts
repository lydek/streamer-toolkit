// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import type { BreakCard } from '../overlay/break/breakCard'
import BreakCardSettings from './BreakCardSettings.vue'

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(Date.parse('2026-11-22T12:00:00.000Z'))
})
enableAutoUnmount(afterEach)
afterEach(() => vi.useRealTimers())

it('takes the end time in 24-hour form', async () => {
  const put = vi.spyOn(client, 'PUT').mockResolvedValue({ data: undefined, response: new Response(null, { status: 204 }) } as never)
  const wrapper = mount(BreakCardSettings, { props: { card: { ...card(null), mode: 'until' } }, global: { plugins: [i18n, PrimeVue] } })
  const input = wrapper.get<HTMLInputElement>('#break-until')
  expect(input.element.value).toBe('20:00')
  await input.setValue('21:30')
  await input.trigger('blur')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/break', expect.objectContaining({ body: expect.objectContaining({ until_time: '21:30' }) }))
})

const card = (endsAt: string | null): BreakCard => ({
  shown: endsAt !== null,
  text: '休息一下',
  endText: '馬上回來',
  endsAt,
  mode: 'minutes',
  minutes: 10,
  untilTime: '20:00',
})

it('previews a card that was just shown at its full length, between clock ticks', async () => {
  const wrapper = mount(BreakCardSettings, { props: { card: card(null) }, global: { plugins: [i18n, PrimeVue] } })
  expect(wrapper.get('output').text()).toBe('10:00')
  // Shown 300 ms after the last tick: it must not read 10:01 first.
  vi.setSystemTime(Date.parse('2026-11-22T12:00:00.300Z'))
  await wrapper.setProps({ card: card('2026-11-22T12:10:00.300Z') })
  expect(wrapper.get('output').text()).toBe('10:00')
  await vi.advanceTimersByTimeAsync(1000)
  expect(wrapper.get('output').text()).toBe('09:59')
})
