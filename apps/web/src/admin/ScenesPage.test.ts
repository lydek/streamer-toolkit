// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import ScenesPage from './ScenesPage.vue'
import { kindKey } from './scenes'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t
const ok = () => Promise.resolve({ data: undefined, response: new Response(null, { status: 204 }) })
const fail = (code: string, status: number) =>
  Promise.resolve({ error: { code }, response: new Response(null, { status }) })

// The page's `/ws/overlay` socket; tests push messages through it.
let socket: { onmessage?: (e: { data: string }) => void }
let seq = 0
const send = (msg: object) => socket.onmessage!({ data: JSON.stringify({ seq: ++seq, ...msg }) })
const empty = { scene: null, scenes: [], inputs: [], streaming: false, recording: false }
const item = (id: number, source: string, kind: string, enabled = true, locked = false) => ({
  id,
  source,
  kind,
  enabled,
  locked,
})
const connected = (live = 'Game', items = [item(2, 'Chat overlay', 'browser_source'), item(1, 'Capture', 'spout_capture', false)]) => ({
  scene: live,
  scenes: [
    { name: 'Opening', items: [] },
    { name: 'Game', items },
  ],
  inputs: [],
  streaming: false,
  recording: false,
})
function snapshot(state: object) {
  send({
    type: 'snapshot',
    payload: {
      'obs.status': [],
      'obs.state': [{ payload: state }],
      'presentation.queue': [],
      'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'chat.text_size': [],
      'chat.message': [],
    },
  })
}

let put: ReturnType<typeof vi.fn>

async function page() {
  const wrapper = mount(ScenesPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  seq = 0
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
  put = vi.fn(ok)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  toastAdd.mockReset()
})

enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('ScenesPage', () => {
  it('in studio mode puts a picked scene in the preview, and transitions with the transition picked', async () => {
    const post = vi.fn(ok)
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const wrapper = await page()
    snapshot({ ...connected(), studioMode: true, preview: 'Opening', transitions: ['Cut', 'Fade'], transition: 'Fade' })
    await flushPromises()
    expect(wrapper.text()).toContain(t('scene.studioNote'))
    const buttons = wrapper.findAll('ul button')
    expect(buttons.map((b) => b.attributes('aria-pressed'))).toEqual(['true', 'false'])
    await buttons[1].trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/preview', { body: { scene: 'Game' } })
    await wrapper.findAll('button').find((b) => b.text() === t('scene.transition'))!.trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/obs/transition', { body: { transition: 'Fade' } })
  })

  it('shows and changes where an item sits, and says when it is off the canvas', async () => {
    vi.spyOn(client, 'GET').mockImplementation((() =>
      Promise.resolve({
        data: {
          canvas: { width: 1920, height: 1080 },
          items: [
            { item: 2, x: 40, y: 30, align: 'top_left' },
            { item: 1, x: 2000, y: 10, align: 'center' },
          ],
        },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const rows = wrapper.findAll('tbody tr')
    expect(rows[0].text()).not.toContain(t('scene.outside'))
    expect(rows[1].text()).toContain(t('scene.outside'))
    const x = rows[0].get(`input[aria-label="${t('scene.x', { item: 'Chat overlay' })}"]`)
    await x.setValue('100')
    await x.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 100, y: 30, align: 'top_left' } },
    })
    // OBS sends no event for a moved item: the next change keeps the X just saved.
    const y = rows[0].get(`input[aria-label="${t('scene.y', { item: 'Chat overlay' })}"]`)
    await y.setValue('80')
    await y.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 100, y: 80, align: 'top_left' } },
    })
  })

  it('moves an item forward or back, counted front to back, and keeps the ends from going further', async () => {
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const forward = (name: string) => wrapper.get(`button[aria-label="${t('scene.forward', { item: name })}"]`)
    const back = (name: string) => wrapper.get(`button[aria-label="${t('scene.backward', { item: name })}"]`)
    expect(forward('Chat overlay').attributes('disabled')).toBeDefined()
    expect(back('Capture').attributes('disabled')).toBeDefined()
    await back('Chat overlay').trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, position: 1 } })
  })

  it('says OBS is not connected and links to its settings', async () => {
    const wrapper = await page()
    snapshot(empty)
    await flushPromises()
    expect(wrapper.text()).toContain(t('scene.notConnected'))
    expect(wrapper.get('a').attributes('href')).toContain('/settings/connections#obs')
    expect(wrapper.find('table').exists()).toBe(false)
  })

  it('lists scenes with the one on air pressed, and switches through the Host', async () => {
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const buttons = wrapper.findAll('ul button')
    expect(buttons.map((b) => [b.text(), b.attributes('aria-pressed')])).toEqual([
      ['Opening', 'false'],
      ['Game', 'true'],
    ])

    await buttons[0].trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/scene', { body: { scene: 'Opening' } })
    // The view follows OBS, not the click.
    expect(wrapper.get('[aria-pressed="true"]').text()).toBe('Game')
    send({ type: 'obs.state', payload: connected('Opening') })
    await flushPromises()
    expect(wrapper.get('[aria-pressed="true"]').text()).toBe('Opening')
    expect(wrapper.text()).toContain(t('scene.noItems'))
  })

  it('shows the items front to back and asks OBS to change one', async () => {
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const rows = wrapper.findAll('tbody tr')
    // A known kind reads as its name; another shows as OBS names it.
    expect(rows.map((r) => [r.get('th').text(), r.findAll('td')[0].text()])).toEqual([
      ['Chat overlay', t('scene.kinds.browser')],
      ['Capture', 'spout_capture'],
    ])

    const show = () => wrapper.get(`[aria-label="${t('scene.showItem', { item: 'Capture' })}"]`)
    const checked = () => show().element.closest('.p-toggleswitch')!.classList.contains('p-toggleswitch-checked')
    expect(checked()).toBe(false)
    await show().trigger('change')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 1, enabled: true } })
    send({ type: 'obs.state', payload: connected('Game', [item(2, 'Chat overlay', 'browser_source'), item(1, 'Capture', 'spout_capture', true)]) })
    await flushPromises()
    expect(checked()).toBe(true)

    await wrapper.get(`[aria-label="${t('scene.lockItem', { item: 'Chat overlay' })}"]`).trigger('change')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, locked: true } })
  })

  it('reports a refused command and puts the switch back where OBS has it', async () => {
    put.mockImplementation(() => fail('obs.not_found', 404))
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('ul button')[0].trigger('click')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('errors.obs.not_found') }))

    const show = () => wrapper.get(`[aria-label="${t('scene.showItem', { item: 'Capture' })}"]`)
    await show().trigger('change')
    await flushPromises()
    expect(toastAdd).toHaveBeenCalledTimes(2)
    expect(show().element.closest('.p-toggleswitch')!.classList).not.toContain('p-toggleswitch-checked')
  })
})

describe('kindKey', () => {
  it('shares one name across versioned kinds and leaves unknown ones alone', () => {
    expect(kindKey('text_gdiplus_v3')).toBe('scene.kinds.text')
    expect(kindKey('scene')).toBe('scene.kinds.scene')
    expect(kindKey('obs-shaderfilter')).toBeNull()
  })
})
