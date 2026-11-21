// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { useDock } from './dock'
import { onDrawerSubmit, useDrawer } from './drawer'
import { UNBUILT_TILES } from './home'
import HomeView from './HomeView.vue'
import { testRouter } from './testRouter'

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
const pencil = () => [...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes('編輯首頁'))
const done = () => [...document.querySelectorAll('header button')].find((b) => b.textContent?.includes('完成'))
const pickStage = async (name: string) => {
  ;[...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.trim() === name)!.click()
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
    expect(headings()).toContain('開播前檢查')
    expect(document.querySelector('#tiles li.size-w h2')?.textContent).toBe('開播前檢查')
    expect(document.querySelector('[data-expand="queue"]')).toBeNull()
    // 直播中 has its own home: the queue as a small tile, no checklist; 待處理 has no items yet.
    await pickStage('直播中')
    expect(headings()).not.toContain('開播前檢查')
    expect(headings()).not.toContain('待處理')
    expect(document.querySelector('[data-expand="queue"]')?.getAttribute('aria-expanded')).toBe('false')
    await pickStage('下播後')
    expect(headings()).toContain('本場摘要')
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
    await pickStage('直播中')
    document.querySelector<HTMLButtonElement>('[data-tile="chat"] [data-action="remove"]')!.click()
    await flushPromises()
    expect(put).toHaveBeenCalledTimes(1)
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', {
      params: { query: { stage: 'live' } },
      body: ['pending', 'stats', 'feed', 'queue', 'alerts', 'redemption', 'checkin', 'sound', 'scene', 'bgm', 'counter', 'screen-tools'],
    })
    // The other stages keep their own orders: 開播前 still has its chat tile, 下播後 stays empty.
    await pickStage('開播前')
    expect(tileIds()).toEqual(['chat'])
    await pickStage('下播後')
    expect(tileIds()).toEqual([])
  })
})

describe('HomeView blocks that open in place', () => {
  it('opens a small tile into its block and back, with focus kept on the same place, and saves nothing', async () => {
    await home()
    await pickStage('直播中')
    const toggle = () => document.querySelector<HTMLButtonElement>('#tiles [data-expand="queue"]')!
    toggle().click()
    await flushPromises()
    expect(document.getElementById('block-queue')?.closest('li')?.classList).toContain('size-m')
    expect(toggle().getAttribute('aria-expanded')).toBe('true')
    expect(document.activeElement).toBe(toggle())
    toggle().click()
    await flushPromises()
    expect(document.getElementById('block-queue')).toBeNull()
    expect(toggle().getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(toggle())

    document.querySelector<HTMLButtonElement>('#tiles [data-expand="feed"]')!.click()
    await flushPromises()
    expect(document.getElementById('block-feed')?.closest('li')?.classList).toContain('size-l')
    // The feed loads with the live messages, which this test does not send.
    expect(document.getElementById('block-feed')?.textContent).toContain('載入中')
    expect(put).not.toHaveBeenCalled()
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
    expect(text('sound')).toContain('2個音效叮咚、歡呼')
  })

  it('name a tile that is no module after its page', async () => {
    vi.mocked(client.GET).mockImplementation(((path: string) =>
      ok({ ...responses, '/api/admin/home-tiles': ['rehearsal'] }[path])) as never)
    await home()
    expect(document.querySelector('[data-drawer-tile="rehearsal"]')?.textContent).toContain('演練')
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
    expect(document.getElementById('tiles')!.textContent).toContain('載入失敗')
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
      body: ['checklist', 'scene', 'screen-tools', 'bgm', 'sound', 'alerts'],
    })
    ;(done() as HTMLButtonElement).click()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).toBeNull()

    fail()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).not.toBeNull()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: '設定儲存失敗。' }))
  })
})
