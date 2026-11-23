// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { useAfterNumbers } from './afterNumbers'
import { useDock } from './dock'
import { onDrawerSubmit, useDrawer } from './drawer'
import { type Stage, UNBUILT_TILES } from './home'
import HomeView from './HomeView.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data: unknown) => Promise.resolve({ data, response: new Response(null, { status: 200 }) })
const responses: Record<string, unknown> = {
  '/api/admin/twitch': { client_id: '', status: { state: 'signed_out' } },
  '/api/admin/home-tiles': null,
  '/api/admin/modules': [{ id: 'chat', enabled: true, closed: false }],
  '/api/admin/templates/chat': { error: null },
  '/api/admin/feed': [],
}
let put: ReturnType<typeof vi.fn>
const drawer = useDrawer()

async function home() {
  const wrapper = mount(HomeView, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}
const pencil = () => [...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes(t('home.arrange')))
const done = () => [...document.querySelectorAll('header button')].find((b) => b.textContent?.includes(t('arrange.done')))
const pickStage = async (stage: Stage) => {
  ;[...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.trim() === t(`stage.${stage}`))!.click()
  await flushPromises()
}
const esc = async () => {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
  await flushPromises()
}

beforeEach(() => {
  vi.stubGlobal('WebSocket', class {
    close() {}
  })
  // Wider than 1000px: the drawer is a non-modal aside.
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(responses[path])) as never)
  put = vi.fn(() => ok(undefined))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  toastAdd.mockReset()
})

enableAutoUnmount(afterEach)
afterEach(async () => {
  await drawer.close()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('HomeView mixer tile', () => {
  it('mutes only the chosen mic, shows OBS state once confirmed, and says why it cannot', async () => {
    let socket!: { onmessage?: (e: { data: string }) => void }
    vi.stubGlobal('WebSocket', class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    })
    let seq = 0
    const obsState = (inputs: object[]) =>
      socket.onmessage!({ data: JSON.stringify({ seq: ++seq, type: 'obs.state', payload: { scene: 'Game', scenes: [{ name: 'Game', items: [] }], inputs, streaming: false, recording: false } }) })
    const mic = (muted: boolean) => ({ name: 'Mic', muted, global: true })
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(
        path === '/api/admin/modules'
          ? [{ id: 'mixer', enabled: true, closed: false }]
          : path === '/api/admin/mixer/mic'
            ? { input: 'Mic' }
            : path === '/api/admin/checkin/today'
              ? { count: 0, recent: [] }
              : responses[path],
      )) as never)
    await home()
    const topics = ['obs.status', 'obs.state', 'presentation.queue', 'stream.status', 'bgm.player', 'counter.values', 'counter.marathon', 'screen.break', 'screen.warnings', 'screen.appearance', 'leaderboard.update', 'chat.text_size', 'chat.message']
    socket.onmessage!({ data: JSON.stringify({ seq: ++seq, type: 'snapshot', payload: Object.fromEntries(topics.map((k) => [k, []])) }) })
    obsState([mic(false), { name: 'Desktop', muted: false, global: true }])
    await flushPromises()
    const tile = () => document.querySelector('[data-drawer-tile="mixer"]')!.closest('.tile-wrap')!
    const button = () => tile().querySelector<HTMLButtonElement>('.tile-actions button')!
    expect(tile().textContent).toContain(t('mixer.tile.on'))
    expect(button().textContent).toContain(t('mixer.tile.mute'))
    button().click()
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/input', { body: { input: 'Mic', muted: true } })
    // Not muted until OBS says so.
    expect(tile().textContent).toContain(t('mixer.tile.on'))
    obsState([mic(true), { name: 'Desktop', muted: false, global: true }])
    await flushPromises()
    expect(tile().textContent).toContain(t('mixer.tile.muted'))
    expect(button().textContent).toContain(t('mixer.tile.unmute'))
    expect(tile().querySelector('.dot')).not.toBeNull()

    // The chosen mic is gone: no other input is touched.
    obsState([{ name: 'Desktop', muted: false, global: true }])
    await flushPromises()
    expect(button().disabled).toBe(true)
    expect(tile().textContent).toContain(t('mixer.micMissing', { input: 'Mic' }))
    expect(put).toHaveBeenCalledTimes(1)
  })
})

describe('HomeView chat tile', () => {
  it('steps aside while the chat panel is docked beside the page', async () => {
    const dock = useDock()
    dock.docked = true
    await home()
    expect(document.querySelector('[data-drawer-tile="chat"]')).toBeNull()
    dock.docked = false
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).not.toBeNull()
  })
})

describe('HomeView stage blocks', () => {
  it('come from the home of each stage, and 待處理 only with items', async () => {
    await home()
    const headings = () => [...document.querySelectorAll('#tiles h2')].map((h) => h.textContent)
    // No stream yet: 開播前, whose home leads with the checklist.
    expect(headings()).toContain(t('home.blocks.checklist'))
    expect(document.querySelector('#tiles li.size-w h2')?.textContent).toBe(t('home.blocks.checklist'))
    expect(document.querySelector('[data-drawer-tile="queue"]')).toBeNull()
    // 直播中 has its own home: the queue as a small tile, no checklist; 待處理 has no items yet.
    await pickStage('live')
    expect(headings()).not.toContain(t('home.blocks.checklist'))
    expect(headings()).not.toContain(t('home.blocks.pending'))
    expect(document.querySelector('[data-drawer-tile="queue"]')?.getAttribute('aria-expanded')).toBe('false')
    await pickStage('after')
    expect(headings()).toContain(t('home.blocks.summary'))
  })
})

describe('HomeView stage homes', () => {
  it('edits the stage picked in edit mode and saves only that one', async () => {
    // Each stage answers its own order: 開播前 arranged, 直播中 never (its defaults), 下播後 emptied.
    const orders: Record<string, string[] | null> = { before: ['chat'], live: null, after: [] }
    vi.mocked(client.GET).mockImplementation(((path: string, init?: { params?: { query?: { stage?: string } } }) =>
      ok(path === '/api/admin/home-tiles' ? orders[init!.params!.query!.stage!] : responses[path])) as never)
    await home()
    const tileIds = () => [...document.querySelectorAll('#tiles [data-drawer-tile], #tiles [data-tile]')].map(
      (el) => el.getAttribute('data-drawer-tile') ?? el.getAttribute('data-tile'),
    )
    expect(tileIds()).toEqual(['chat'])
    pencil()!.click()
    await flushPromises()
    await pickStage('live')
    document.querySelector<HTMLButtonElement>('[data-tile="chat"] [data-action="remove"]')!.click()
    await flushPromises()
    expect(put).toHaveBeenCalledTimes(1)
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', {
      params: { query: { stage: 'live' } },
      body: ['pending', 'alerts', 'scene', 'mixer', 'bgm', 'counter', 'sound', 'screen-tools', 'queue', 'redemption', 'checkin'],
    })
    // The other stages keep their own orders: 開播前 still has its chat tile, 下播後 stays empty.
    await pickStage('before')
    expect(tileIds()).toEqual(['chat'])
    await pickStage('after')
    expect(tileIds()).toEqual([])
  })
})

describe('HomeView blocks that open in place', () => {
  it('opens a small tile into its block and back, with focus kept on the same place, and saves nothing', async () => {
    // 本場直播 and 事件流 are not on the live home by default; this home was arranged with them.
    vi.mocked(client.GET).mockImplementation(((path: string, init?: { params?: { query?: { stage?: string } } }) =>
      ok(path === '/api/admin/home-tiles' ? (init!.params!.query!.stage === 'live' ? ['feed', 'stats', 'queue'] : null) : responses[path])) as never)
    await home()
    await pickStage('live')
    const toggle = () => document.querySelector<HTMLButtonElement>('#tiles [data-expand="feed"]')!
    toggle().click()
    await flushPromises()
    expect(document.getElementById('block-feed')?.closest('li')?.classList).toContain('size-l')
    expect(toggle().getAttribute('aria-expanded')).toBe('true')
    expect(document.activeElement).toBe(toggle())
    // The feed loads with the live messages, which this test does not send.
    expect(document.getElementById('block-feed')?.textContent).toContain(t('app.loading'))
    toggle().click()
    await flushPromises()
    expect(document.getElementById('block-feed')).toBeNull()
    expect(toggle().getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(toggle())

    document.querySelector<HTMLButtonElement>('#tiles [data-expand="stats"]')!.click()
    await flushPromises()
    expect(document.getElementById('block-stats')?.closest('li')?.classList).toContain('size-w')
    expect(put).not.toHaveBeenCalled()
  })

  it('opens the presentation queue in a drawer like other tiles, not in place', async () => {
    await home()
    await pickStage('live')
    expect(document.querySelector('#tiles [data-expand="queue"]')).toBeNull()
    document.querySelector<HTMLButtonElement>('#tiles [data-drawer-tile="queue"]')!.click()
    await flushPromises()
    expect(drawer.current.value).toBe('queue')
    expect(document.getElementById('block-queue')).toBeNull()
    expect(document.querySelector('.drawer-content .queue')).not.toBeNull()
  })
})

describe('HomeView numbers after the stream', () => {
  afterEach(() => Object.assign(useAfterNumbers(), { mode: 'show', revealed: [] }))

  it('masks a tile number with a button that shows only that one, and folds the tiles away for 整頁不顯示', async () => {
    const data: Record<string, unknown> = {
      ...responses,
      '/api/admin/home-tiles': ['sound'],
      '/api/admin/modules': [{ id: 'sound', enabled: true, closed: false }],
      '/api/admin/sounds': { sounds: [{ id: 'a', label: '叮咚' }], slots: {}, volume: 100 },
    }
    vi.mocked(client.GET).mockImplementation(((path: string) => ok(data[path])) as never)
    vi.spyOn(client, 'POST').mockImplementation((() => ok(undefined)) as never)
    await home()
    Object.assign(useAfterNumbers(), { mode: 'mask', revealed: [] })
    await pickStage('after')
    const tile = () => document.querySelector('[data-drawer-tile="sound"]')!
    expect(tile().querySelector('.num')!.textContent).toContain('––')
    const show = [...tile().parentElement!.querySelectorAll<HTMLButtonElement>('.tile-actions button')].find(
      (b) => b.textContent?.trim() === t('home.numbers.reveal'),
    )!
    show.click()
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/after-numbers', { body: { mode: 'mask', revealed: ['sound'] } })
    expect(tile().querySelector('.num')!.textContent).toContain('1')
    expect(drawer.current.value).toBeNull()

    Object.assign(useAfterNumbers(), { mode: 'hide' })
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="sound"]')).toBeNull()
    ;[...document.querySelectorAll<HTMLButtonElement>('#tiles button')].find((b) => b.textContent?.trim() === t('home.numbers.peek'))!.click()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="sound"]')).not.toBeNull()
    // Only for this visit: back on another stage and again, the tiles are folded.
    await pickStage('live')
    await pickStage('after')
    expect(document.querySelector('[data-drawer-tile="sound"]')).toBeNull()
  })
})

describe('HomeView masked numbers and word values', () => {
  afterEach(() => Object.assign(useAfterNumbers(), { mode: 'show', revealed: [] }))

  it('leaves a state like 開著 on the mic tile, and its button, alone', async () => {
    let socket!: { onmessage?: (e: { data: string }) => void }
    vi.stubGlobal('WebSocket', class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    })
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(
        path === '/api/admin/modules'
          ? [{ id: 'mixer', enabled: true, closed: false }]
          : path === '/api/admin/mixer/mic'
            ? { input: 'Mic' }
            : path === '/api/admin/home-tiles'
              ? ['mixer']
              : path === '/api/admin/checkin/today'
                ? { count: 0, recent: [] }
                : responses[path],
      )) as never)
    Object.assign(useAfterNumbers(), { mode: 'mask', revealed: [] })
    await home()
    const topics = ['obs.status', 'obs.state', 'presentation.queue', 'stream.status', 'bgm.player', 'counter.values', 'counter.marathon', 'screen.break', 'screen.warnings', 'screen.appearance', 'leaderboard.update', 'chat.text_size', 'chat.message']
    socket.onmessage!({ data: JSON.stringify({ seq: 1, type: 'snapshot', payload: Object.fromEntries(topics.map((k) => [k, []])) }) })
    socket.onmessage!({ data: JSON.stringify({ seq: 2, type: 'obs.state', payload: { scene: 'Game', scenes: [{ name: 'Game', items: [] }], inputs: [{ name: 'Mic', muted: false, global: true }], streaming: false, recording: false } }) })
    await pickStage('after')
    await flushPromises()
    const tile = document.querySelector('[data-drawer-tile="mixer"]')!.closest('.tile-wrap')!
    expect(tile.textContent).toContain(t('mixer.tile.on'))
    expect(tile.textContent).toContain(t('mixer.tile.mute'))
  })
})

describe('HomeView stats tile after the stream', () => {
  afterEach(() => Object.assign(useAfterNumbers(), { mode: 'show', revealed: [] }))

  it('masks its counts, folded and opened, when the numbers are masked', async () => {
    const data: Record<string, unknown> = {
      ...responses,
      '/api/admin/home-tiles': ['stats'],
      '/api/admin/stream/summary': {
        startedAt: '2026-11-21T10:00:00.000Z', endedAt: '2026-11-21T12:00:00.000Z', endedSinceLaunch: true, peakViewers: 42,
        follows: 3, subs: 47, bits: 150, raids: 1, redemptions: 58, chatMessages: 10, firstChats: 2,
      },
    }
    vi.mocked(client.GET).mockImplementation(((path: string) => ok(data[path])) as never)
    Object.assign(useAfterNumbers(), { mode: 'mask', revealed: [] })
    await home()
    // At once, not after the few seconds a tile's line usually waits.
    await pickStage('after')
    const tile = () => document.querySelector('[data-expand="stats"]')!
    expect(tile().textContent).not.toMatch(/58|47/)
    ;(tile() as HTMLButtonElement).click()
    await flushPromises()
    const block = document.getElementById('block-stats')!
    expect(block.textContent).not.toMatch(/58|47|42/)
    expect(block.querySelectorAll('button.masked').length).toBeGreaterThan(0)
  })
})

describe('HomeView tiles that act in place', () => {
  it('count, play and show the break card from the tile without opening its drawer, and report a failure', async () => {
    let socket: { onmessage?: (e: { data: string }) => void } = {}
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
    const data: Record<string, unknown> = {
      ...responses,
      '/api/admin/home-tiles': ['counter', 'sound', 'screen-tools'],
      '/api/admin/checkin/today': { count: 0, recent: [] },
      '/api/admin/modules': ['counter', 'sound', 'screen-tools'].map((id) => ({ id, enabled: true, closed: false })),
      '/api/admin/sounds': { sounds: ['叮咚', '歡呼', '喝水', '拍手'].map((label, i) => ({ id: `s${i}`, label })), slots: {}, volume: 100 },
    }
    vi.mocked(client.GET).mockImplementation(((path: string) => ok(data[path])) as never)
    const post = vi.fn(() => ok(undefined))
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    await home()
    await pickStage('live')
    const event = (type: string, payload: unknown) => [{ id: type, type, version: 1, seq: 1, occurredAt: '', source: 'system', payload }]
    socket.onmessage!({
      data: JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          ...Object.fromEntries(
            ['chat.message', 'chat.removed', 'obs.status', 'obs.state', 'presentation.queue', 'stream.status', 'bgm.player', 'counter.marathon', 'screen.warnings', 'screen.appearance', 'leaderboard.update', 'chat.text_size'].map((k) => [k, []]),
          ),
          'counter.values': event('counter.values', { counters: [{ id: 'c1', name: '死亡次數', value: 12 }] }),
          'screen.break': event('screen.break', { shown: false, mode: 'minutes', minutes: 10, untilTime: '21:00', text: '', endText: '' }),
        },
      }),
    })
    await flushPromises()
    const actions = (tile: string) => document.querySelector(`[data-drawer-tile="${tile}"]`)!.parentElement!.querySelector('.tile-actions')!
    const button = (tile: string, label: string) =>
      [...actions(tile).querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!

    button('counter', '+1').click()
    expect(post).toHaveBeenLastCalledWith('/api/admin/counters/{id}/add', { params: { path: { id: 'c1' } }, body: { delta: 1 } })
    button('counter', '−1').click()
    expect(post).toHaveBeenLastCalledWith('/api/admin/counters/{id}/add', { params: { path: { id: 'c1' } }, body: { delta: -1 } })
    // The first three sounds only.
    expect([...actions('sound').querySelectorAll('button')].map((b) => b.textContent?.trim())).toEqual(['叮咚', '歡呼', '喝水'])
    button('sound', '歡呼').click()
    expect(post).toHaveBeenLastCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 's1' } } })
    actions('screen-tools').querySelector<HTMLInputElement>('input')!.click()
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith('/api/admin/screen-tools/break/show', { body: { minutes: 10 } })
    expect(drawer.current.value).toBeNull()

    post.mockImplementationOnce(() => Promise.reject(new Error('down')))
    button('counter', '+1').click()
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledTimes(1)
  })
})

describe('HomeView tiles', () => {
  it('show a big count with its unit and the latest on the line', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(
        {
          ...responses,
          '/api/admin/modules': ['sound'].map((id) => ({ id, enabled: true, closed: false })),
          '/api/admin/sounds': { sounds: [{ id: 'a', label: '叮咚' }, { id: 'b', label: '歡呼' }], slots: {}, volume: 100 },
        }[path],
      )) as never)
    await home()
    const text = (id: string) => document.querySelector(`[data-drawer-tile="${id}"]`)?.textContent?.replace(/\s+/g, '')
    expect(text('sound')).toContain(`2${t('home.units.sound')}叮咚、歡呼`.replace(/\s+/g, ''))
  })

  it('name a tile that is no module after its page', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok({ ...responses, '/api/admin/home-tiles': ['rehearsal'] }[path])) as never)
    await home()
    expect(document.querySelector('[data-drawer-tile="rehearsal"]')?.textContent).toContain(t('pages.rehearsal.name'))
  })
})

describe('HomeView skip link', () => {
  const skip = () => document.querySelector<HTMLAnchorElement>('a.skip-link')!

  it('focuses the first tile', async () => {
    await home()
    skip().click()
    expect(document.activeElement).toBe(document.querySelector('[data-drawer-tile="chat"]'))
  })

  it('leaves the open drawer alone when a grey tile is clicked', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(path === '/api/admin/home-tiles' ? ['chat', UNBUILT_TILES[0]!.id] : responses[path])) as never)
    await home()
    await drawer.open('chat')
    ;(document.querySelector('.tile.off') as HTMLElement).click()
    await flushPromises()
    expect(drawer.current.value).toBe('chat')
  })

  it('focuses "Arrange tiles" on an emptied home', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(path === '/api/admin/home-tiles' ? [] : responses[path])) as never)
    await home()
    skip().click()
    expect(document.activeElement).toBe(document.querySelector('[data-drawer-arrange]'))
  })

  it('focuses the tiles section while there is no tile yet: loading, then load failed', async () => {
    let fail!: () => void
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      path === '/api/admin/modules'
        ? new Promise((resolve) => {
            fail = () => resolve({ error: { code: 'x' }, response: new Response(null, { status: 500 }) })
          })
        : ok(responses[path])) as never)
    await home()
    skip().click()
    expect(document.activeElement).toBe(document.getElementById('tiles'))

    ;(document.activeElement as HTMLElement).blur()
    fail()
    await flushPromises()
    expect(document.getElementById('tiles')!.textContent).toContain(t('home.loadFailed'))
    skip().click()
    expect(document.activeElement).toBe(document.getElementById('tiles'))
  })
})

describe('HomeView update notice', () => {
  it('moves focus to the first tile once dismissed, since its button is gone', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok(path === '/api/admin/update' ? { current: '0.1.0', available: '0.2.0', check: true } : responses[path])) as never)
    await home()
    const dismiss = [...document.querySelectorAll<HTMLButtonElement>('.update button')].find(
      (b) => b.textContent === i18n.global.t('update.dismiss'),
    )!
    dismiss.focus()
    dismiss.click()
    await flushPromises()
    expect(document.querySelector('.update')).toBeNull()
    expect(document.activeElement).toBe(document.querySelector('[data-drawer-tile="chat"]'))
  })
})

describe('HomeView arrange mode', () => {
  it('enters only after the open drawer submitted and closed, and moves focus between ✎ and Done', async () => {
    await home()
    await drawer.open('chat')
    const removeFailing = onDrawerSubmit(async () => false)
    pencil()!.click()
    await flushPromises()
    expect(done()).toBeUndefined()
    expect(drawer.current.value).toBe('chat')

    removeFailing()
    pencil()!.click()
    await flushPromises()
    expect(drawer.current.value).toBeNull()
    expect(document.activeElement).toBe(done())
    expect(document.querySelector('[data-tile="chat"]')).not.toBeNull()

    ;(done() as HTMLButtonElement).click()
    await flushPromises()
    expect(document.activeElement).toBe(pencil())
    expect(document.querySelector('[data-drawer-tile="chat"]')).not.toBeNull()
  })

  it('Esc closes a drawer first, is left to an open dialog, then leaves arrange mode', async () => {
    await home()
    pencil()!.click()
    await flushPromises()
    await drawer.open('connections')
    await esc()
    expect(drawer.current.value).toBeNull()
    expect(done()).toBeDefined()

    const dialog = document.body.appendChild(document.createElement('dialog'))
    dialog.setAttribute('open', '')
    await esc()
    expect(done()).toBeDefined()
    dialog.remove()

    await esc()
    expect(done()).toBeUndefined()
    expect(document.activeElement).toBe(pencil())
  })

  it('a save that fails after leaving arrange mode still restores the tiles and reports the error', async () => {
    let fail!: () => void
    put.mockImplementation(
      () =>
        new Promise((resolve) => {
          fail = () =>
            resolve({ error: { code: 'settings.save_failed' }, response: new Response(null, { status: 500 }) })
        }),
    )
    await home()
    pencil()!.click()
    await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-tile="chat"] [data-action="remove"]')!.click()
    await flushPromises()
    // The tiles other than chat stay in the order, hidden while their modules are off.
    // 開播前's home, without the chat tile; the other stages are not sent.
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', {
      params: { query: { stage: 'before' } },
      body: ['checklist', 'scene', 'mixer', 'screen-tools', 'bgm', 'sound', 'alerts'],
    })
    ;(done() as HTMLButtonElement).click()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).toBeNull()

    fail()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).not.toBeNull()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.settings.save_failed') }))
  })
})
