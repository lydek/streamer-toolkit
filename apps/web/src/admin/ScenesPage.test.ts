// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import Menu from 'primevue/menu'
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

  it('in studio mode edits the preview scene: its items, placements and changes, not the scene on air', async () => {
    const get = vi.fn((_path: string, init?: { params?: { query?: { scene?: string } } }) =>
      Promise.resolve({
        data: { canvas: { width: 1920, height: 1080 }, items: init?.params?.query?.scene === 'Opening' ? [{ item: 9, x: 0, y: 0, align: 'top_left' }] : [] },
        response: new Response(null, { status: 200 }),
      }),
    )
    vi.spyOn(client, 'GET').mockImplementation(get as never)
    const wrapper = await page()
    const live = connected()
    live.scenes[0].items = [item(9, 'Logo', 'image_source')] as never[]
    snapshot({ ...live, studioMode: true, preview: 'Opening', transitions: [], transition: null })
    await flushPromises()
    expect(wrapper.get('#scene-items').text()).toBe(t('scene.items', { scene: 'Opening' }))
    expect(wrapper.get('.items-head .stag').text()).toBe(t('scene.preview'))
    expect(get).toHaveBeenLastCalledWith('/api/admin/obs/placements', { params: { query: { scene: 'Opening' } } })
    await wrapper.get('tbody tr input[role="switch"]').trigger('change')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', expect.objectContaining({ body: expect.objectContaining({ scene: 'Opening', item: 9 }) }))
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
    // The row shows where it sits; its name opens the editor (one at a time) with the fields.
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toBe('40, 30')
    const opener = (name: string) => wrapper.findAll('.item-name').find((b) => b.text() === name)!
    await opener('Capture').trigger('click')
    expect(wrapper.get('tr.editor').text()).toContain(t('scene.outside'))
    await opener('Chat overlay').trigger('click')
    expect(wrapper.findAll('tr.editor')).toHaveLength(1)
    expect(wrapper.get('tr.editor').text()).not.toContain(t('scene.outside'))
    const x = wrapper.get(`input[aria-label="${t('scene.x', { item: 'Chat overlay' })}"]`)
    await x.setValue('100')
    await x.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 100, y: 30, align: 'top_left' } },
    })
    // The editor stays as it was (no rebuild), and the row's summary follows.
    expect(wrapper.find('tr.editor').exists()).toBe(true)
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toBe('100, 30')
    // OBS sends no event for a moved item: the next change keeps the X just saved.
    const y = wrapper.get(`input[aria-label="${t('scene.y', { item: 'Chat overlay' })}"]`)
    await y.setValue('80')
    await y.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 100, y: 80, align: 'top_left' } },
    })
  })

  it('moves an item to the front, the back, a typed place or below another, and by dragging', async () => {
    const wrapper = await page()
    const four = ['A', 'B', 'C', 'D'].map((n, i) => item(10 + i, n, 'browser_source'))
    snapshot(connected('Game', four))
    await flushPromises()
    // The row menu's entries, as PrimeVue's popup Menu gets them.
    const pick = async (name: string, label: string) => {
      await wrapper.get(`button[aria-label="${t('scene.moreMoves', { item: name })}"]`).trigger('click')
      const menu = wrapper.findAllComponents(Menu).find((m) => m.props('model')?.some((i) => i.label === label))!
      menu.props('model')!.find((i) => i.label === label)!.command!({} as never)
      await flushPromises()
    }
    // C to the front, B to the back (front to back from 0: B is 1, C is 2).
    await pick('C', t('scene.toFront'))
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 12, position: 0 } })
    await pick('B', t('scene.toBack'))
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 11, position: 3 } })
    // A typed place is counted from 1.
    await wrapper.findAll('.item-name').find((b) => b.text() === 'A')!.trigger('click')
    const index = wrapper.get('input#index-10')
    await index.setValue('3')
    await index.trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 10, position: 2 } })
    // Dragging D onto the top half of B lands it right above B.
    const rows = () => wrapper.findAll('tbody tr[data-item]')
    const b = rows().find((r) => r.attributes('data-item') === '11')!
    vi.spyOn(b.element, 'getBoundingClientRect').mockReturnValue({ top: 0, height: 20 } as DOMRect)
    await rows().find((r) => r.attributes('data-item') === '13')!.trigger('dragstart')
    await b.trigger('dragover', { clientY: 5 })
    await b.trigger('drop')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 13, position: 1 } })
  })

  it('resizes an item keeping its ratio unless 等比例 is turned off, and shows the size in its row', async () => {
    vi.spyOn(client, 'GET').mockImplementation((() =>
      Promise.resolve({
        data: { canvas: { width: 1920, height: 1080 }, items: [{ item: 2, x: 40, y: 30, align: 'top_left', width: 400, height: 600 }] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toBe('40, 30 · 400 × 600')
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    const width = () => wrapper.get(`input[aria-label="${t('scene.width', { item: 'Chat overlay' })}"]`)
    await width().setValue('200')
    await width().trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, size: { width: 200, height: 300 } } })
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toBe('40, 30 · 200 × 300')
    await wrapper.findAll('button').find((b) => b.text() === t('scene.ratio'))!.trigger('click')
    await width().setValue('250')
    await width().trigger('blur')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, size: { width: 250, height: 300 } } })
  })

  it('keeps a locked item where it is: its fields are off, its order still changes, and 解鎖 unlocks it', async () => {
    vi.spyOn(client, 'GET').mockImplementation((() =>
      Promise.resolve({
        data: { canvas: { width: 1920, height: 1080 }, items: [{ item: 2, x: 40, y: 30, align: 'top_left', width: 400, height: 600 }] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected('Game', [item(2, 'Chat overlay', 'browser_source', true, true), item(1, 'Capture', 'spout_capture')]))
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    const editor = wrapper.get('tr.editor')
    expect(editor.text()).toContain(t('scene.lockedNote'))
    for (const label of [t('scene.x', { item: 'Chat overlay' }), t('scene.width', { item: 'Chat overlay' })])
      expect(editor.get(`input[aria-label="${label}"]`).attributes('disabled')).toBeDefined()
    expect(editor.get('input#index-2').attributes('disabled')).toBeUndefined()
    await editor.findAll('button').find((b) => b.text() === t('scene.unlock'))!.trigger('click')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, locked: false } })
  })

  it('aligns an item once below another, centred, with an offset, keeping its own align point', async () => {
    vi.spyOn(client, 'GET').mockImplementation((() =>
      Promise.resolve({
        data: {
          canvas: { width: 1920, height: 1080 },
          items: [
            { item: 2, x: 0, y: 0, align: 'top_left', width: 100, height: 50 },
            { item: 1, x: 100, y: 200, align: 'top_left', width: 300, height: 100 },
          ],
        },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    const vm = wrapper.vm as unknown as { rel: { ref: string; side: string; cross: string; ox: number; oy: number } }
    Object.assign(vm.rel, { ref: '1', side: 'bottom', cross: 'middle', ox: 0, oy: 10 })
    await wrapper.findAll('tr.editor button').find((b) => b.text() === t('scene.apply'))!.trigger('click')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 200, y: 310, align: 'top_left' } },
    })
  })

  it('turns, flips and resets an item under 變形, sending only the part changed', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/transform'
            ? { rotation: 0, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' }
            : { canvas: { width: 1920, height: 1080 }, items: [{ item: 2, x: 40, y: 30, align: 'top_left', width: 400, height: 600 }] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.transform'))!.trigger('click')
    await flushPromises()
    const button = (label: string) => wrapper.findAll('tr.editor button').find((b) => b.text() === label)!
    await button('90°').trigger('click')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, transform: { rotation: 90 } } })
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toContain('90°')
    await button(t('scene.flip_h')).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', { body: { scene: 'Game', item: 2, transform: { flip_h: true } } })
    const placementReads = () => vi.mocked(client.GET).mock.calls.filter((c) => c[0] === '/api/admin/obs/placements').length
    const before = placementReads()
    await button(t('scene.resetTransform')).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, transform: { rotation: 0, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' } },
    })
    // Crop and bounding box change the size on the canvas: the positions and sizes are read again.
    expect(placementReads()).toBe(before + 1)
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
