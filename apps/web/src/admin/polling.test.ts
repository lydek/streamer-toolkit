// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Component } from 'vue'
import { i18n } from '../i18n'
import CheckinOutbox from './CheckinOutbox.vue'
import HomeView from './HomeView.vue'
import SettingsView from './SettingsView.vue'
import { testRouter } from './testRouter'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), PUT: vi.fn(), POST: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('WebSocket', class { close() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  client.GET.mockReset()
})

// Pages that poll every 2 s once loaded. Leaving one before its first load answers must stop the polling too.
describe.each<[string, Component, Record<string, unknown>]>([
  ['HomeView', HomeView, {}],
  ['SettingsView', SettingsView, {}],
  ['CheckinOutbox', CheckinOutbox, { state: undefined }],
])('%s', (_, page, props) => {
  it('stops polling when left before the first load answers', async () => {
    let answer!: () => void
    const firstLoad = new Promise<void>((r) => (answer = r))
    client.GET.mockImplementation(async () => {
      await firstLoad
      throw new Error('offline')
    })
    const router = testRouter([{ path: '/', component: page }])
    await router.push('/')
    const wrapper = mount(page, { props, global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    wrapper.unmount()
    answer()
    await flushPromises()

    const calls = client.GET.mock.calls.length
    await vi.advanceTimersByTimeAsync(10_000)
    expect(client.GET.mock.calls.length).toBe(calls)
  })
})
