// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { forgetSetup, needsSetup } from './setup'
import SetupPage from './SetupPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const failed = () => Promise.resolve({ error: { code: 'e2e.unmocked' }, response: new Response(null, { status: 404 }) })
let put: ReturnType<typeof vi.fn>

beforeEach(() => {
  forgetSetup()
  toastAdd.mockReset()
  put = vi.fn(() => ok())
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
})
enableAutoUnmount(afterEach)
afterEach(() => vi.restoreAllMocks())

describe('needsSetup', () => {
  it('follows the Host, asks once, and counts a failed request as done', async () => {
    const get = vi.spyOn(client, 'GET').mockImplementation((() => ok({ done: false })) as never)
    expect(await needsSetup()).toBe(true)
    expect(await needsSetup()).toBe(true)
    expect(get).toHaveBeenCalledTimes(1)
    forgetSetup()
    get.mockImplementation((() => ok({ done: true })) as never)
    expect(await needsSetup()).toBe(false)
    forgetSetup()
    get.mockImplementation(failed as never)
    expect(await needsSetup()).toBe(false)
  })
})

describe('首次設定', () => {
  let overview = { client_id: '', irc_connected: false, status: { state: 'signed_out' } }
  beforeEach(() => (overview = { client_id: '', irc_connected: false, status: { state: 'signed_out' } }))

  async function open() {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      path === '/api/admin/twitch' ? ok(overview) : path === '/api/admin/obs' ? ok({ host: '127.0.0.1', port: 4455 }) : ok()) as never)
    const router = testRouter([{ path: '/setup', component: SetupPage }])
    await router.push('/setup')
    const w = mount({ template: '<RouterView />' }, { global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    await flushPromises()
    return { w, router }
  }
  const button = (w: Awaited<ReturnType<typeof open>>['w'], label: string) => w.findAll('button').find((b) => b.text() === label)!

  it('walks the five steps, saves OBS on the way, and finishes on home for good', async () => {
    const { w, router } = await open()
    expect(w.get('h2').text()).toBe(t('setup.heading.register'))
    expect(button(w, t('setup.back')).attributes('disabled')).toBeDefined()
    for (let i = 0; i < 3; i++) await button(w, t('setup.next')).trigger('click')
    expect(w.get('h2').text()).toBe(t('setup.heading.obs'))
    await w.get('#setup-obs-port').setValue('4460')
    await button(w, t('setup.next')).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs', { body: { host: '127.0.0.1', port: 4460 } })
    expect(w.get('h2').text()).toBe(t('setup.heading.overlays'))
    await button(w, t('setup.finish')).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/first-run', { body: { done: true } })
    expect(router.currentRoute.value.path).toBe('/')
    expect(await needsSetup()).toBe(false)
  })

  it('keeps the OBS step on a port out of range', async () => {
    const { w } = await open()
    for (let i = 0; i < 3; i++) await button(w, t('setup.next')).trigger('click')
    await w.get('#setup-obs-port').setValue('70000')
    await button(w, t('setup.next')).trigger('click')
    await flushPromises()
    expect(put).not.toHaveBeenCalled()
    expect(w.get('h2').text()).toBe(t('setup.heading.obs'))
  })

  it('skips the whole setup from any step', async () => {
    const { w, router } = await open()
    await button(w, t('setup.skip')).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/first-run', { body: { done: true } })
    expect(router.currentRoute.value.path).toBe('/')
  })

  it('shows the Client ID form, moves on to signing in once it is saved, and asks for it first when missing', async () => {
    const { w } = await open()
    await button(w, t('setup.next')).trigger('click')
    await button(w, t('setup.next')).trigger('click')
    // No Client ID yet: the sign-in step sends back.
    expect(w.get('h2').text()).toBe(t('setup.heading.signIn'))
    expect(w.text()).toContain(t('setup.clientIdFirst'))
    await button(w, t('setup.back')).trigger('click')
    put.mockImplementation((path: string) => {
      if (path === '/api/admin/twitch/client-id') overview = { ...overview, client_id: 'abc' }
      return ok()
    })
    await w.get('#client-id').setValue('abc')
    await w.get('form').trigger('submit')
    await flushPromises()
    expect(w.get('h2').text()).toBe(t('setup.heading.signIn'))
  })

  it('says so when copying fails', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: () => Promise.reject(new Error('busy')) } })
    const { w } = await open()
    await w.get(`[aria-label="${t('setup.copy.redirect')}"]`).trigger('click')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('chat.copyFailed') }))
    vi.unstubAllGlobals()
  })
})
