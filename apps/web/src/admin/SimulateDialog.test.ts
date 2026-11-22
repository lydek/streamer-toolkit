// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import SimulateDialog from './SimulateDialog.vue'

const calls = vi.hoisted(() => ({ get: vi.fn(), post: vi.fn(), modules: { value: undefined as unknown } }))

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: calls.get, POST: calls.post },
  api: (p: Promise<unknown>) => p,
}))
vi.mock('./features', async (original) => ({
  ...(await original<typeof import('./features')>()),
  useModules: () => ({ modules: calls.modules }),
}))

const on = (ids: string[]) => ids.map((id) => ({ id, enabled: true, closed: false }))
let answers: Record<string, unknown>

beforeEach(() => {
  // jsdom has no modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true
  }
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false
    this.dispatchEvent(new Event('close'))
  }
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  answers = {
    '/api/admin/overlays': { chat: 1, alerts: 0, checkin: 1, sound: 0, bgm: 0, counter: 0, marathon: 0, break: 0, warnings: 0, leaderboard: 0, other: 0 },
    '/api/admin/sounds': { sounds: [{ id: 's1', label: '叮咚' }], slots: {}, volume: 100 },
    '/api/admin/checkin/settings': { card_slots: 7 },
    '/api/admin/music': {
      tracks: [
        { id: 't1', title: '沒量長度', duration_ms: null, missing: false, used_by: [] },
        { id: 't2', title: 'Lo-fi', duration_ms: 60000, missing: false, used_by: [] },
      ],
      playlists: [{ id: 'p1', name: '聊天', tracks: ['t1', 't2'], used_by: [] }],
      scene_rules: [],
    },
  }
  calls.get.mockReset()
  calls.get.mockImplementation((path: string) => Promise.resolve(answers[path]))
  calls.post.mockReset()
  calls.post.mockResolvedValue(undefined)
  calls.modules.value = on(['chat', 'alerts', 'sound', 'checkin', 'counter', 'marathon', 'bgm'])
})

const player = (state: string, playlist: string | null = 'p1') => ({
  id: 'e1',
  type: 'bgm.player',
  version: 1,
  seq: 1,
  occurredAt: '',
  source: 'system',
  payload: { state, playlist, track: null, positionMs: 0, onAir: true, shuffle: false, volume: 50 },
})

afterEach(() => vi.unstubAllGlobals())

async function opened(bgm: unknown = player('stopped')) {
  const wrapper = mount(SimulateDialog, { props: { bgm: bgm ?? undefined } as never, global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await (wrapper.vm as unknown as { open: () => Promise<void> }).open()
  await flushPromises()
  return wrapper
}
const row = (id: string) => document.querySelector(`[data-row="${id}"]`)!
const press = async (id: string, label: string) => {
  ;[...row(id).querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!.click()
  await flushPromises()
}

describe('SimulateDialog', () => {
  it('sends one test event per row through the shared routes and says so', async () => {
    const wrapper = await opened()
    await press('chat', '送一則訊息')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/chat', { body: { text: '這是一則測試訊息' } })
    expect(wrapper.get('[role="status"]').text()).toBe('已送出測試：聊天室 · 送一則訊息')
    await press('alerts', '送禮訂閱')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/alert', { body: { kind: 'gift', amount: 5 } })
    await press('alerts', '追隨')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/alert', { body: { kind: 'follow', amount: null } })
    await press('sound', '播放')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 's1' } } })
    // 集滿 uses the card's own slot count.
    await press('checkin', '集滿')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/checkin', { body: { stamps: 7 } })
    // The Host picks the test values; the dialog only names the overlay.
    await press('counter', '顯示測試值')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/overlay-test', { body: { overlay: 'counter' } })
    await press('marathon', '顯示測試值')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/simulate/overlay-test', { body: { overlay: 'marathon' } })
    await press('bgm', '播 5 秒')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/music/player/test')
    wrapper.unmount()
  })

  it('shows each overlay connection in words, and 無法確認 when it cannot be read', async () => {
    const wrapper = await opened()
    expect(row('chat').textContent).toContain('OBS 已連上')
    expect(row('alerts').textContent).toContain('還沒放進 OBS')
    wrapper.unmount()
    answers['/api/admin/overlays'] = undefined
    calls.get.mockImplementation((path: string) =>
      path === '/api/admin/overlays' ? Promise.reject(new Error('offline')) : Promise.resolve(answers[path]),
    )
    const again = await opened()
    expect(row('chat').textContent).toContain('無法確認')
    again.unmount()
  })

  it('cannot send for a feature turned off or with an empty sound library, and reports a failure', async () => {
    calls.modules.value = on(['chat', 'alerts', 'checkin'])
    answers['/api/admin/sounds'] = { sounds: [], slots: {}, volume: 100 }
    calls.post.mockRejectedValue(new Error('boom'))
    const wrapper = await opened()
    expect(row('sound').textContent).toContain('功能已關閉')
    expect([...row('sound').querySelectorAll('button')].every((b) => b.disabled)).toBe(true)
    for (const id of ['counter', 'marathon']) {
      expect(row(id).textContent).toContain('功能已關閉')
      expect(row(id).querySelector('button')!.disabled).toBe(true)
    }
    await press('chat', '送一則訊息')
    expect(wrapper.get('[role="status"]').text()).toContain('boom')
    wrapper.unmount()
  })

  it('follows the counter and the marathon timer switches each on its own', async () => {
    for (const [onlyOn, off] of [['counter', 'marathon'], ['marathon', 'counter']]) {
      calls.modules.value = on([onlyOn])
      const wrapper = await opened()
      expect(row(onlyOn).querySelector('button')!.disabled).toBe(false)
      expect(row(off).textContent).toContain('功能已關閉')
      wrapper.unmount()
    }
  })

  it('puts focus on 關閉 when it opens and tells the bar when it closes', async () => {
    const wrapper = await opened()
    expect(document.activeElement?.textContent?.trim()).toBe('關閉')
    ;(document.activeElement as HTMLButtonElement).click()
    expect(wrapper.emitted('closed')).toHaveLength(1)
    wrapper.unmount()
  })

  it('says why the music test cannot play: playing, no playlist, nothing playable, or no status yet', async () => {
    const reason = async (bgm: unknown) => {
      const wrapper = await opened(bgm)
      const text = row('bgm').querySelector('.state')!.textContent!.trim()
      const disabled = row('bgm').querySelector('button')!.disabled
      wrapper.unmount()
      return [text, disabled]
    }
    expect(await reason(player('playing'))).toEqual(['背景音樂正在播', true])
    expect(await reason(player('stopped', null))).toEqual(['還沒選過歌單', true])
    expect(await reason(null)).toEqual(['無法確認', true])
    // Paused is fine: the test plays on its own and the player stays paused.
    expect((await reason(player('paused')))[1]).toBe(false)
    answers['/api/admin/music'] = { tracks: [], playlists: [{ id: 'p1', name: '聊天', tracks: [], used_by: [] }], scene_rules: [] }
    expect(await reason(player('stopped'))).toEqual(['歌單裡沒有能播的歌', true])
  })


  it('offers only sounds still in the library, and 集滿 only once the card size is known', async () => {
    let slots: (v: unknown) => void = () => {}
    calls.get.mockImplementation((path: string) =>
      path === '/api/admin/checkin/settings' ? new Promise((r) => (slots = r)) : Promise.resolve(answers[path]),
    )
    const wrapper = await opened()
    const full = () => [...row('checkin').querySelectorAll('button')].find((b) => b.textContent?.trim() === '集滿')!
    expect(full().disabled).toBe(true)
    slots({ card_slots: 7 })
    await flushPromises()
    expect(full().disabled).toBe(false)

    // Reopened after the chosen sound was deleted: the next one is picked; an unreadable card size turns 集滿 off.
    ;(document.querySelector('dialog') as HTMLDialogElement).close()
    answers['/api/admin/sounds'] = { sounds: [{ id: 's2', label: '鈴聲' }], slots: {}, volume: 100 }
    calls.get.mockImplementation((path: string) =>
      path === '/api/admin/checkin/settings' ? Promise.reject(new Error('offline')) : Promise.resolve(answers[path]),
    )
    await (wrapper.vm as unknown as { open: () => Promise<void> }).open()
    await flushPromises()
    await press('sound', '播放')
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 's2' } } })
    expect(full().disabled).toBe(true)
    wrapper.unmount()
  })

})
