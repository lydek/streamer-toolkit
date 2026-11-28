// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { testRouter } from './testRouter'
import { i18n } from '../i18n'
import CheckinsPage from './CheckinsPage.vue'

const t = i18n.global.t

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
  it('shows the login only when it differs from the display name by more than case', async () => {
    const others = [
      { ...viewer, id: 'v2', user_login: 'akai', user_name: 'Akai' },
      { ...viewer, id: 'v3', user_login: 'akai_tw', user_name: '阿凱' },
    ]
    client.GET.mockResolvedValue(ok({ items: others, total: 2 }))
    const router = testRouter([{ path: '/checkins', component: CheckinsPage }])
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    expect(wrapper.findAll('.viewers .name').map((n) => n.text())).toEqual(['Akai', '阿凱 akai_tw'])
  })

  it('registers the day it is when clicked, even if the page opened before midnight', async () => {
    vi.setSystemTime(new Date(2026, 10, 17, 23, 59, 30))
    const router = testRouter([{ path: '/checkins', component: CheckinsPage }])
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
    const router = testRouter([{ path: '/checkins', component: CheckinsPage }])
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    await wrapper.findAll('button').find((b) => b.text() === t('checkin.push.show'))!.trigger('click')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/viewers/{id}/card', { params: { path: { id: 'v1' } } })
    await wrapper.findAll('button').find((b) => b.text() === t('checkin.push.hide'))!.trigger('click')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/card/hide')
  })

  it('deducts stamps with an optional reason and lists the change', async () => {
    // jsdom has no modal dialogs: open and close them by the attribute, firing close like a browser.
    HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
      this.setAttribute('open', '')
    }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
      this.removeAttribute('open')
      this.dispatchEvent(new Event('close'))
    }
    client.GET.mockImplementation(async (path: string) =>
      path === '/api/admin/checkin/deductions'
        ? ok([{ id: 'd1', viewer_id: 'v1', user_name: 'Ann', amount: 2, reason: '換一首歌', deducted_at: '2026-11-22T12:00:00.000Z' }])
        : ok({ items: [viewer], total: 1 }),
    )
    client.POST.mockResolvedValue(ok({ available: 1 }))
    const router = testRouter([{ path: '/checkins', component: CheckinsPage }])
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    await flushPromises()
    // The 可用印章 column.
    expect(wrapper.findAll('tbody td')[1].text()).toBe('3')
    expect(wrapper.text()).toContain(t('checkin.deduct.logLine', { name: 'Ann', n: 2 }))

    const open = async () => {
      await wrapper.findAll('button').find((b) => b.text() === t('checkin.deduct.open'))!.trigger('click')
      await flushPromises()
    }
    // Esc cancels: nothing is sent, and the next opening starts empty.
    await open()
    await wrapper.get('#deduct-reason').setValue('typed then cancelled')
    wrapper.get<HTMLDialogElement>('dialog.deduct').element.close()
    await flushPromises()
    await open()
    expect(wrapper.get<HTMLInputElement>('#deduct-reason').element.value).toBe('')
    expect(client.POST).not.toHaveBeenCalled()

    const amount = wrapper.get('#deduct-amount')
    await amount.setValue('2')
    await amount.trigger('blur')
    await wrapper.get('#deduct-reason').setValue('換一首歌')
    await wrapper.get('dialog.deduct form').trigger('submit')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/checkin/viewers/{id}/deductions', {
      params: { path: { id: 'v1' } },
      body: { amount: 2, reason: '換一首歌' },
    })
    expect(toastAdd.mock.calls.at(-1)?.[0].summary).toBe(t('checkin.deduct.done', { name: 'Ann', n: 2, left: 1 }))
  })

  it('draws the card from its slot setting: a dot a slot up to 20, a bar above, and the stamps on it', async () => {
    const rows = [
      { ...viewer, id: 'a', user_name: 'A', user_login: 'a', stamps: 1, card_slots: 1 },
      { ...viewer, id: 'b', user_name: 'B', user_login: 'b', stamps: 20, card_slots: 20, available: 0 },
      { ...viewer, id: 'c', user_name: 'C', user_login: 'c', stamps: 7, card_slots: 21 },
      { ...viewer, id: 'd', user_name: 'D', user_login: 'd', stamps: 50, card_slots: 100 },
    ]
    client.GET.mockImplementation((path: string) => Promise.resolve(ok(path.includes('deductions') ? [] : { items: rows, total: 4 })))
    const router = testRouter([{ path: '/checkins', component: CheckinsPage }])
    await router.push('/checkins')
    const wrapper = mount(CheckinsPage, { global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    const cards = wrapper.findAll('.card-progress')
    expect(cards.map((c) => c.text())).toEqual(['1/1', '20/20', '7/21', '50/100'])
    expect(cards.map((c) => [c.findAll('.dots i').length, c.findAll('.dots i.on').length, c.find('.bar').exists()])).toEqual([
      [1, 1, false],
      [20, 20, false],
      [0, 0, true],
      [0, 0, true],
    ])
    expect(cards[3].get('.bar i').attributes('style')).toContain('width: 50%')
    // Stamps come from the lifetime count, never from what is left to spend (B has none left, its card is full).
    expect(wrapper.findAll('tbody tr')[1].findAll('td')[1].text()).toBe('0')
  })
})
