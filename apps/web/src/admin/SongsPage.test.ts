// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import SongsPage from './SongsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

/** The page's live connection: the last socket opened, to push events into. */
class FakeSocket {
  static last: FakeSocket
  onmessage?: (e: { data: string }) => void
  onclose?: () => void
  constructor() {
    FakeSocket.last = this
  }
  close() {}
}

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const fail = (code: string) => Promise.resolve({ error: { code }, response: new Response(null, { status: 409 }) })

const request = (id: string, title: string, more: object = {}) => ({
  id,
  song_id: `s-${id}`,
  title,
  artist: '',
  viewer: null,
  by_hand: false,
  requested_at: '2026-11-28T12:00:00.000000Z',
  finished_at: null,
  ...more,
})
let board: { singing: unknown; queued: unknown[]; sung: unknown[] }

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('WebSocket', FakeSocket)
  board = {
    singing: request('r1', '晴天', { artist: '周杰倫', viewer: '小魚' }),
    queued: [request('r2', '小幸運', { artist: '田馥甄', viewer: '橘子' }), request('r3', '告白氣球')],
    sung: [request('r0', '稻香', { finished_at: '2026-11-28T13:18:00.000000Z' })],
  }
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(
      path === '/api/admin/song-list'
        ? { songs: [{ id: 's9', title: '倒數', artist: '鄧紫棋', category_id: null, youtube_url: null, last_sung_at: null, request_count: 0 }], categories: [] }
        : board,
    )) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(SongsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}

const button = (w: Awaited<ReturnType<typeof open>>, label: string) => w.findAll('button').find((b) => b.text() === label || b.attributes('aria-label') === label)!

it('shows the song being sung, the queue in order and what was sung', async () => {
  const w = await open()
  expect(w.get('[data-singing]').text()).toContain('晴天')
  expect(w.get('[data-singing]').text()).toContain(t('songs.by', { viewer: '小魚' }))
  expect(w.findAll('.queue:not(.sung) li').map((li) => li.find('.t').text())).toEqual(['小幸運田馥甄 · @橘子', '告白氣球'])
  expect(w.get('.sung li').text()).toContain('稻香')
  // While a song is being sung, no other one can start.
  expect(w.find(`[aria-label="${t('songs.startLabel', { title: '小幸運' })}"]`).exists()).toBe(false)
})

it('finishes the song and reads the queue again', async () => {
  const post = vi.spyOn(client, 'POST').mockImplementation((() => ok()) as never)
  const w = await open()
  const get = vi.mocked(client.GET)
  get.mockClear()
  await button(w, t('songs.finish')).trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/song-requests/finish')
  expect(get).toHaveBeenCalledWith('/api/admin/song-requests')
})

it('moves a queued song and removes one', async () => {
  const put = vi.spyOn(client, 'PUT').mockImplementation((() => ok()) as never)
  const del = vi.spyOn(client, 'DELETE').mockImplementation((() => ok()) as never)
  const w = await open()
  await button(w, `${t('arrange.later')}：小幸運`).trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/song-requests/order', { body: { ids: ['r3', 'r2'] } })
  await button(w, t('songs.removeLabel', { title: '告白氣球' })).trigger('click')
  await flushPromises()
  expect(del).toHaveBeenCalledWith('/api/admin/song-requests/{id}', { params: { path: { id: 'r3' } } })
})

it('starts a queued song when none is being sung, and shows why the Host refused', async () => {
  board.singing = null
  vi.spyOn(client, 'POST').mockImplementation((() => fail('songs.not_allowed')) as never)
  const w = await open()
  expect(w.text()).toContain(t('songs.nothingNow'))
  expect(button(w, t('songs.finish')).attributes('disabled')).toBeDefined()
  await button(w, t('songs.startLabel', { title: '小幸運' })).trigger('click')
  await flushPromises()
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('errors.songs.not_allowed') }))
})

it('reads the queue again when a request comes in from chat', async () => {
  await open()
  const get = vi.mocked(client.GET)
  get.mockClear()
  FakeSocket.last.onmessage!({ data: JSON.stringify({ type: 'songs.changed', id: 'e', version: 1, seq: 2, occurredAt: '', source: 'system', payload: {} }) })
  await flushPromises()
  expect(get).toHaveBeenCalledWith('/api/admin/song-requests')
})

it('requests a song by hand for someone', async () => {
  const post = vi.spyOn(client, 'POST').mockImplementation((() => ok()) as never)
  const w = await open()
  ;(w.vm as unknown as { $: { setupState: { pick: string } } }).$.setupState.pick = 's9'
  await w.get(`input[aria-label="${t('songs.for')}"]`).setValue(' 莓莓 ')
  await w.get('form.add').trigger('submit')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/song-requests', { body: { song_id: 's9', viewer: '莓莓' } })
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('songs.added', { title: '倒數' }) }))
})
