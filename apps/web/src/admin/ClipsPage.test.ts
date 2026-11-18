// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import type { Schemas } from './api'
import ClipsPage from './ClipsPage.vue'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))
const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), POST: vi.fn(), PUT: vi.fn(), DELETE: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = (data?: unknown, status = 200) => ({ data, response: new Response(null, { status }) })
const fail = (code: string, status: number) => ({ error: { code }, response: new Response(null, { status }) })
const clip = (n: number) => ({ id: `Clip${n}`, title: `<b>Clip ${n}</b>`, broadcasterName: 'Twitch', durationMs: 30_000 })
const queue = (over: Partial<Schemas['ClipQueue']> = {}): Schemas['ClipQueue'] => ({
  queue: [{ entry: 'e2', ...clip(2) }],
  state: 'estimated_playing',
  sessionId: 's1',
  current: clip(1),
  elapsedMs: 12_400,
  ...over,
})
const t = i18n.global.t

let current: Schemas['ClipQueue']

beforeEach(() => {
  vi.useFakeTimers()
  current = queue()
  client.GET.mockImplementation(async (path: string) =>
    path === '/api/admin/clips/settings' ? ok({ bufferSecs: 2 }) : ok(current),
  )
  client.POST.mockResolvedValue(ok(undefined, 204))
  client.PUT.mockResolvedValue(ok(undefined, 204))
  client.DELETE.mockResolvedValue(ok(undefined, 204))
  toastAdd.mockReset()
})
afterEach(() => {
  vi.useRealTimers()
  vi.clearAllMocks()
})

async function page() {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/clips', component: ClipsPage }] })
  await router.push('/clips')
  const wrapper = mount(ClipsPage, { global: { plugins: [router, i18n, PrimeVue] } })
  await flushPromises()
  return wrapper
}

const button = (wrapper: Awaited<ReturnType<typeof page>>, label: string) =>
  wrapper.findAll('button').find((b) => b.text() === label)!

describe('ClipsPage', () => {
  it('shows the estimated state, the remaining time and the queue as text', async () => {
    const wrapper = await page()
    const state = wrapper.get('[data-clip-state]').text()
    expect(state).toContain(t('clips.state.estimated_playing'))
    expect(state).toContain('<b>Clip 1</b>')
    expect(wrapper.find('b').exists()).toBe(false)
    expect(wrapper.text()).toContain(t('clips.remaining', { s: 18 }))
    expect(wrapper.text()).toContain(t('clips.queue', { n: 1 }))
    expect(wrapper.text()).toContain('<b>Clip 2</b>')
    expect(wrapper.text()).not.toContain('成功')
  })

  it('adds a clip and shows refusals in words', async () => {
    const wrapper = await page()
    client.POST.mockResolvedValueOnce(ok({ entry: 'e3', ...clip(3) }))
    await wrapper.get('#clip-input').setValue(' https://clips.twitch.tv/Clip3 ')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(client.POST).toHaveBeenCalledWith('/api/admin/clips', { body: { input: ' https://clips.twitch.tv/Clip3 ' } })
    expect(toastAdd.mock.calls[0][0].summary).toBe(t('clips.added', { title: '<b>Clip 3</b>' }))
    expect((wrapper.get('#clip-input').element as HTMLInputElement).value).toBe('')

    client.POST.mockResolvedValueOnce(fail('clip.not_found', 404))
    await wrapper.get('#clip-input').setValue('GoneClip')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toBe(t('errors.clip.not_found'))
  })

  it('names the session for skip and replay', async () => {
    const wrapper = await page()
    await button(wrapper, t('clips.skip')).trigger('click')
    await button(wrapper, t('clips.replay')).trigger('click')
    await button(wrapper, t('clips.stop')).trigger('click')
    await flushPromises()
    expect(client.POST.mock.calls).toEqual([
      ['/api/admin/clips/skip', { body: { sessionId: 's1' } }],
      ['/api/admin/clips/replay', { body: { sessionId: 's1' } }],
      ['/api/admin/clips/stop'],
    ])
    expect(button(wrapper, t('clips.play')).attributes('disabled')).toBeDefined()
  })

  it('offers only what the state allows', async () => {
    current = queue({ state: 'idle', sessionId: null, current: null, elapsedMs: null, queue: [] })
    const wrapper = await page()
    for (const label of ['clips.skip', 'clips.stop', 'clips.play', 'clips.replay']) {
      expect(button(wrapper, t(label)).attributes('disabled'), label).toBeDefined()
    }
    expect(wrapper.text()).toContain(t('clips.empty'))

    current = queue({ state: 'stopped', elapsedMs: null })
    await vi.advanceTimersByTimeAsync(1000)
    expect(button(wrapper, t('clips.play')).attributes('disabled')).toBeUndefined()
    expect(button(wrapper, t('clips.skip')).attributes('disabled')).toBeDefined()
    expect(wrapper.text()).not.toContain(t('clips.remaining', { s: 18 }))
  })

  it('removes, clears and stops polling when left', async () => {
    const wrapper = await page()
    await button(wrapper, t('clips.remove')).trigger('click')
    await button(wrapper, t('clips.clear')).trigger('click')
    await flushPromises()
    expect(client.DELETE.mock.calls).toEqual([
      ['/api/admin/clips/{entry}', { params: { path: { entry: 'e2' } } }],
      ['/api/admin/clips'],
    ])

    wrapper.unmount()
    const calls = client.GET.mock.calls.length
    await vi.advanceTimersByTimeAsync(5000)
    expect(client.GET.mock.calls.length).toBe(calls)
  })
})
