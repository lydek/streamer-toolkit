// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import Menu from 'primevue/menu'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import { counterLine } from './counters'
import CountersPage from './CountersPage.vue'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t
const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
let counters: Schemas['CounterBody'][]
let socket: { onmessage?: (e: { data: string }) => void }
let get: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>, put: ReturnType<typeof vi.fn>

async function page() {
  const wrapper = mount(CountersPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

const deaths: Schemas['CounterBody'] = { id: 'c1', name: '死亡次數', value: 12, command: '死亡', allowed: 'moderator', per_stream: true }

beforeEach(() => {
  counters = [structuredClone(deaths)]
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    },
  )
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  get = vi.fn(() => ok(structuredClone(counters)))
  post = vi.fn((path: string, init: { body: Schemas['CounterInput'] }) => {
    if (path !== '/api/admin/counters') return ok()
    if (init.body.command === '打卡') {
      return Promise.resolve({ error: { code: 'commands.taken', module: 'checkin', label: '!打卡' }, response: new Response(null, { status: 409 }) })
    }
    const c = { id: `c${counters.length + 1}`, value: 0, ...init.body }
    counters.push(c)
    return ok(c)
  })
  put = vi.fn((_: string, init: { body: Schemas['CounterInput'] }) => ok({ ...deaths, ...init.body }))
  vi.spyOn(client, 'GET').mockImplementation(get as never)
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  vi.spyOn(client, 'DELETE').mockImplementation((() => ok()) as never)
  toastAdd.mockReset()
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('CountersPage', () => {
  it('lists counters with live values and counts with the buttons and the menu', async () => {
    const wrapper = await page()
    expect(wrapper.get('output.value').text()).toBe('12')
    socket.onmessage!({
      data: JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [],
          'chat.removed': [],
          'obs.status': [],
          'obs.state': [],
          'presentation.queue': [],
          'stream.status': [],
          'bgm.player': [],
          'counter.values': [{ id: 'v', type: 'counter.values', version: 1, seq: 1, occurredAt: '', source: 'system', payload: { counters: [{ id: 'c1', name: '死亡次數', value: 13 }] } }],
        },
      }),
    })
    await flushPromises()
    // A chat command counted it.
    expect(wrapper.get('output.value').text()).toBe('13')

    const button = (label: string) => wrapper.findAll('button').find((b) => b.text() === label)!
    await button(t('counter.plusOne')).trigger('click')
    expect(post).toHaveBeenCalledWith('/api/admin/counters/{id}/add', { params: { path: { id: 'c1' } }, body: { delta: 1 } })
    await button(t('counter.reset')).trigger('click')
    expect(post).toHaveBeenCalledWith('/api/admin/counters/{id}/reset', { params: { path: { id: 'c1' } } })
    const minus = wrapper.getComponent(Menu).props('model')!.find((i) => i.label === t('counter.minusOne'))!
    minus.command!({} as never)
    expect(post).toHaveBeenLastCalledWith('/api/admin/counters/{id}/add', { params: { path: { id: 'c1' } }, body: { delta: -1 } })
  })

  it('saves a setting with the others as saved', async () => {
    const wrapper = await page()
    await wrapper.get('#counter-command-c1').setValue('!抓包')
    await wrapper.get('#counter-command-c1').trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/counters/{id}', {
      params: { path: { id: 'c1' } },
      body: { name: '死亡次數', command: '抓包', allowed: 'moderator', per_stream: true },
    })
  })

  it('adds counters, and says which feature has a command name', async () => {
    const wrapper = await page()
    await wrapper.get('#new-counter-name').setValue('被抓包')
    await wrapper.get('#new-counter-command').setValue('!打卡')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: '已被打卡的「!打卡」使用。' }))
    await wrapper.get('#new-counter-command').setValue('抓包')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith('/api/admin/counters', {
      body: { name: '被抓包', command: '抓包', allowed: 'moderator', per_stream: false },
    })
    expect(wrapper.findAll('li.counter')).toHaveLength(2)
  })

  it('deletes a counter after confirming', async () => {
    const wrapper = await page()
    const del = wrapper.getComponent(Menu).props('model')!.find((i) => i.label === t('counter.delete'))!
    del.command!({} as never)
    await flushPromises()
    expect(wrapper.text()).toContain(t('counter.deleteNote'))
    counters = []
    await wrapper.findAll('button').find((b) => b.text() === t('counter.confirmDelete'))!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain(t('counter.empty'))
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('counter.deleted', { name: '死亡次數' }) }))
  })
})

describe('counterLine', () => {
  it('shows the first counter, or that there is none', () => {
    expect(counterLine([{ id: 'a', name: '死亡次數', value: 12 }, { id: 'b', name: '被抓包', value: 3 }])).toBe('死亡次數：12')
    expect(counterLine([])).toBe(t('counter.empty'))
  })
})
