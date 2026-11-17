// @vitest-environment jsdom
import { invoke } from '@tauri-apps/api/core'
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import UpdateNotice from './UpdateNotice.vue'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), PUT: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = (data?: unknown) => ({ data, response: new Response(null, { status: data === undefined ? 204 : 200 }) })
const t = i18n.global.t

async function mountNotice() {
  const wrapper = mount(UpdateNotice, { global: { plugins: [i18n, PrimeVue] } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.mocked(invoke).mockReset()
  client.PUT.mockReset()
  toastAdd.mockReset()
})

describe('UpdateNotice', () => {
  it('shows the running and the new version, and opens the download page without passing any URL', async () => {
    client.GET.mockResolvedValue(ok({ current: '0.1.0', available: '0.2.0', check: true }))
    vi.mocked(invoke).mockResolvedValue(undefined)
    const wrapper = await mountNotice()

    expect(wrapper.text()).toContain(t('update.available', { version: '0.2.0' }))
    expect(wrapper.text()).toContain(t('update.current', { version: '0.1.0' }))
    await wrapper.findAll('button')[0].trigger('click')
    expect(invoke).toHaveBeenCalledWith('open_download_page')
  })

  it('"not this one" sends that version and hides the notice', async () => {
    client.GET.mockResolvedValue(ok({ current: '0.1.0', available: '0.2.0', check: true }))
    client.PUT.mockResolvedValue(ok())
    const wrapper = await mountNotice()

    await wrapper.findAll('button')[1].trigger('click')
    await flushPromises()
    expect(client.PUT).toHaveBeenCalledWith('/api/admin/update/dismiss', { body: { version: '0.2.0' } })
    expect(wrapper.find('section').exists()).toBe(false)
    // Home moves focus on: the focused button went with the notice.
    expect(wrapper.emitted('dismissed')).toHaveLength(1)
  })

  it('shows nothing, and no error, without a newer release or when the Host cannot answer', async () => {
    client.GET.mockResolvedValue(ok({ current: '0.1.0', available: null, check: true }))
    expect((await mountNotice()).find('section').exists()).toBe(false)

    client.GET.mockResolvedValue({ error: { code: 'x' }, response: new Response(null, { status: 500 }) })
    expect((await mountNotice()).find('section').exists()).toBe(false)
    expect(toastAdd).not.toHaveBeenCalled()
  })

  it('reports a browser that cannot be opened', async () => {
    client.GET.mockResolvedValue(ok({ current: '0.1.0', available: '0.2.0', check: true }))
    vi.mocked(invoke).mockRejectedValue('admin.open_failed')
    const wrapper = await mountNotice()

    await wrapper.findAll('button')[0].trigger('click')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('errors.admin.open_failed') }))
  })
})
