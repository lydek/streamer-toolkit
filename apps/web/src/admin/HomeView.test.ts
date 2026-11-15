// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { onDrawerSubmit, useDrawer } from './drawer'
import HomeView from './HomeView.vue'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data: unknown) => Promise.resolve({ data, response: new Response(null, { status: 200 }) })
const responses: Record<string, unknown> = {
  '/api/admin/twitch': { client_id: '', status: { state: 'signed_out' } },
  '/api/admin/home-tiles': null,
  '/api/admin/modules': [{ id: 'chat', enabled: true }],
  '/api/admin/templates/chat': { error: null },
}
let put: ReturnType<typeof vi.fn>
const drawer = useDrawer()

async function home() {
  const wrapper = mount(HomeView, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}
const pencil = () => document.querySelector<HTMLButtonElement>('header [aria-label="整理功能磚"]')
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
    expect(put).toHaveBeenCalledWith('/api/admin/home-tiles', { body: [] })
    ;(done() as HTMLButtonElement).click()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).toBeNull()

    fail()
    await flushPromises()
    expect(document.querySelector('[data-drawer-tile="chat"]')).not.toBeNull()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: '設定儲存失敗。' }))
  })
})
