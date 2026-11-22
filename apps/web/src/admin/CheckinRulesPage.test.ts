// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import CheckinRulesPage from './CheckinRulesPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), PUT: vi.fn(), POST: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = (data?: unknown) => ({ data, response: new Response(null, { status: data === undefined ? 204 : 200 }) })
const GETS: Record<string, unknown> = {
  '/api/admin/checkin/settings': { command: '打卡', allowed: 'everyone', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: null },
  '/api/admin/checkin/rewards': [{ id: 'w-1', title: '打卡' }],
  '/api/admin/sounds': { sounds: [], slots: {} },
}

// 打卡規則: the check-in settings moved here from Settings › features.
async function mountSettings() {
  const router = testRouter([{ path: '/checkin-rules', component: CheckinRulesPage }])
  await router.push('/checkin-rules')
  const wrapper = mount(CheckinRulesPage, { global: { plugins: [router, i18n, PrimeVue, ToastService] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  client.GET.mockImplementation(async (path: string) => ok(GETS[path]))
  client.PUT.mockReset()
})
afterEach(() => {
  document.body.innerHTML = ''
})

describe('CheckinRulesPage', () => {
  it('says why the channel rewards cannot be listed and keeps the saved one by its ID', async () => {
    client.GET.mockImplementation(async (path: string) =>
      path === '/api/admin/checkin/rewards'
        ? { error: { code: 'twitch.request_refused' }, response: new Response(null, { status: 502 }) }
        : path === '/api/admin/checkin/settings'
          ? ok({ command: '打卡', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: 'w-9' })
          : ok(GETS[path]),
    )
    const wrapper = await mountSettings()
    await vi.waitFor(() => expect(wrapper.find('#checkin-reward-error').exists()).toBe(true))
    expect(wrapper.get('#checkin-reward-error').text()).toBe(i18n.global.t('errors.twitch.request_refused'))
    expect(wrapper.get('#checkin-reward').attributes('aria-describedby')).toBe('checkin-reward-error')
    expect(wrapper.get('#checkin-reward').text()).toBe('w-9')
    wrapper.unmount()
  })

  it('saves check-in fields committed back to back with the other values already saved', async () => {
    client.PUT.mockImplementation(async (_: string, init: { body: unknown }) => ok(init.body))
    const wrapper = await mountSettings()
    await vi.waitFor(() => expect(wrapper.find('#checkin-command').exists()).toBe(true))

    await wrapper.get('#checkin-command').setValue('!sign')
    await wrapper.get('#checkin-command').trigger('blur')
    await wrapper.get('#checkin-slots').setValue('12')
    await wrapper.get('#checkin-slots').trigger('keydown', { key: 'Enter' })
    await vi.waitFor(() => expect(client.PUT).toHaveBeenCalledTimes(2))
    expect(client.PUT.mock.calls.map(([, init]) => init.body)).toEqual([
      { command: 'sign', allowed: 'everyone', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: null },
      { command: 'sign', allowed: 'everyone', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 12, reward_id: null },
    ])
    // The "!" is shown in front of the field, so a typed one is dropped.
    expect(wrapper.get<HTMLInputElement>('#checkin-command').element.value).toBe('sign')

    await wrapper.get('#checkin-slots').setValue('0')
    await wrapper.get('#checkin-slots').trigger('blur')
    await vi.waitFor(() => expect(wrapper.get<HTMLInputElement>('#checkin-slots').element.value).toBe('12'))
    expect(client.PUT).toHaveBeenCalledTimes(2) // refused before it reaches the Host
    wrapper.unmount()
  })

  it('chooses who may check in, and says which feature already has a command name', async () => {
    client.PUT.mockImplementation(async (_: string, init: { body: { command: string } }) =>
      init.body.command === 'deaths'
        ? { error: { code: 'commands.taken', module: 'counter', label: '死亡次數' }, response: new Response(null, { status: 409 }) }
        : ok(init.body),
    )
    const wrapper = await mountSettings()
    await vi.waitFor(() => expect(wrapper.find('#checkin-command').exists()).toBe(true))
    const select = wrapper.findAllComponents({ name: 'Select' }).find((s) => s.props('inputId') === 'checkin-allowed')!
    select.vm.$emit('update:modelValue', 'vip')
    await vi.waitFor(() => expect(client.PUT).toHaveBeenCalledTimes(1))
    expect(client.PUT.mock.calls[0][1].body).toMatchObject({ command: '打卡', allowed: 'vip' })

    await wrapper.get('#checkin-command').setValue('deaths')
    await wrapper.get('#checkin-command').trigger('blur')
    await vi.waitFor(() =>
      expect(wrapper.text()).toContain(t('errors.commands.taken', { feature: t('modules.names.counter'), label: '死亡次數' })),
    )
    expect(wrapper.get<HTMLInputElement>('#checkin-command').element.value).toBe('打卡')
    wrapper.unmount()
  })
})
