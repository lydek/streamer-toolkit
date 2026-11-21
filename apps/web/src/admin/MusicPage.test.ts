// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import MultiSelect from 'primevue/multiselect'
import Select from 'primevue/select'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import { MUSIC_MAX_BYTES, formatLength } from './music'
import MusicPage from './MusicPage.vue'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t
const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const fail = (code: string, status: number) => Promise.resolve({ error: { code }, response: new Response(null, { status }) })

let library: Schemas['MusicLibrary']
// Lengths in seconds that the fake `<audio>` reads per URL.
let lengths: Record<string, number>
let get: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>, put: ReturnType<typeof vi.fn>
let socket: { onmessage?: (e: { data: string }) => void }

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
      { id: 't1', title: 'Lo-fi Morning', duration_ms: 188_000, missing: false, used_by: [] },
      { id: 't2', title: 'Gone', duration_ms: 60_000, missing: true, used_by: [] },
    ],
    playlists: [{ id: 'p1', name: '聊天時段', tracks: ['t1'], used_by: [] }],
    scene_rules: [],
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
  get = vi.fn((path: string) => (path === '/api/admin/sounds' ? ok({ sounds: [], slots: {}, volume: 80 }) : ok(structuredClone(library))))
  post = vi.fn((path: string, init: { params?: { query: { name: string } }; body: { name: string } }) => {
    if (path === '/api/admin/music/playlists') {
      const p = { id: `p${library.playlists.length + 1}`, name: init.body.name, tracks: [], used_by: [] }
      library.playlists.push(p)
      return ok(p)
    }
    const track = { id: `t${library.tracks.length + 1}`, title: init.params!.query.name.replace(/\.\w+$/, ''), duration_ms: null, missing: false, used_by: [] }
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

describe('MusicPage player', () => {
  const player = (p: Partial<Schemas['BgmPlayerPayload']> = {}) => ({
    id: 'b',
    type: 'bgm.player',
    version: 1,
    seq: 1,
    occurredAt: new Date().toISOString(),
    source: 'system',
    payload: { state: 'stopped', playlist: null, track: null, positionMs: 0, onAir: true, shuffle: false, volume: 50, ...p },
  })
  const snapshot = (event: ReturnType<typeof player>) => ({
    type: 'snapshot',
    version: 1,
    seq: 1,
    payload: { 'chat.message': [], 'chat.removed': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [event], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
  })
  const receive = async (msg: unknown) => {
    socket.onmessage!({ data: JSON.stringify(msg) })
    await flushPromises()
  }
  const control = (wrapper: Awaited<ReturnType<typeof page>>, key: string) =>
    wrapper.get<HTMLButtonElement>(`[aria-label="${t(`music.player.${key}`)}"]`)
  const slide = async (wrapper: Awaited<ReturnType<typeof page>>, id: string, value: number) => {
    const input = wrapper.get<HTMLInputElement>(`#${id}`)
    await input.setValue(value)
    await input.trigger('change')
    await flushPromises()
  }

  it('plays the chosen playlist and shows what plays, or that no overlay is open', async () => {
    const wrapper = await page()
    await receive(snapshot(player()))
    expect(wrapper.text()).toContain(t('music.player.stopped'))
    expect(control(wrapper, 'play').element.disabled).toBe(true)

    wrapper.findAllComponents(Select)[0].vm.$emit('update:modelValue', 'p1')
    await flushPromises()
    await control(wrapper, 'play').trigger('click')
    expect(post).toHaveBeenCalledWith('/api/admin/music/player', { body: { action: 'play', playlist: 'p1' } })

    const track = { id: 't1', title: 'Lo-fi Morning', url: '/music/t1', durationMs: 188_000 }
    await receive({ ...player({ state: 'playing', playlist: 'p1', track, positionMs: 65_000 }), seq: 2 })
    expect(wrapper.text()).toContain(t('music.player.playing', { title: 'Lo-fi Morning' }))
    expect(wrapper.get('output[for="player-position"]').text()).toBe('1:05 / 3:08')
    expect(wrapper.find('[role="status"]').exists()).toBe(false)
    await control(wrapper, 'pause').trigger('click')
    expect(post).toHaveBeenLastCalledWith('/api/admin/music/player', { body: { action: 'pause' } })
    await slide(wrapper, 'player-position', 120_000)
    expect(post).toHaveBeenLastCalledWith('/api/admin/music/player', { body: { action: 'seek', position_ms: 120_000 } })
    for (const key of ['next', 'previous', 'stop']) {
      await control(wrapper, key).trigger('click')
      expect(post).toHaveBeenLastCalledWith('/api/admin/music/player', { body: { action: key } })
    }

    // Not pretending: without a music overlay it says so instead of the track.
    await receive({ ...player({ state: 'playing', playlist: 'p1', track, onAir: false }), seq: 3 })
    expect(wrapper.get('[role="status"]').text()).toBe(t('music.player.offAir'))
    expect(wrapper.text()).not.toContain(t('music.player.playing', { title: 'Lo-fi Morning' }))
  })

  it('saves the music and sound volumes and the order', async () => {
    const wrapper = await page()
    await receive(snapshot(player()))
    expect(wrapper.get<HTMLInputElement>('#sound-volume').element.value).toBe('80')
    await slide(wrapper, 'player-volume', 30)
    expect(put).toHaveBeenCalledWith('/api/admin/music/player/options', { body: { volume: 30, shuffle: false } })
    await slide(wrapper, 'sound-volume', 60)
    expect(put).toHaveBeenCalledWith('/api/admin/sounds/volume', { body: { volume: 60 } })
    await wrapper.get('#player-shuffle').setValue(true)
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/music/player/options', { body: { volume: 50, shuffle: true } })
  })

  it('shows why the Host refused to play', async () => {
    post.mockImplementation(() => fail('music.nothing_to_play', 409))
    const wrapper = await page()
    await receive(snapshot(player({ playlist: 'p1' })))
    await control(wrapper, 'play').trigger('click')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('errors.music.nothing_to_play') }))
  })
})

describe('formatLength', () => {
  it('shows minutes and seconds, and hours once there are any', () => {
    expect(formatLength(188_000)).toBe('3:08')
    expect(formatLength(59_600)).toBe('1:00')
    expect(formatLength(36_000_000)).toBe('10:00:00')
  })
})

describe('MusicPage scene rules', () => {
  const rule = (p: Partial<Schemas['SceneRuleBody']> = {}): Schemas['SceneRuleBody'] => ({
    id: 'r1',
    name: '聊天',
    scenes: ['Chat'],
    playlist: 'p1',
    shuffle: false,
    volume: 60,
    autoplay: true,
    stop_on_leave: true,
    enabled: true,
    ...p,
  })
  const obsSnapshot = (scenes: string[]) => ({
    type: 'snapshot',
    version: 1,
    seq: 1,
    payload: {
      'chat.message': [],
      'chat.removed': [],
      'obs.status': [],
      'obs.state': [
        {
          id: 'o',
          type: 'obs.state',
          version: 1,
          seq: 1,
          occurredAt: '',
          source: 'system',
          payload: { scene: scenes[0], scenes: scenes.map((name) => ({ name, items: [] })), inputs: [], streaming: false, recording: false },
        },
      ],
      'presentation.queue': [],
      'stream.status': [],
      'bgm.player': [],
      'counter.values': [],
      'counter.marathon': [],
      'chat.text_size': [],
    },
  })

  it('lists the scenes without a rule, saves every change and adds rules', async () => {
    library.scene_rules = [rule()]
    const wrapper = await page()
    socket.onmessage!({ data: JSON.stringify(obsSnapshot(['Chat', 'Singing'])) })
    await flushPromises()
    expect(wrapper.text()).toContain(t('music.rules.unmapped', { scene: 'Singing' }))
    expect(wrapper.text()).not.toContain(t('music.rules.unmapped', { scene: 'Chat' }))

    const volume = wrapper.get<HTMLInputElement>('#rule-volume-0')
    await volume.setValue(35)
    await volume.trigger('change')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/music/scene-rules', { body: [rule({ volume: 35 })] })

    await wrapper.get('#rule-leave-0').setValue(false)
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/music/scene-rules', { body: [rule({ stop_on_leave: false })] })

    const add = wrapper.findAll('button').find((b) => b.text() === t('music.rules.add'))!
    await add.trigger('click')
    await flushPromises()
    expect(put.mock.lastCall![1].body[1]).toEqual(
      rule({ id: '', name: t('music.rules.newName', { n: 2 }), scenes: [], volume: 50 }),
    )
  })

  it('shows a rule whose playlist is gone, which cannot be enabled', async () => {
    library.scene_rules = [rule({ playlist: null, enabled: false })]
    const wrapper = await page()
    expect(wrapper.text()).toContain(t('music.rules.playlistGone'))
    expect(wrapper.get('#rule-enabled-0').attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain(t('scene.notConnected'))
  })

  it('lists the rules a playlist or track deletion affects', async () => {
    library.playlists[0].used_by = ['聊天']
    library.tracks[0].used_by = ['聊天']
    const wrapper = await page()
    const deleteIn = (row: string) =>
      wrapper.get(row).findAll('button').find((b) => b.text() === t('music.delete'))!.trigger('click')
    await deleteIn('li.playlist')
    expect(wrapper.get('li.playlist').text()).toContain(t('music.playlistUsedBy'))
    expect(wrapper.get('li.playlist .confirm li').text()).toBe('聊天')
    await deleteIn('li.track')
    expect(wrapper.get('li.track').text()).toContain(t('music.trackUsedBy'))
  })
})
