// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import AlertEventsPage from './AlertEventsPage.vue'
import { client } from './api'
import { testRouter } from './testRouter'

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))
const t = i18n.global.t

describe('AlertEventsPage', () => {
  it('tests one alert kind through the simulator, with its amount', async () => {
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
    vi.spyOn(client, 'GET').mockResolvedValue({
      data: { sounds: [], slots: {} },
      response: new Response(null, { status: 200 }),
    } as never)
    const post = vi.spyOn(client, 'POST').mockResolvedValue({ data: undefined, response: new Response(null, { status: 204 }) } as never)
    const wrapper = mount(AlertEventsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] } })
    await flushPromises()
    await wrapper.get(`button[aria-label="${t('alerts.testKind', { kind: t('alerts.kinds.cheer') })}"]`).trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/simulate/alert', { body: { kind: 'cheer', amount: 100 } })
  })
})
