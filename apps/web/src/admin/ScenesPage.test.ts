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
      'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'chat.text_size': [],
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
