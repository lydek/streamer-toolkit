// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import InputNumber from 'primevue/inputnumber'
import Menu from 'primevue/menu'
import Select from 'primevue/select'
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
      'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [],
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

  it('shows a group with its items under it, folds it, and changes an item inside it through the group', async () => {
    const get = vi.fn((_path: string, init?: { params?: { query?: { scene?: string } } }) =>
      Promise.resolve({
        data: {
          canvas: { width: 1920, height: 1080 },
          // The group's item has the same ID as the scene's capture; each keeps its own place.
          items: init?.params?.query?.scene === 'Cam set' ? [{ item: 1, x: 5, y: 6, align: 'top_left' }] : [{ item: 1, x: 70, y: 80, align: 'top_left' }],
        },
        response: new Response(null, { status: 200 }),
      }),
    )
    vi.spyOn(client, 'GET').mockImplementation(get as never)
    const wrapper = await page()
    const group = { ...item(3, 'Cam set', 'group', false), children: [item(1, 'Webcam', 'dshow_input'), item(2, 'Frame', 'image_source')] }
    snapshot(connected('Game', [group, item(1, 'Capture', 'spout_capture')] as never))
    await flushPromises()
    const rows = () => wrapper.findAll('tbody tr').filter((r) => !r.classes('editor'))
    expect(rows().map((r) => [r.get('.num').text(), r.get('.item-name').text()])).toEqual([
      ['1', 'Cam set'],
      ['1.1', 'Webcam'],
      ['1.2', 'Frame'],
      ['2', 'Capture'],
    ])
    // The group is hidden: its items fade.
    expect(rows()[1].classes()).toEqual(expect.arrayContaining(['child', 'faded']))
    expect(get).toHaveBeenCalledWith('/api/admin/obs/placements', { params: { query: { scene: 'Cam set' } } })
    expect(rows()[1].get('.geo').text()).toBe('5, 6')
    expect(rows()[3].get('.geo').text()).toBe('70, 80')

    await wrapper.get(`[aria-label="${t('scene.showItem', { item: 'Webcam' })}"]`).trigger('change')
    await wrapper.get(`[aria-label="${t('scene.backward', { item: 'Webcam' })}"]`).trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', { body: { scene: 'Cam set', item: 1, enabled: false } })
    // Moved inside the group: position 1 of its two items.
    expect(put).toHaveBeenCalledWith('/api/admin/obs/item', { body: { scene: 'Cam set', item: 1, position: 1 } })
    // The last item of the group cannot go further back, even with an item below the group.
    expect(wrapper.get(`[aria-label="${t('scene.backward', { item: 'Frame' })}"]`).attributes('disabled')).toBeDefined()

    await wrapper.get(`[aria-label="${t('scene.foldGroup', { group: 'Cam set' })}"]`).trigger('click')
    expect(rows().map((r) => r.get('.item-name').text())).toEqual(['Cam set', 'Capture'])
  })

  it('reads positions again when scenes or items change, not when only a fader moves', async () => {
    const get = vi.fn(() => Promise.resolve({ data: { canvas: { width: 1920, height: 1080 }, items: [] }, response: new Response(null, { status: 200 }) }))
    vi.spyOn(client, 'GET').mockImplementation(get as never)
    await page()
    snapshot(connected())
    await flushPromises()
    const reads = () => get.mock.calls.filter((c) => (c as unknown[])[0] === '/api/admin/obs/placements').length
    const before = reads()
    send({ type: 'obs.state', payload: { ...connected(), inputs: [{ name: 'Mic', muted: false, volumeDb: -12 }] } })
    await flushPromises()
    expect(reads()).toBe(before)
    send({ type: 'obs.state', payload: connected('Game', [item(2, 'Chat overlay', 'browser_source')]) })
    await flushPromises()
    expect(reads()).toBe(before + 1)
  })

  it('keeps the position read for another scene when an older move answers late', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((_path: string, init?: { params?: { query?: { scene?: string } } }) =>
      Promise.resolve({
        data: {
          canvas: { width: 1920, height: 1080 },
          items: [init?.params?.query?.scene === 'Opening' ? { item: 2, x: 500, y: 600, align: 'top_left' } : { item: 2, x: 40, y: 30, align: 'top_left' }],
        },
        response: new Response(null, { status: 200 }),
      })) as never)
    let answer!: () => void
    put.mockImplementationOnce(() => new Promise((r) => (answer = () => r({ data: undefined, response: new Response(null, { status: 204 }) }))))
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    const x = wrapper.get(`input[aria-label="${t('scene.x', { item: 'Chat overlay' })}"]`)
    await x.setValue('100')
    await x.trigger('blur')
    // Another scene with an item 2 goes on air before the move is answered.
    const other = connected('Opening')
    other.scenes[0].items = [item(2, 'Logo', 'image_source')] as never[]
    send({ type: 'obs.state', payload: other })
    await flushPromises()
    answer()
    await flushPromises()
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toBe('500, 600')
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
    let rotation = 0
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/transform'
            ? { rotation, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' }
            : {
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
    // Turned: refused with the reason, nothing sent.
    rotation = 90
    put.mockClear()
    await wrapper.findAll('tr.editor button').find((b) => b.text() === t('scene.apply'))!.trigger('click')
    await flushPromises()
    expect(put).not.toHaveBeenCalled()
    expect(toastAdd).toHaveBeenLastCalledWith(expect.objectContaining({ summary: t('scene.relativeTurned') }))
  })

  it('aligns only to the reference chosen for this item, and never to a reference that is gone', async () => {
    // The transform reads wait until released, as a slow OBS would.
    let held: (() => void)[] = []
    let rotation = 0
    const release = () => {
      held.forEach((answer) => answer())
      held = []
    }
    vi.spyOn(client, 'GET').mockImplementation(((path: string) => {
      const answer = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
      if (path === '/api/admin/obs/transform')
        return new Promise((resolve) => {
          held.push(() => resolve(answer({ rotation, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' })))
        })
      return Promise.resolve(
        answer({
          canvas: { width: 1920, height: 1080 },
          items: [
            { item: 2, x: 0, y: 0, align: 'top_left', width: 100, height: 50 },
            { item: 1, x: 100, y: 200, align: 'top_left', width: 300, height: 100 },
          ],
        }),
      )
    }) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const vm = wrapper.vm as unknown as { rel: { ref: string; side: string } }
    const openItem = async (name: string) => {
      await wrapper.findAll('.item-name').find((b) => b.text() === name)!.trigger('click')
      await flushPromises()
    }
    const apply = async () => {
      await wrapper.findAll('tr.editor button').find((b) => b.text() === t('scene.apply'))!.trigger('click')
      await flushPromises()
    }

    // Chosen for Chat overlay, then Capture opened: Capture starts from the whole screen.
    await openItem('Chat overlay')
    Object.assign(vm.rel, { ref: '1', side: 'bottom' })
    await openItem('Capture')
    expect(vm.rel.ref).toBe('canvas')

    // A reference not in the list: said so, nothing moves.
    await openItem('Chat overlay')
    vm.rel.ref = '99'
    put.mockClear()
    await apply()
    expect(put).not.toHaveBeenCalled()
    expect(toastAdd).toHaveBeenLastCalledWith(expect.objectContaining({ summary: t('scene.relativeMissing') }))

    // Another item opened while the reads were out, even switching back before they answer: nothing moves.
    vm.rel.ref = '1'
    await apply()
    await openItem('Capture')
    release()
    await flushPromises()
    expect(put).not.toHaveBeenCalled()
    await openItem('Chat overlay')
    vm.rel.ref = '1'
    await apply()
    await openItem('Capture')
    await openItem('Chat overlay')
    release()
    await flushPromises()
    expect(put).not.toHaveBeenCalled()

    // A turned item's answer arriving after another item opened: nothing said, nothing moves.
    rotation = 90
    toastAdd.mockClear()
    vm.rel.ref = '1'
    await apply()
    await openItem('Capture')
    release()
    await flushPromises()
    expect(toastAdd).not.toHaveBeenCalled()
    expect(put).not.toHaveBeenCalled()
    rotation = 0
    await openItem('Chat overlay')

    // Applied twice before the reads answer: only the latest choice moves the item.
    vm.rel.ref = '1'
    await apply()
    vm.rel.ref = 'canvas'
    await apply()
    release()
    await flushPromises()
    expect(put).toHaveBeenCalledTimes(1)
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/item', {
      body: { scene: 'Game', item: 2, placement: { x: 910, y: 1030, align: 'top_left' } },
    })
    put.mockClear()

    // The reference removed while the reads were out: said so, nothing moves (Chat overlay is open).
    vm.rel.ref = '1'
    toastAdd.mockClear()
    await apply()
    snapshot(connected('Game', [item(2, 'Chat overlay', 'browser_source')]))
    await flushPromises()
    release()
    await flushPromises()
    expect(put).not.toHaveBeenCalled()
    expect(toastAdd).toHaveBeenLastCalledWith(expect.objectContaining({ summary: t('scene.relativeMissing') }))
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

  it('keeps the 變形 of the scene now edited when an older read answers late', async () => {
    let lateAnswer!: () => void
    const transform = (rotation: number) => ({ rotation, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' })
    const reply = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
    vi.spyOn(client, 'GET').mockImplementation(((path: string, init?: { params?: { query?: { scene?: string } } }) => {
      if (path !== '/api/admin/obs/transform')
        return Promise.resolve(reply({ canvas: { width: 1920, height: 1080 }, items: [{ item: 2, x: 0, y: 0, align: 'top_left', width: 400, height: 600 }] }))
      if (init?.params?.query?.scene === 'Game') return new Promise((r) => (lateAnswer = () => r(reply(transform(0)))))
      return Promise.resolve(reply(transform(90)))
    }) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.transform'))!.trigger('click')
    await flushPromises()
    // Another scene goes on air whose item 2 is turned 90°; then the first read answers.
    const other = connected('Opening')
    other.scenes[0].items = [item(2, 'Logo', 'image_source')] as never[]
    send({ type: 'obs.state', payload: other })
    await flushPromises()
    lateAnswer()
    await flushPromises()
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).toContain('90°')
  })

  it('drops a late 變形 read once another scene is edited, even without a 變形 tab there', async () => {
    let lateAnswer!: () => void
    const transform = { rotation: 45, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' }
    const reply = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      path === '/api/admin/obs/transform'
        ? new Promise((r) => (lateAnswer = () => r(reply(transform))))
        : Promise.resolve(reply({ canvas: { width: 1920, height: 1080 }, items: [{ item: 2, x: 0, y: 0, align: 'top_left', width: 400, height: 600 }] }))) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Chat overlay')!.trigger('click')
    await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.transform'))!.trigger('click')
    await flushPromises()
    // The other scene's item 2 is a group: no 變形 tab, so nothing is read for it.
    const other = connected('Opening')
    other.scenes[0].items = [item(2, 'Cam set', 'group')] as never[]
    send({ type: 'obs.state', payload: other })
    await flushPromises()
    lateAnswer()
    await flushPromises()
    expect(wrapper.findAll('tbody tr')[0].get('.geo').text()).not.toContain('45°')
  })

  it("reads a source's settings under 來源設定, saying where only OBS can change them", async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string, init?: { params?: { query?: { input?: string } } }) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/source'
            ? init?.params?.query?.input === 'Capture'
              ? { family: 'other', fields: {}, url_hidden: false }
              : { family: 'browser', fields: { width: 400 }, url_hidden: true }
            : { canvas: { width: 1920, height: 1080 }, items: [] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const openSource = async (name: string) => {
      await wrapper.findAll('.item-name').find((b) => b.text() === name)!.trigger('click')
      await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.source'))!.trigger('click')
      await flushPromises()
    }
    await openSource('Chat overlay')
    expect(wrapper.get('tr.editor').text()).toContain(t('scene.sourceShared', { source: 'Chat overlay' }))
    expect(wrapper.get('tr.editor').text()).toContain(t('scene.sourceUrlHidden'))
    await openSource('Capture')
    expect(wrapper.get('tr.editor').text()).toContain(t('scene.sourceInObs'))
    // The open row shows another source now (an item with the same ID): that source's settings are read.
    send({ type: 'obs.state', payload: connected('Game', [item(1, 'Other overlay', 'browser_source')]) })
    await flushPromises()
    expect(client.GET).toHaveBeenCalledWith('/api/admin/obs/source', { params: { query: { input: 'Other overlay' } } })
    expect(wrapper.get('tr.editor').text()).toContain(t('scene.sourceUrlHidden'))
  })

  it("changes a browser source's CSS and refreshes it, and a text source's color, one field at a time", async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string, init?: { params?: { query?: { input?: string } } }) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/source'
            ? init?.params?.query?.input === 'Capture'
              ? { family: 'text', fields: { text: 'hi', color: 0xffffff }, url_hidden: false }
              : { family: 'browser', fields: { url: 'http://127.0.0.1:47810/overlay/chat/', width: 400, height: 600, css: '' }, url_hidden: false }
            : { canvas: { width: 1920, height: 1080 }, items: [] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const post = vi.fn(ok)
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const openSource = async (name: string) => {
      await wrapper.findAll('.item-name').find((b) => b.text() === name)!.trigger('click')
      await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.source'))!.trigger('click')
      await flushPromises()
    }
    await openSource('Chat overlay')
    const placementReads = () => vi.mocked(client.GET).mock.calls.filter((c) => (c[0] as string) === '/api/admin/obs/placements').length
    const reads = placementReads()
    const css = wrapper.get(`textarea[aria-label="${t('scene.src.css')}"]`)
    await css.setValue('body { color: red; }')
    await css.trigger('change')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/source', { body: { input: 'Chat overlay', fields: { css: 'body { color: red; }' } } })
    // A source change may resize it on the canvas: positions and sizes are read again.
    expect(placementReads()).toBeGreaterThan(reads)
    // Refused: the source is read again, so the form shows what OBS has.
    const sourceReads = () => vi.mocked(client.GET).mock.calls.filter((c) => (c[0] as string) === '/api/admin/obs/source').length
    const readsBefore = sourceReads()
    put.mockImplementationOnce(() => Promise.resolve({ error: { code: 'obs.command_failed' }, response: new Response(null, { status: 502 }) }))
    await css.setValue('body { color: blue; }')
    await css.trigger('change')
    await flushPromises()
    expect(sourceReads()).toBe(readsBefore + 1)
    await wrapper.findAll('tr.editor button').find((b) => b.text() === t('scene.src.refresh'))!.trigger('click')
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith('/api/admin/obs/source/refresh', { body: { input: 'Chat overlay' } })
    await openSource('Capture')
    const color = wrapper.get(`input[type="color"]`)
    await color.setValue('#ff0000')
    await color.trigger('change')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/source', { body: { input: 'Capture', fields: { color: 0x0000ff } } })

    // The font's size changed before its face is saved: the size goes out after, with the new face.
    let release!: () => void
    put.mockImplementationOnce(() => new Promise((r) => (release = () => r({ data: undefined, response: new Response(null, { status: 204 }) }))))
    const face = wrapper.get(`input[aria-label="${t('scene.src.fontFace')}"]`)
    // setValue fires the field's change too.
    await face.setValue('Noto Sans TC')
    const size = wrapper.findAllComponents(InputNumber).find((c) => c.props('ariaLabel') === t('scene.src.fontSize'))!
    size.vm.$emit('update:modelValue', 48)
    await flushPromises()
    const fonts = () => put.mock.calls.filter((c) => c[1].body.fields.font).map((c) => c[1].body.fields.font)
    expect(fonts()).toEqual([{ face: 'Noto Sans TC' }])
    release()
    await flushPromises()
    expect(fonts()).toEqual([{ face: 'Noto Sans TC' }, { face: 'Noto Sans TC', size: 48 }])
  })

  it("keeps a change made while the source is read again, rather than the read's older fields", async () => {
    let late!: () => void
    let reads = 0
    vi.spyOn(client, 'GET').mockImplementation(((path: string, init?: { params?: { query?: { input?: string } } }) => {
      const reply = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
      if (path !== '/api/admin/obs/source') return Promise.resolve(reply({ canvas: { width: 1920, height: 1080 }, items: [] }))
      if (init?.params?.query?.input === 'Capture') return Promise.resolve(reply({ family: 'text', fields: { text: 'hi' }, url_hidden: false }))
      const old = reply({ family: 'browser', fields: { url: 'http://127.0.0.1:47810/overlay/chat/', css: 'old' }, url_hidden: false })
      // The second read of the overlay answers only after the change below.
      return ++reads === 2 ? new Promise((r) => (late = () => r(old))) : Promise.resolve(old)
    }) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const openSource = async (name: string) => {
      await wrapper.findAll('.item-name').find((b) => b.text() === name)!.trigger('click')
      await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.source'))!.trigger('click')
      await flushPromises()
    }
    await openSource('Chat overlay')
    await openSource('Capture')
    await openSource('Chat overlay')
    const css = () => wrapper.get(`textarea[aria-label="${t('scene.src.css')}"]`)
    await css().setValue('new')
    await css().trigger('change')
    await flushPromises()
    late()
    await flushPromises()
    expect((css().element as HTMLTextAreaElement).value).toBe('new')
  })

  it('lists capture choices from OBS, showing the window only for a specific window', async () => {
    let mode = 'any'
    const get = vi.fn((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/source'
            ? { family: 'game', fields: { capture_mode: mode }, url_hidden: false }
            : path === '/api/admin/obs/source/options'
              ? [{ name: 'My Game', value: 'game.exe:My Game' }]
              : { canvas: { width: 1920, height: 1080 }, items: [] },
        response: new Response(null, { status: 200 }),
      }),
    )
    vi.spyOn(client, 'GET').mockImplementation(get as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const openSource = async () => {
      await wrapper.findAll('.item-name').find((b) => b.text() === 'Capture')!.trigger('click')
      await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.source'))!.trigger('click')
      await flushPromises()
    }
    await openSource()
    const labels = () => wrapper.findAllComponents(Select).map((c) => c.props('ariaLabel'))
    expect(labels()).toContain(t('scene.src.capture_mode'))
    expect(labels()).not.toContain(t('scene.src.window'))
    // Closing and opening the editor again reads the source again.
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Capture')!.trigger('click')
    mode = 'window'
    await openSource()
    const window = wrapper.findAllComponents(Select).find((c) => c.props('ariaLabel') === t('scene.src.window'))!
    window.vm.$emit('show')
    await flushPromises()
    expect(get).toHaveBeenCalledWith('/api/admin/obs/source/options', { params: { query: { input: 'Capture', property: 'window' } } })
    window.vm.$emit('update:modelValue', 'game.exe:My Game')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/source', { body: { input: 'Capture', fields: { window: 'game.exe:My Game' } } })
  })

  it('switches a video capture to its custom mode when a resolution is picked, or OBS ignores it', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/obs/source'
            ? { family: 'video_capture', fields: { res_type: 0 }, url_hidden: false }
            : { canvas: { width: 1920, height: 1080 }, items: [] },
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    await wrapper.findAll('.item-name').find((b) => b.text() === 'Capture')!.trigger('click')
    await wrapper.findAll('.editor-tabs button').find((b) => b.text() === t('scene.tabs.source'))!.trigger('click')
    await flushPromises()
    wrapper.findAllComponents(Select).find((c) => c.props('ariaLabel') === t('scene.src.resolution'))!.vm.$emit('update:modelValue', '1920x1080')
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/obs/source', { body: { input: 'Capture', fields: { res_type: 1, resolution: '1920x1080' } } })
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
    expect(buttons.map((b) => [b.get('.name').text(), b.attributes('aria-pressed')])).toEqual([
      ['Opening', 'false'],
      ['Game', 'true'],
    ])
    // The scene on air carries the same 直播中 pill as the item table's heading.
    expect(buttons[1].get('.stag.live').text()).toBe(t('scene.onAir'))

    await buttons[0].trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/obs/scene', { body: { scene: 'Opening' } })
    // The view follows OBS, not the click.
    expect(wrapper.get('[aria-pressed="true"] .name').text()).toBe('Game')
    send({ type: 'obs.state', payload: connected('Opening') })
    await flushPromises()
    expect(wrapper.get('[aria-pressed="true"] .name').text()).toBe('Opening')
    expect(wrapper.text()).toContain(t('scene.noItems'))
  })

  it('shows the items front to back and asks OBS to change one', async () => {
    const wrapper = await page()
    snapshot(connected())
    await flushPromises()
    const rows = wrapper.findAll('tbody tr')
    // A known kind reads as its name; another shows as OBS names it.
    expect(rows.map((r) => [r.get('th .num').text(), r.get('th .item-name').text(), r.findAll('td')[1].text()])).toEqual([
      ['1', 'Chat overlay', t('scene.kinds.browser')],
      ['2', 'Capture', 'spout_capture'],
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

    const lock = wrapper.get(`[aria-label="${t('scene.lockItem', { item: 'Chat overlay' })}"]`)
    expect(lock.attributes('aria-pressed')).toBe('false')
    await lock.trigger('click')
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
