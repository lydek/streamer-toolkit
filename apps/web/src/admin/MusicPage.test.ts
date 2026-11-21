// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import MultiSelect from 'primevue/multiselect'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { MUSIC_MAX_BYTES, formatLength } from './music'
import MusicPage from './MusicPage.vue'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t
const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const fail = (code: string, status: number) => Promise.resolve({ error: { code }, response: new Response(null, { status }) })

type Track = { id: string; title: string; duration_ms: number | null; missing: boolean }
type Playlist = { id: string; name: string; tracks: string[] }
let library: { tracks: Track[]; playlists: Playlist[] }
// Lengths in seconds that the fake `<audio>` reads per URL.
let lengths: Record<string, number>
let get: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>, put: ReturnType<typeof vi.fn>

async function page() {
  const wrapper = mount(MusicPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

function chooseFiles(wrapper: Awaited<ReturnType<typeof page>>, files: File[]) {
  const input = wrapper.get<HTMLInputElement>('#music-file')
  Object.defineProperty(input.element, 'files', { value: files, configurable: true })
  return input.trigger('change')
}

beforeEach(() => {
  library = {
    tracks: [
      { id: 't1', title: 'Lo-fi Morning', duration_ms: 188_000, missing: false },
      { id: 't2', title: 'Gone', duration_ms: 60_000, missing: true },
    ],
    playlists: [{ id: 'p1', name: '聊天時段', tracks: ['t1'] }],
  }
  lengths = {}
  vi.stubGlobal(
    'Audio',
    class {
      duration = NaN
      onloadedmetadata: (() => void) | null = null
      onerror: (() => void) | null = null
      set src(url: string) {
        queueMicrotask(() => {
          if (url in lengths) {
            this.duration = lengths[url]
            this.onloadedmetadata?.()
          } else this.onerror?.()
        })
      }
    },
  )
  get = vi.fn(() => ok(structuredClone(library)))
  post = vi.fn((path: string, init: { params?: { query: { name: string } }; body: { name: string } }) => {
    if (path === '/api/admin/music/playlists') {
      const p = { id: `p${library.playlists.length + 1}`, name: init.body.name, tracks: [] }
      library.playlists.push(p)
      return ok(p)
    }
    const track = { id: `t${library.tracks.length + 1}`, title: init.params!.query.name.replace(/\.\w+$/, ''), duration_ms: null, missing: false }
    library.tracks.push(track)
    return ok(track)
  })
  put = vi.fn((path: string, init: { params: { path: { id: string } }; body: { duration_ms: number } }) => {
    if (path === '/api/admin/music/{id}/duration') library.tracks.find((t) => t.id === init.params.path.id)!.duration_ms = init.body.duration_ms
    return ok()
  })
  vi.spyOn(client, 'GET').mockImplementation(get as never)
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  toastAdd.mockReset()
})

enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('MusicPage', () => {
  it('lists each track with its length, or says its file is gone', async () => {
    const wrapper = await page()
    const rows = wrapper.findAll('li.track')
    expect(rows.map((r) => (r.get('input').element as HTMLInputElement).value)).toEqual(['Lo-fi Morning', 'Gone'])
    expect(rows[0].text()).toContain('3:08')
    expect(rows[1].text()).toContain(t('music.missing'))
    expect(rows[1].get(`[aria-label="${t('music.play', { title: 'Gone' })}"]`).attributes('disabled')).toBeDefined()
  })

  it('imports each chosen file, refuses one over the limit, and measures the new tracks', async () => {
    lengths['/music/t3'] = 125.4
    const wrapper = await page()
    const big = new File(['x'], 'big.wav')
    Object.defineProperty(big, 'size', { value: MUSIC_MAX_BYTES + 1 })
    await chooseFiles(wrapper, [new File(['ID3'], 'Opening.mp3'), big])
    await flushPromises()

    expect(post).toHaveBeenCalledTimes(1)
    expect(post.mock.calls[0][1].params.query.name).toBe('Opening.mp3')
    expect(wrapper.get('[role="alert"]').text()).toBe(
      t('music.importFailed', { file: 'big.wav', reason: t('errors.music.too_large') }),
    )
    expect(put).toHaveBeenCalledWith('/api/admin/music/{id}/duration', { params: { path: { id: 't3' } }, body: { duration_ms: 125_400 } })
    const added = wrapper.findAll('li.track')[2]
    expect((added.get('input').element as HTMLInputElement).value).toBe('Opening')
    expect(added.text()).toContain('2:05')
  })

  it('shows why the Host refused a file', async () => {
    post.mockImplementation(() => fail('music.unsupported_type', 400))
    const wrapper = await page()
    await chooseFiles(wrapper, [new File(['RIFF'], 'fake.mp3')])
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain(t('errors.music.unsupported_type'))
    expect(toastAdd).not.toHaveBeenCalled()
  })

  it('puts a track into playlists and adds new playlists', async () => {
    const wrapper = await page()
    await wrapper.get('#new-playlist').setValue('遊戲 BGM')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/music/playlists', { body: { name: '遊戲 BGM' } })
    expect(wrapper.findAll('li.playlist')).toHaveLength(2)

    const select = wrapper.findAllComponents(MultiSelect)[1]
    expect(select.props('modelValue')).toEqual([])
    select.vm.$emit('update:modelValue', ['p1', 'p2'])
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/music/{id}/playlists', {
      params: { path: { id: 't2' } },
      body: { playlists: ['p1', 'p2'] },
    })
  })
})

describe('formatLength', () => {
  it('shows minutes and seconds, and hours once there are any', () => {
    expect(formatLength(188_000)).toBe('3:08')
    expect(formatLength(59_600)).toBe('1:00')
    expect(formatLength(36_000_000)).toBe('10:00:00')
  })
})
