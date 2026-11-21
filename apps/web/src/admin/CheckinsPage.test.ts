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
const viewer = { id: 'v1', user_login: 'ann', user_name: 'Ann', total: 3, available: 3, stamps: 3, card_slots: 10, last_check_in: null }

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

  it('pushes a viewer’s card to the screen and hides the card on screen', async () => {
    client.POST.mockResolvedValue({ data: undefined, response: new Response(null, { status: 204 }) })
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/checkins', component: CheckinsPage }] })
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    await wrapper.findAll('button').find((b) => b.text() === '把集點卡推到畫面')!.trigger('click')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/viewers/{id}/card', { params: { path: { id: 'v1' } } })
    await wrapper.findAll('button').find((b) => b.text() === '隱藏畫面卡片')!.trigger('click')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/card/hide')
  })

  it('deducts stamps with an optional reason and lists the change', async () => {
    client.GET.mockImplementation(async (path: string) =>
      path === '/api/admin/checkin/deductions'
        ? ok([{ id: 'd1', viewer_id: 'v1', user_name: 'Ann', amount: 2, reason: '換一首歌', deducted_at: '2026-11-22T12:00:00.000Z' }])
        : ok({ items: [viewer], total: 1 }),
    )
    client.POST.mockResolvedValue(ok({ available: 1 }))
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/checkins', component: CheckinsPage }] })
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    expect(wrapper.text()).toContain('可用印章 3')
    expect(wrapper.text()).toContain('Ann 扣 2 格')

    await wrapper.findAll('button').find((b) => b.text() === '扣印章')!.trigger('click')
    const amount = wrapper.get('#deduct-amount-v1')
    await amount.setValue('2')
    await amount.trigger('blur')
    await wrapper.get('#deduct-reason-v1').setValue('換一首歌')
    await wrapper.get('form.deduct').trigger('submit')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/viewers/{id}/deductions', {
      params: { path: { id: 'v1' } },
      body: { amount: 2, reason: '換一首歌' },
    })
    expect(toastAdd.mock.calls.at(-1)?.[0].summary).toBe('已扣 Ann 2 格，還有 1 格可用。')
  })
})
