// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import PerformancePanel from './PerformancePanel.vue'

const t = i18n.global.t

enableAutoUnmount(afterEach)
afterEach(() => vi.restoreAllMocks())

const sample = (obs: object | null, warning = false) => ({
  cpu_percent: 12.5,
  memory_used_mb: 8000,
  memory_total_mb: 16000,
  app_cpu_percent: 1.2,
  app_memory_mb: 180,
  obs,
  warning,
})
const answer = (data: unknown) =>
  vi.spyOn(client, 'GET').mockResolvedValue({ data, response: new Response(null, { status: 200 }) } as never)

it('shows — for OBS while it is not connected and the rest as usual', async () => {
  answer(sample(null))
  const wrapper = mount(PerformancePanel, { global: { plugins: [i18n] } })
  await flushPromises()
  expect(wrapper.text()).toContain('12.5%')
  expect(wrapper.text()).toContain(t('performance.memoryOf', { used: 8000, total: 16000 }))
  expect(wrapper.text()).toContain(t('performance.dropped') + t('performance.droppedWindow'))
  expect(wrapper.findAll('dd').slice(3).map((d) => d.text())).toEqual(['—', '—', '—'])
  expect(wrapper.find('[role="alert"]').exists()).toBe(false)
})

it('warns when more than 5 % of frames dropped in the last minute', async () => {
  answer(sample({ cpu_percent: 4, dropped_percent: 6.25, bitrate_kbps: 6000 }, true))
  const wrapper = mount(PerformancePanel, { global: { plugins: [i18n] } })
  await flushPromises()
  expect(wrapper.get('[role="alert"]').text()).toBe(t('performance.warning', { n: '6.3' }))
  expect(wrapper.text()).toContain(t('performance.kbps', { n: 6000 }))
})
