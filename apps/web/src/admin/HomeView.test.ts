// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { useDock } from './dock'
import { onDrawerSubmit, useDrawer } from './drawer'
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
})

describe('HomeView skip link', () => {
  const skip = () => document.querySelector<HTMLAnchorElement>('a.skip-link')!

  it('focuses the first tile', async () => {
    await home()
    skip().click()
    expect(document.activeElement).toBe(document.querySelector('[data-drawer-tile="chat"]'))
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
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', {
      body: ['alerts', 'feed', 'redemption', 'checkin', 'sound', 'scene', 'bgm', 'counter'],
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
