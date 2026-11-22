// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { testRouter } from './testRouter'
import TroubleshootPanel from './TroubleshootPanel.vue'

const t = i18n.global.t

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn(async () => {}) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const ok = (data: unknown) => Promise.resolve({ data, response: new Response(null, { status: 200 }) })

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(
      path === '/api/admin/performance'
        ? null
        : path === '/api/admin/diagnostics/connections'
        ? { twitch_signed_in: true, twitch_latency_ms: 87, eventsub_connected: true, eventsub_subscriptions: 9, obs_connected: false }
        : [{ at: '2026-11-22T09:00:00Z', level: 'WARN', code: 'obs.unreachable', message: 'OBS is not running' }],
    )) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('shows the connection checks and the log, and opens only the logs folder', async () => {
  const wrapper = mount(TroubleshootPanel, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  expect(wrapper.text()).toContain(t('troubleshoot.latency', { ms: 87 }))
  expect(wrapper.text()).toContain(t('troubleshoot.subscriptions', { n: 9 }))
  expect(wrapper.text()).toContain('obs.unreachable')
  // Only the failing row points to its settings.
  const fixes = wrapper.findAll('a.fix')
  expect(fixes.map((a) => a.attributes('href'))).toEqual(['/settings/connections#obs'])
  expect(fixes[0].text()).toBe(t('troubleshoot.fix'))
  expect(client.GET).toHaveBeenCalledWith('/api/admin/diagnostics/logs', { params: { query: { level: 'warn' } } })
  await wrapper.findAll('button').find((b) => b.text() === t('troubleshoot.openFolder'))!.trigger('click')
  expect(invoke).toHaveBeenCalledWith('open_logs_folder')
})
