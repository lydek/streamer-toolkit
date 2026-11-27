// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import SongListPage from './SongListPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const fail = (code: string) => Promise.resolve({ error: { code }, response: new Response(null, { status: 409 }) })

const song = (id: string, title: string, more: object = {}) => ({
  id,
  title,
  artist: '',
  category_id: null,
  youtube_url: null,
  last_sung_at: null,
  request_count: 0,
  ...more,
})
let library: { songs: unknown[]; categories: unknown[] }

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true
  }
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false
  }
})
beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  library = {
    songs: [
      song('s1', '晴天', { artist: '周杰倫', category_id: 'c1', request_count: 14, youtube_url: 'https://youtu.be/abc' }),
      song('s2', 'Lemon', { artist: '米津玄師', category_id: 'c2', last_sung_at: new Date(Date.now() - 3 * 86_400_000).toISOString() }),
    ],
    categories: [
      { id: 'c1', name: '華語', requestable: true, songs: 1 },
      { id: 'c2', name: '待練勿點', requestable: false, songs: 1 },
    ],
  }
  vi.spyOn(client, 'GET').mockImplementation((() => ok(library)) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(SongListPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}

it('lists each song with its category, when it was last sung and how often it was requested', async () => {
  const w = await open()
  const rows = w.findAll('tbody tr').map((r) => r.findAll('th, td').map((c) => c.text()))
  expect(rows).toEqual([
    ['晴天', '周杰倫', '華語', t('songList.never'), '14', ''],
    ['Lemon', '米津玄師', '待練勿點', t('songList.daysAgo', { n: 3 }), '0', ''],
  ])
  expect(w.find('[data-song="s1"] .pi-youtube').exists()).toBe(true)
  expect(w.text()).toContain(t('songList.links', { n: 2, linked: 1 }))
})

it('searches by title, artist or category', async () => {
  const w = await open()
  await w.get('input[type="search"]').setValue('米津')
  expect(w.findAll('tbody tr').map((r) => r.attributes('data-song'))).toEqual(['s2'])
  await w.get('input[type="search"]').setValue('華語')
  expect(w.findAll('tbody tr').map((r) => r.attributes('data-song'))).toEqual(['s1'])
})

it('opens or closes a category for requests from its chip', async () => {
  const put = vi.spyOn(client, 'PUT').mockImplementation((() => ok({})) as never)
  const w = await open()
  const chip = w.get('.chips [data-category="c2"]')
  expect(chip.attributes('aria-pressed')).toBe('false')
  await chip.trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/song-list/categories/{id}', {
    params: { path: { id: 'c2' } },
    body: { name: '待練勿點', requestable: true },
  })
})

it('adds a song, and shows why the Host refused one', async () => {
  const post = vi.spyOn(client, 'POST').mockImplementationOnce((() => fail('songs.duplicate')) as never)
  const w = await open()
  await w.findAll('button').find((b) => b.text() === t('songList.add'))!.trigger('click')
  await w.get('#song-name').setValue('晴天')
  await w.get('#song-artist').setValue('周杰倫')
  await w.get('#song-form').trigger('submit')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/song-list/songs', {
    body: { title: '晴天', artist: '周杰倫', category_id: null, youtube_url: '' },
  })
  expect(w.get('.song-dialog [role="alert"]').text()).toBe(t('errors.songs.duplicate'))
  expect((w.get('.song-dialog').element as HTMLDialogElement).open).toBe(true)
})

it('previews an import with what it adds, updates and skips, and writes only when confirmed', async () => {
  const preview = { added: 3, updated: 1, unchanged: 2, new_categories: ['日語'], skipped: [{ sheet: '華語', row: 4, reason: 'no_title' }] }
  const post = vi
    .spyOn(client, 'POST')
    .mockImplementationOnce((() => ok(preview)) as never)
    .mockImplementationOnce((() => ok(preview)) as never)
  const w = await open()
  const input = w.get('input[data-import]')
  const file = new File(['歌名\n晴天'], 'songs.csv')
  Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
  await input.trigger('change')
  await flushPromises()
  expect(post).toHaveBeenLastCalledWith(
    '/api/admin/song-list/import/preview',
    expect.objectContaining({ params: { query: { format: 'csv' } }, body: file }),
  )
  const dialog = w.get('.import-dialog')
  expect(dialog.get('[data-preview]').text()).toBe(t('songList.importSummary', { added: 3, updated: 1, unchanged: 2 }))
  expect(dialog.text()).toContain('日語')
  expect(dialog.get('.skipped').text()).toBe(`${t('songList.skipWhereSheet', { sheet: '華語', row: 4 })}：${t('songList.skip.no_title')}`)
  expect(post).toHaveBeenCalledTimes(1)
  await dialog.findAll('button').find((b) => b.text() === t('songList.importOk'))!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenLastCalledWith('/api/admin/song-list/import', expect.objectContaining({ body: file }))
  expect((dialog.element as HTMLDialogElement).open).toBe(false)
})

it('refuses a file over 5 MB without sending it', async () => {
  const post = vi.spyOn(client, 'POST')
  const w = await open()
  const input = w.get('input[data-import]')
  const file = new File(['x'], 'big.xlsx')
  Object.defineProperty(file, 'size', { value: 5 * 1024 * 1024 + 1 })
  Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
  await input.trigger('change')
  await flushPromises()
  expect(post).not.toHaveBeenCalled()
  expect(w.get('.import-dialog [role="alert"]').text()).toBe(t('errors.songs.unreadable_file'))
})
