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
let viewerCounters: Schemas['ViewerCounterBody'][]
let viewerCounts: Schemas['ViewerCountBody'][]
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
  viewerCounts = [
    { twitch_user_id: '1', login: 'fish', display_name: '小魚', value: 3 },
    { twitch_user_id: '2', login: 'berry', display_name: '莓莓', value: 2 },
  ]
  viewerCounters = [{ id: 'v1', name: '猜中死亡次數', total: 5, top: structuredClone(viewerCounts) }]
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
  get = vi.fn((path: string, init?: { params: { query?: { q: string } } }) => {
    if (path === '/api/admin/counters/viewers') return ok(structuredClone(viewerCounters))
    if (path === '/api/admin/counters/viewers/{id}/counts') return ok(structuredClone(viewerCounts))
    if (path === '/api/admin/counters/viewers/{id}/search') {
      const q = init!.params.query!.q.toLowerCase()
      const directory = [...viewerCounts, { twitch_user_id: '3', login: 'fishbone', display_name: '魚骨', value: 0 }]
      return ok(directory.filter((v) => v.login.includes(q)))
    }
    return ok(structuredClone(counters))
  })
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
          'counter.marathon': [],
          'chat.text_size': [],
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
    await wrapper.get('#new-counter-name').element.closest('form')!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: '已被打卡的「!打卡」使用。' }))
    await wrapper.get('#new-counter-command').setValue('抓包')
    await wrapper.get('#new-counter-name').element.closest('form')!.dispatchEvent(new Event('submit'))
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

describe('CountersPage per-viewer counts', () => {
  it('shows the total and top three, finds viewers and counts for them', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const wrapper = await page()
    const row = () => wrapper.get('li.viewer-counter')
    expect(row().text()).toContain(t('counter.viewers.total', { n: 5 }))
    expect(row().get('.top').text()).toContain('小魚 3 · 莓莓 2')

    await row().get('#viewer-search-v1').setValue('fish')
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()
    const results = row().findAll('ul.viewers li')
    expect(results.map((r) => r.get('.who').text())).toEqual(['小魚（fish）', '魚骨（fishbone）'])
    await results[1].findAll('button').find((b) => b.text() === t('counter.plusOne'))!.trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/counters/viewers/{id}/add', {
      params: { path: { id: 'v1' } },
      body: { twitch_user_id: '3', delta: 1 },
    })
    expect(get).toHaveBeenCalledWith('/api/admin/counters/viewers')

    await row().findAll('ul.viewers li')[0].get(`[aria-label="${t('counter.more', { name: '小魚' })}"]`).trigger('click')
    const minus = row().getComponent(Menu).props('model')!.find((i) => i.label === t('counter.minusOne'))!
    minus.command!({} as never)
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith('/api/admin/counters/viewers/{id}/add', {
      params: { path: { id: 'v1' } },
      body: { twitch_user_id: '1', delta: -1 },
    })
    vi.useRealTimers()
  })

  it('lists everyone when asked, and adds per-viewer counters', async () => {
    const wrapper = await page()
    await wrapper.findAll('button').find((b) => b.text() === t('counter.viewers.showAll'))!.trigger('click')
    await flushPromises()
    expect(wrapper.findAll('ol.viewers li').map((r) => r.get('.value').text())).toEqual(['3', '2'])

    await wrapper.get('#new-viewer-counter-name').setValue('猜中')
    await wrapper.get('#new-viewer-counter-name').element.closest('form')!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/counters/viewers', { body: { name: '猜中' } })
  })
})

describe('CountersPage marathon timer', () => {
  const marathon = (p: Partial<Schemas['MarathonPayload']>) => ({
    id: 'm',
    type: 'counter.marathon',
    version: 1,
    seq: 1,
    occurredAt: new Date().toISOString(),
    source: 'system',
    payload: { remainingMs: 3_725_000, running: false, paused: true, live: true, subMinutes: 5, bitsMinutes: 2, maxHours: 24, ...p },
  })
  const receive = async (event: ReturnType<typeof marathon>) => {
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
          'counter.values': [],
          'counter.marathon': [event],
          'chat.text_size': [],
        },
      }),
    })
    await flushPromises()
  }

  it('shows the remaining time and state, pauses, resumes, adds time and saves the options', async () => {
    const wrapper = await page()
    await receive(marathon({}))
    expect(wrapper.get('.remaining').text()).toContain('1:02:05')
    expect(wrapper.get('.remaining').text()).toContain(t('counter.marathon.paused'))
    const button = (label: string) => wrapper.findAll('button').find((b) => b.text() === label)!
    await button(t('counter.marathon.resume')).trigger('click')
    expect(post).toHaveBeenCalledWith('/api/admin/counters/marathon/pause', { body: { paused: false } })

    await wrapper.get('#marathon-add-minutes').element.closest('form')!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/counters/marathon/add', { body: { minutes: 10 } })

    const hours = wrapper.get('#marathon-hours')
    await hours.setValue('12')
    await hours.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/counters/marathon/options', {
      body: { sub_minutes: 5, bits_minutes: 2, max_hours: 12 },
    })

    await receive(marathon({ paused: false, live: false }))
    expect(wrapper.get('.remaining').text()).toContain(t('counter.marathon.waiting'))
  })
})

describe('counterLine', () => {
  it('shows the first counter, or that there is none', () => {
    expect(counterLine([{ id: 'a', name: '死亡次數', value: 12 }, { id: 'b', name: '被抓包', value: 3 }])).toBe('死亡次數：12')
    expect(counterLine([])).toBe(t('counter.empty'))
  })
})
