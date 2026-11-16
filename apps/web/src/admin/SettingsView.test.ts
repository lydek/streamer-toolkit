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
  '/api/admin/modules': [{ id: 'chat', enabled: true, closed: false }],
  '/api/admin/checkin/settings': { command: '打卡', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: null },
  '/api/admin/checkin/rewards': [{ id: 'w-1', title: '打卡' }],
  '/api/admin/checkin/outbox': { pending: 0, oldest_pending_at: null, dead_letters: [] },
}

async function mountSettings() {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/settings', component: SettingsView }] })
  await router.push('/settings')
  const wrapper = mount(SettingsView, { global: { plugins: [router, i18n, PrimeVue, ToastService] }, attachTo: document.body })
  await vi.waitFor(() => expect(wrapper.find('#obs-host').exists()).toBe(true))
  return wrapper
}

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  client.GET.mockImplementation(async (path: string) => ok(GETS[path]))
  client.PUT.mockReset()
})
afterEach(() => {
  document.body.innerHTML = ''
  GETS['/api/admin/general'] = { ask_before_close: true }
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

  it('keeps the module switches usable when the OBS settings fail to load', async () => {
    client.GET.mockImplementation(async (path: string) =>
      path === '/api/admin/obs'
        ? { error: { code: 'obs.secret_store_failed' }, response: new Response(null, { status: 500 }) }
        : ok(GETS[path]),
    )
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/settings', component: SettingsView }] })
    await router.push('/settings')
    const wrapper = mount(SettingsView, { global: { plugins: [router, i18n, PrimeVue, ToastService] }, attachTo: document.body })
    await vi.waitFor(() => expect(wrapper.find('#module-chat').exists()).toBe(true))
    expect(wrapper.find('#ask-before-close').exists()).toBe(true)
    expect(wrapper.find('#obs-host').exists()).toBe(false)
    wrapper.unmount()
  })

  it('reads "Ask before closing" again when the window gets focus back', async () => {
    const wrapper = await mountSettings()
    expect(wrapper.get<HTMLInputElement>('#ask-before-close').element.checked).toBe(true)
    GETS['/api/admin/general'] = { ask_before_close: false } // the close hint's "Don't show again"
    window.dispatchEvent(new Event('focus'))
    await vi.waitFor(() => expect(wrapper.get<HTMLInputElement>('#ask-before-close').element.checked).toBe(false))
    expect(client.PUT).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('keeps a toggle saved while that read was in flight', async () => {
    client.PUT.mockResolvedValue(ok())
    const wrapper = await mountSettings()
    let answer!: () => void
    client.GET.mockImplementationOnce(() => new Promise((r) => (answer = () => r(ok({ ask_before_close: true })))))
    window.dispatchEvent(new Event('focus')) // the read starts, returning the old value later
    await wrapper.get('#ask-before-close').trigger('change')
    await vi.waitFor(() => expect(client.PUT).toHaveBeenCalledTimes(1))
    await flushPromises()
    answer()
    await flushPromises()
    expect(wrapper.get<HTMLInputElement>('#ask-before-close').element.checked).toBe(false)
    wrapper.unmount()
  })

  it('turns a module off from Features', async () => {
    client.PUT.mockResolvedValue(ok())
    const wrapper = await mountSettings()
    await wrapper.get('#module-chat').trigger('change')
    await vi.waitFor(() =>
      expect(client.PUT).toHaveBeenCalledWith('/api/admin/modules/{id}', {
        params: { path: { id: 'chat' } },
        body: { enabled: false, close: false },
      }),
    )
    wrapper.unmount()
  })

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
      { command: 'sign', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: null },
      { command: 'sign', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 12, reward_id: null },
    ])
    // The "!" is shown in front of the field, so a typed one is dropped.
    expect(wrapper.get<HTMLInputElement>('#checkin-command').element.value).toBe('sign')

    await wrapper.get('#checkin-slots').setValue('0')
    await wrapper.get('#checkin-slots').trigger('blur')
    await vi.waitFor(() => expect(wrapper.get<HTMLInputElement>('#checkin-slots').element.value).toBe('12'))
    expect(client.PUT).toHaveBeenCalledTimes(2) // refused before it reaches the Host
    wrapper.unmount()
  })

  describe('check-in switch and outbox', () => {
    beforeEach(() => {
      HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
        this.open = true
      }
      HTMLDialogElement.prototype.close = function (this: HTMLDialogElement, value?: string) {
        if (value !== undefined) this.returnValue = value
        this.open = false
        this.dispatchEvent(new Event('close'))
      }
      GETS['/api/admin/modules'] = [{ id: 'checkin', enabled: true, closed: false }]
      GETS['/api/admin/checkin/outbox'] = { pending: 0, oldest_pending_at: null, dead_letters: [] }
      client.POST.mockReset()
    })
    afterEach(() => {
      GETS['/api/admin/modules'] = [{ id: 'chat', enabled: true, closed: false }]
    })

    it('asks whether to pause or close before turning check-ins off', async () => {
      client.PUT.mockResolvedValue(ok())
      const wrapper = await mountSettings()
      await vi.waitFor(() => expect(wrapper.find('#module-checkin').exists()).toBe(true))
      const dialog = wrapper.get('dialog').element as HTMLDialogElement

      await wrapper.get('#module-checkin').trigger('change')
      expect(dialog.open).toBe(true)
      expect(client.PUT).not.toHaveBeenCalled()
      dialog.close() // Esc or Cancel
      await flushPromises()
      expect(client.PUT).not.toHaveBeenCalled()

      await wrapper.get('#module-checkin').trigger('change')
      expect(wrapper.get('#checkin-close-warning').text()).toBe(i18n.global.t('checkin.disable.closeWarning'))
      await wrapper.get('dialog').findAll('button')[1].trigger('click')
      await vi.waitFor(() =>
        expect(client.PUT).toHaveBeenCalledWith('/api/admin/modules/{id}', {
          params: { path: { id: 'checkin' } },
          body: { enabled: false, close: true },
        }),
      )
      wrapper.unmount()
    })

    it('shows the backlog while paused and offers to close, warning about what gets skipped', async () => {
      GETS['/api/admin/modules'] = [{ id: 'checkin', enabled: false, closed: false }]
      GETS['/api/admin/checkin/outbox'] = { pending: 3, oldest_pending_at: '2026-11-16T15:00:00.000Z', dead_letters: [] }
      client.PUT.mockResolvedValue(ok())
      const wrapper = await mountSettings()
      await vi.waitFor(() => expect(wrapper.text()).toContain(i18n.global.t('checkin.outbox.paused', { n: 3 })))
      expect(wrapper.find('time[datetime="2026-11-16T15:00:00.000Z"]').exists()).toBe(true)
      const toClose = wrapper.findAll('button').find((b) => b.text() === i18n.global.t('checkin.outbox.switchToClose'))!
      await toClose.trigger('click')
      expect(wrapper.get('#checkin-close-warning').text()).toContain(
        i18n.global.t('checkin.disable.pendingSkipped', { n: 3 }),
      )
      wrapper.unmount()
    })

    it('retries or skips a dead letter, showing viewer text as text', async () => {
      GETS['/api/admin/checkin/outbox'] = {
        pending: 0,
        oldest_pending_at: null,
        dead_letters: [
          {
            event_id: 'e1',
            occurred_at: '2026-11-16T15:00:00.000Z',
            failed_at: '2026-11-16T15:00:20.000Z',
            attempts: 5,
            error: 'checkin.storage_failed: disk full',
            user_name: '<b>Viewer</b>',
            reward_title: '打卡',
          },
        ],
      }
      client.POST.mockResolvedValue(ok())
      const wrapper = await mountSettings()
      await vi.waitFor(() => expect(wrapper.find('#checkin-dead-letters').exists()).toBe(true))
      expect(wrapper.html()).toContain('&lt;b&gt;Viewer&lt;/b&gt;')
      expect(wrapper.find('b').exists()).toBe(false)
      const button = (label: string) => wrapper.findAll('button').find((b) => b.text() === i18n.global.t(label))!
      await button('checkin.outbox.retry').trigger('click')
      await vi.waitFor(() => expect(button('checkin.outbox.skip').attributes('disabled')).toBeUndefined())
      await button('checkin.outbox.skip').trigger('click')
      await vi.waitFor(() => expect(client.POST).toHaveBeenCalledTimes(2))
      expect(client.POST.mock.calls.map(([path]) => path)).toEqual([
        '/api/admin/checkin/outbox/{event_id}/retry',
        '/api/admin/checkin/outbox/{event_id}/skip',
      ])
      expect(client.POST.mock.calls[0][1]).toEqual({ params: { path: { event_id: 'e1' } } })
      wrapper.unmount()
    })
  })
})
