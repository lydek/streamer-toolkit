// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import CheckinsPage from './CheckinsPage.vue'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), POST: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
const viewer = { id: 'v1', user_login: 'ann', user_name: 'Ann', total: 3, stamps: 3, card_slots: 10, last_check_in: null }

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.stubGlobal('WebSocket', class { close() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  client.GET.mockResolvedValue(ok({ items: [viewer], total: 1 }))
  client.POST.mockResolvedValue(ok({ result: 'checked_in' }))
  toastAdd.mockReset()
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('CheckinsPage', () => {
  it('registers the day it is when clicked, even if the page opened before midnight', async () => {
    vi.setSystemTime(new Date(2026, 10, 17, 23, 59, 30))
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/checkins', component: CheckinsPage }] })
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    expect((wrapper.get('#cf-date').element as HTMLInputElement).value).toBe('2026-11-17')

    vi.setSystemTime(new Date(2026, 10, 18, 0, 0, 30))
    const button = wrapper.findAll('button').find((b) => b.text() === i18n.global.t('checkin.page.registerToday'))!
    await button.trigger('click')
    await flushPromises()

    expect(client.POST.mock.calls[0][1].body).toEqual({ date: null })
    expect(toastAdd.mock.calls[0][0].summary).toBe(
      i18n.global.t('checkin.page.registered', { name: 'Ann', date: '2026-11-18' }),
    )
    expect((wrapper.get('#cf-date').element as HTMLInputElement).value).toBe('2026-11-18')
  })
})
