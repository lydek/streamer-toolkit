// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import SettingsView from './SettingsView.vue'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), PUT: vi.fn(), POST: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = (data?: unknown) => ({ data, response: new Response(null, { status: data === undefined ? 204 : 200 }) })
const GETS: Record<string, unknown> = {
  '/api/admin/general': { ask_before_close: true },
  '/api/admin/obs': { host: '127.0.0.1', port: 4455, password_set: false, status: { state: 'error', reason: null } },
  '/api/admin/twitch': { client_id: '', status: { state: 'signed_out' } },
  '/api/admin/modules': [{ id: 'chat', enabled: true }],
}

async function mountSettings() {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/settings', component: SettingsView }] })
  await router.push('/settings')
  const wrapper = mount(SettingsView, { global: { plugins: [router, i18n, PrimeVue, ToastService] }, attachTo: document.body })
  await vi.waitFor(() => expect(wrapper.find('#obs-host').exists()).toBe(true))
  return wrapper
}

beforeEach(() => {
  client.GET.mockImplementation(async (path: string) => ok(GETS[path]))
  client.PUT.mockReset()
})
afterEach(() => {
  document.body.innerHTML = ''
})

describe('SettingsView', () => {
  it('saves OBS fields committed back to back in order, each with the other one already saved', async () => {
    let release!: () => void
    client.PUT.mockImplementationOnce(() => new Promise((r) => (release = () => r(ok())))).mockResolvedValue(ok())
    const wrapper = await mountSettings()

    await wrapper.get('#obs-host').setValue('obs-pc')
    await wrapper.get('#obs-host').trigger('blur')
    await wrapper.get('#obs-port').setValue('4456')
    await wrapper.get('#obs-port').trigger('blur')
    await flushPromises()
    expect(client.PUT).toHaveBeenCalledTimes(1) // the port waits for the host
    release()

    await vi.waitFor(() => expect(client.PUT).toHaveBeenCalledTimes(2))
    expect(client.PUT.mock.calls.map(([, init]) => init.body)).toEqual([
      { host: 'obs-pc', port: 4455 },
      { host: 'obs-pc', port: 4456 },
    ])
    wrapper.unmount()
  })

  it('sends a typed password once, then empties the field', async () => {
    client.PUT.mockResolvedValue(ok())
    const wrapper = await mountSettings()
    const field = wrapper.get<HTMLInputElement>('#obs-password')

    await field.trigger('blur')
    expect(client.PUT).not.toHaveBeenCalled() // untouched: nothing to send

    await field.setValue('hunter2')
    await field.trigger('keydown', { key: 'Enter' })
    await vi.waitFor(() => expect(field.element.value).toBe(''))
    expect(client.PUT).toHaveBeenCalledWith('/api/admin/obs', {
      body: { host: '127.0.0.1', port: 4455, password: 'hunter2' },
    })
    expect(wrapper.get('#obs-password-state').text()).toBe(i18n.global.t('settings.obs.passwordSet'))

    await field.trigger('blur')
    expect(client.PUT).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('turns a module off from Features', async () => {
    client.PUT.mockResolvedValue(ok())
    const wrapper = await mountSettings()
    await wrapper.get('#module-chat').trigger('change')
    await vi.waitFor(() =>
      expect(client.PUT).toHaveBeenCalledWith('/api/admin/modules/{id}', {
        params: { path: { id: 'chat' } },
        body: { enabled: false },
      }),
    )
    wrapper.unmount()
  })
})
