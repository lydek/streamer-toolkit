// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { i18n } from '../i18n'
import type { Schemas } from './api'
import { drawerId } from './drawer'
import {
  type Line,
  type LiveState,
  applyLive,
  errorToast,
  obsLight,
  successToast,
  tileGroups,
  twitchLight,
  useLiveEvents,
  useThrottled,
} from './home'
import HomeTile from './HomeTile.vue'

const chat = (seq: number, text = `msg ${seq}`): Schemas['ChatMessageEvent'] => ({
  id: String(seq),
  type: 'chat.message',
  version: 1,
  seq,
  occurredAt: '2026-11-14T10:00:00.000Z',
  source: 'simulation',
  payload: { userId: '1', userLogin: 'u', userName: 'U', color: null, text, fragments: [] },
})
const obsEvent = (seq: number, state: Schemas['ObsState']): Schemas['ObsStatusEvent'] => ({
  id: `o${seq}`,
  type: 'obs.status',
  version: 1,
  seq,
  occurredAt: '2026-11-14T10:00:00.000Z',
  source: 'obs',
  payload: { state, reason: null },
})
const snapshot = (seq: number, payload: Partial<Schemas['SnapshotPayload']> = {}): Schemas['Snapshot'] => ({
  type: 'snapshot',
  version: 1,
  seq,
  payload: { 'chat.message': [], 'obs.status': [], ...payload },
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('twitchLight', () => {
  const overview = (status: Schemas['AuthStatus'], client_id = 'abc'): Schemas['Overview'] => ({ client_id, status })

  it('maps loading, failure and each sign-in state', () => {
    expect(twitchLight(undefined, false)).toEqual({ state: 'loading', key: 'app.loading' })
    expect(twitchLight(overview({ state: 'signed_in', user_id: '1', login: 'me' }), true).state).toBe('error')
    expect(twitchLight(overview({ state: 'signed_out' }, ''), false).key).toBe('home.status.twitch.setup')
    expect(twitchLight(overview({ state: 'signed_in', user_id: '1', login: 'me' }), false).state).toBe('connected')
    expect(twitchLight(overview({ state: 'pending', user_code: 'X', verification_uri: 'u' }), false).state).toBe(
      'reconnecting',
    )
    expect(twitchLight(overview({ state: 'failed', code: 'twitch.unreachable' }), false).state).toBe('error')
    expect(twitchLight(overview({ state: 'signed_out' }), false).key).toBe('home.status.twitch.signedOut')
  })

  it('has a translation for every key', () => {
    const states: Schemas['AuthStatus'][] = [
      { state: 'signed_in', user_id: '1', login: 'me' },
      { state: 'pending', user_code: 'X', verification_uri: 'u' },
      { state: 'failed', code: 'x' },
      { state: 'signed_out' },
    ]
    const keys = [
      ...states.map((s) => twitchLight(overview(s), false).key),
      twitchLight(overview({ state: 'signed_out' }, ''), false).key,
      twitchLight(undefined, true).key,
      ...(['connected', 'reconnecting', 'error'] as const).map(
        (s) => obsLight({ status: 'open', lastSeq: 0, obs: { state: s, reason: null } }).key,
      ),
    ]
    for (const key of keys) expect(i18n.global.te(key), key).toBe(true)
  })
})

describe('obsLight', () => {
  it('is loading until the snapshot, failed while disconnected', () => {
    expect(obsLight({ status: 'loading', lastSeq: 0 }).state).toBe('loading')
    expect(obsLight({ status: 'open', lastSeq: 1 }).state).toBe('loading')
    expect(obsLight({ status: 'failed', lastSeq: 0 }).key).toBe('home.status.failed')
    expect(obsLight({ status: 'open', lastSeq: 1, obs: { state: 'reconnecting', reason: 'obs.unreachable' } }).state).toBe(
      'reconnecting',
    )
  })
})

describe('applyLive', () => {
  const start: LiveState = { status: 'loading', lastSeq: 0 }

  it('takes the latest OBS status and chat message from the snapshot, then newer events', () => {
    let s = applyLive(start, chat(1))
    expect(s).toBe(start)
    s = applyLive(s, snapshot(5, { 'chat.message': [chat(3), chat(4)], 'obs.status': [obsEvent(2, 'connected')] }))
    expect(s).toMatchObject({ status: 'open', lastSeq: 5, obs: { state: 'connected' }, lastChat: { seq: 4 } })
    expect(applyLive(s, chat(5))).toBe(s)
    s = applyLive(s, obsEvent(6, 'error'))
    s = applyLive(s, chat(7, 'hi'))
    expect(s).toMatchObject({ lastSeq: 7, obs: { state: 'error' }, lastChat: { payload: { text: 'hi' } } })
  })
})

describe('useLiveEvents', () => {
  it('drops known state on disconnect, reconnects, and stops with its scope', () => {
    vi.useFakeTimers()
    const sockets: { onmessage?: (e: { data: string }) => void; onclose?: () => void; close: () => void }[] = []
    vi.stubGlobal(
      'WebSocket',
      class {
        onmessage?: (e: { data: string }) => void
        onclose?: () => void
        close = vi.fn()
        constructor() {
          sockets.push(this)
        }
      },
    )
    const scope = effectScope()
    const live = scope.run(() => useLiveEvents(2000))!

    sockets[0].onmessage!({ data: JSON.stringify(snapshot(3, { 'obs.status': [obsEvent(2, 'connected')] })) })
    expect(live.value).toMatchObject({ status: 'open', obs: { state: 'connected' } })
    sockets[0].onclose!()
    expect(live.value).toEqual({ status: 'failed', lastSeq: 0 })
    vi.advanceTimersByTime(2000)
    expect(sockets).toHaveLength(2)

    scope.stop()
    expect(sockets[1].close).toHaveBeenCalled()
    sockets[1].onclose!()
    vi.advanceTimersByTime(2000)
    expect(sockets).toHaveLength(2)
  })
})

describe('useThrottled', () => {
  it('updates at most once per interval and always ends with the latest value', async () => {
    vi.useFakeTimers()
    const source = ref(0)
    const scope = effectScope()
    const shown = scope.run(() => useThrottled(() => source.value, 3000))!

    source.value = 1
    await nextTick()
    expect(shown.value).toBe(1) // first change is immediate

    source.value = 2
    await nextTick()
    source.value = 3
    await nextTick()
    expect(shown.value).toBe(1)
    vi.advanceTimersByTime(2999)
    expect(shown.value).toBe(1)
    vi.advanceTimersByTime(1)
    expect(shown.value).toBe(3) // trailing update with the latest value

    source.value = 4
    await nextTick()
    vi.advanceTimersByTime(3000)
    expect(shown.value).toBe(4)
    scope.stop()
  })
})

describe('tileGroups', () => {
  it('drops disabled modules and empty groups', () => {
    const tiles = [
      { id: 'chat', group: 'live' as const, icon: '' },
      { id: 'checkin', group: 'features' as const, icon: '' },
    ]
    expect(tileGroups([{ id: 'chat', enabled: true }, { id: 'checkin', enabled: false }], tiles)).toEqual([
      { group: 'live', tiles: [tiles[0]] },
    ])
    expect(tileGroups([{ id: 'chat', enabled: false }], tiles)).toEqual([])
  })
})

describe('toasts', () => {
  it('auto-dismisses success only', () => {
    expect(successToast('ok').life).toBeGreaterThan(0)
    expect(errorToast('no')).not.toHaveProperty('life')
  })
})

describe('HomeTile', () => {
  const tile = (line: Line, attention = false) =>
    mount(HomeTile, {
      props: { id: 'chat', name: '聊天室', icon: 'pi pi-comments', line, attention, expanded: false },
      global: { plugins: [i18n] },
    })

  it('is a disclosure button for its drawer and the drawer focus fallback', () => {
    const btn = tile({ state: 'ready', text: 'U：hi' }).get('button')
    expect(btn.attributes('aria-expanded')).toBe('false')
    expect(btn.attributes('aria-controls')).toBe(drawerId('chat'))
    expect(btn.attributes('data-drawer-tile')).toBe('chat')
    expect(btn.text()).toContain('U：hi')
    expect(btn.find('.dot').exists()).toBe(false)
  })

  it('shows loading text, and failed text with the attention dot and hidden label', () => {
    expect(tile({ state: 'loading' }).text()).toContain('載入中')
    const failed = tile({ state: 'failed' })
    expect(failed.text()).toContain('載入失敗')
    expect(failed.find('.dot').exists()).toBe(true)
    expect(failed.get('.sr-only').text()).toBe('需要注意')
    expect(tile({ state: 'ready', text: 'x' }, true).find('.dot').exists()).toBe(true)
  })
})
