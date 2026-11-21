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
  preStreamChecks,
  sessionRange,
  streamStage,
  arrangedTiles,
  availableTiles,
  dropTile,
  errorToast,
  homeTiles,
  STAGE_DEFAULTS,
  stageDefaults,
  TILES,
  moveTile,
  obsLight,
  successToast,
  tilePosition,
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
  payload: { userId: '1', userLogin: 'u', userName: 'U', color: null, text, fragments: [], badges: [], highlighted: false },
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
  payload: { 'chat.removed': [], 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [], ...payload },
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('twitchLight', () => {
  const overview = (status: Schemas['AuthStatus'], client_id = 'abc'): Schemas['Overview'] => ({
    client_id,
    status,
    eventsub_connected: false,
    irc_connected: false,
  })

  it('maps loading, failure and each sign-in state', () => {
    expect(twitchLight(undefined, false)).toEqual({ state: 'loading', key: 'app.loading' })
    expect(twitchLight(overview({ state: 'signed_in', user_id: '1', login: 'me' }), true).state).toBe('error')
    expect(twitchLight(overview({ state: 'signed_out' }, ''), false).key).toBe('home.status.twitch.setup')
    expect(twitchLight(overview({ state: 'signed_in', user_id: '1', login: 'me' }), false).state).toBe('connected')
    expect(twitchLight(overview({ state: 'pending', user_code: 'X', verification_uri: 'u' }), false).state).toBe(
      'reconnecting',
    )
    expect(twitchLight(overview({ state: 'failed', code: 'twitch.unreachable' }), false).state).toBe('error')
    expect(twitchLight(overview({ state: 'failed', code: 'twitch.scopes_missing' }), false)).toEqual({
      state: 'error',
      key: 'home.status.twitch.relogin',
    })
    expect(twitchLight(overview({ state: 'signed_out' }), false).key).toBe('home.status.twitch.signedOut')
  })

  it('has a translation for every key', () => {
    const states: Schemas['AuthStatus'][] = [
      { state: 'signed_in', user_id: '1', login: 'me' },
      { state: 'pending', user_code: 'X', verification_uri: 'u' },
      { state: 'failed', code: 'x' },
      { state: 'failed', code: 'twitch.scopes_missing' },
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

  it('takes the OBS scenes and outputs from the snapshot, then from newer events', () => {
    const state = (streaming: boolean): Schemas['StreamingState'] => ({ scene: 'Live', scenes: [], inputs: [], streaming, recording: false })
    const stateEvent = (seq: number, streaming: boolean): Schemas['ObsStateEvent'] => ({
      id: `s${seq}`,
      type: 'obs.state',
      version: 1,
      seq,
      occurredAt: '',
      source: 'obs',
      payload: state(streaming),
    })
    let s = applyLive(start, snapshot(2, { 'obs.state': [stateEvent(1, false)] }))
    expect(s.obsState).toEqual(state(false))
    s = applyLive(s, stateEvent(3, true))
    expect(s.obsState?.streaming).toBe(true)
  })

  it('takes the presentation queue from the snapshot, then from newer events', () => {
    const status = (waiting: number): Schemas['QueueStatus'] => ({
      current: null,
      waiting: Array.from({ length: waiting }, () => ({
        stage: 'alerts' as const,
        subject: { kind: 'alert' as const, alert: 'follow', viewer: 'Fan' },
        showMs: 6000,
        onAir: true,
      })),
      gapMs: 1000,
    })
    const queueEvent = (seq: number, waiting: number): Schemas['PresentationQueueEvent'] => ({
      id: `q${seq}`,
      type: 'presentation.queue',
      version: 1,
      seq,
      occurredAt: '',
      source: 'system',
      payload: status(waiting),
    })
    let s = applyLive(start, snapshot(2, { 'presentation.queue': [queueEvent(1, 3)] }))
    expect(s.queue?.waiting).toHaveLength(3)
    s = applyLive(s, queueEvent(3, 0))
    expect(s.queue).toEqual(status(0))
  })

  it('takes the live state from the snapshot, then from newer events', () => {
    const streamEvent = (seq: number, payload: Schemas['StreamStatusPayload']): Schemas['StreamStatusEvent'] => ({
      id: `l${seq}`,
      type: 'stream.status',
      version: 1,
      seq,
      occurredAt: '',
      source: 'twitch',
      payload,
    })
    let s = applyLive(start, snapshot(2, { 'stream.status': [streamEvent(1, { state: 'unknown' })] }))
    expect(s.stream).toEqual({ state: 'unknown' })
    s = applyLive(s, streamEvent(3, { state: 'live', startedAt: '2026-11-19T10:00:00.000Z', viewers: 42 }))
    expect(s.stream).toEqual({ state: 'live', startedAt: '2026-11-19T10:00:00.000Z', viewers: 42 })
  })

  it('keeps the latest alert until a new snapshot, which never holds alerts', () => {
    const alert = (seq: number): Schemas['AlertEvent'] => ({
      id: `a${seq}`,
      type: 'alert',
      version: 1,
      seq,
      occurredAt: '',
      source: 'simulation',
      payload: { kind: 'follow', userId: '7', userLogin: 'fan', userName: 'Fan', durationMs: 6000, sound: { kind: 'built_in', volume: 100 } },
    })
    let s = applyLive(start, snapshot(1, {}))
    s = applyLive(s, alert(2))
    expect(s.lastAlert?.id).toBe('a2')
    expect(applyLive(s, alert(2))).toBe(s)
    expect(applyLive(s, snapshot(3, {})).lastAlert).toBeUndefined()
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
    const heard: string[] = []
    const live = scope.run(() => useLiveEvents(2000, (msg) => heard.push(msg.type)))!

    sockets[0].onmessage!({ data: JSON.stringify(snapshot(3, { 'obs.status': [obsEvent(2, 'connected')] })) })
    expect(live.value).toMatchObject({ status: 'open', obs: { state: 'connected' } })
    expect(heard).toEqual(['snapshot'])
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

describe('stageDefaults', () => {
  it('gives each stage its own home from known tiles, and the defaults stand in until saved', () => {
    for (const stage of ['before', 'live', 'after'] as const) {
      expect(stageDefaults(stage).map((t) => t.id)).toEqual(STAGE_DEFAULTS[stage])
    }
    expect(stageDefaults('before')[0].id).toBe('checklist')
    expect(arrangedTiles(null, TILES, stageDefaults('after')).map((t) => t.id)).toEqual(STAGE_DEFAULTS.after)
    expect(arrangedTiles([], TILES, stageDefaults('after'))).toEqual([])
  })
})

describe('homeTiles', () => {
  it('keeps the order and drops tiles of disabled modules, but not the ones shown always', () => {
    const tiles = [
      { id: 'checkin', icon: '', tint: 'blue' as const },
      { id: 'chat', icon: '', tint: 'blue' as const },
      { id: 'feed', icon: '', tint: 'blue' as const, always: true },
    ]
    const modules = [
      { id: 'chat', enabled: true, closed: false },
      { id: 'checkin', enabled: false, closed: false },
    ]
    expect(homeTiles(modules, tiles).map((t) => t.id)).toEqual(['chat', 'feed'])
  })
})

describe('tile arrangement', () => {
  const tiles = [
    { id: 'chat', icon: '', tint: 'blue' as const },
    { id: 'checkin', icon: '', tint: 'blue' as const },
    { id: 'alerts', icon: '', tint: 'blue' as const },
  ]
  const ids = (list: { id: string }[]) => list.map((t) => t.id)

  it('uses the defaults until saved, skips unknown and repeated IDs, keeps an emptied home empty', () => {
    expect(arrangedTiles(null, tiles)).toBe(tiles)
    expect(ids(arrangedTiles(['alerts', 'gone', 'chat', 'alerts'], tiles))).toEqual(['alerts', 'chat'])
    expect(arrangedTiles([], tiles)).toEqual([])
    expect(ids(availableTiles(arrangedTiles(['alerts'], tiles), tiles))).toEqual(['chat', 'checkin'])
  })

  it('swaps a tile with its neighbour, whatever it is, and not past the edge', () => {
    expect(moveTile(tiles, 'alerts', -1)).toEqual(['chat', 'alerts', 'checkin'])
    expect(moveTile(tiles, 'chat', 1)).toEqual(['checkin', 'chat', 'alerts'])
    expect(moveTile(tiles, 'chat', -1)).toEqual(['chat', 'checkin', 'alerts'])
    expect(moveTile(tiles, 'alerts', 1)).toEqual(['chat', 'checkin', 'alerts'])
  })

  it('drops a dragged tile into the target place, anywhere, and reports positions', () => {
    expect(dropTile(tiles, 'alerts', 'chat')).toEqual(['alerts', 'chat', 'checkin'])
    expect(dropTile(tiles, 'chat', 'alerts')).toEqual(['checkin', 'alerts', 'chat'])
    expect(tilePosition(tiles, 'alerts')).toBe(3)
    expect(tilePosition(tiles, 'checkin')).toBe(2)
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
      props: { id: 'chat', name: '聊天室', icon: 'pi pi-comments', tint: 'blue', line, attention, expanded: false },
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

  it('shows a big count with its unit, a word smaller, and viewer text only as text', () => {
    const counted = tile({ state: 'ready', num: '1,200', unit: '人今天打卡', text: '最新：<b>小魚</b>' })
    expect(counted.get('.num').text()).toBe('1,200人今天打卡')
    expect(counted.get('.num').classes()).not.toContain('word')
    expect(counted.get('.line').text()).toBe('最新：<b>小魚</b>')
    expect(counted.find('b').exists()).toBe(false)
    expect(tile({ state: 'ready', num: '遊戲畫面', text: '6 個場景' }).get('.num').classes()).toContain('word')
    const bare = tile({ state: 'ready', num: '12', unit: '死亡次數', text: '' })
    expect(bare.find('.line').exists()).toBe(false)
    expect(tile({ state: 'ready', text: '還沒有音效' }).find('.num').exists()).toBe(false)
  })
})

describe('stream stage', () => {
  const summary = (endedAt: string | null, endedSinceLaunch = true): Schemas['StreamSummary'] => ({
    startedAt: '2026-11-18T11:58:00.000Z',
    endedAt,
    endedSinceLaunch,
    peakViewers: 58,
    follows: 3,
    subs: 5,
    bits: 1200,
    raids: 1,
    redemptions: 42,
    chatMessages: 1204,
    firstChats: 8,
  })

  it('is live while Twitch says so, after a stream that ended this run, and before otherwise', () => {
    expect(streamStage({ state: 'live', startedAt: '2026-11-18T11:58:00.000Z' }, summary(null))).toBe('live')
    expect(streamStage({ state: 'offline' }, summary('2026-11-18T14:39:00.000Z'))).toBe('after')
    expect(streamStage({ state: 'offline' }, summary('2026-11-18T14:39:00.000Z', false))).toBe('before')
    expect(streamStage({ state: 'offline' }, null)).toBe('before')
    expect(streamStage({ state: 'unknown' }, summary('2026-11-18T14:39:00.000Z'))).toBe('before')
    expect(streamStage(undefined, undefined)).toBe('before')
  })

  it('writes a session as day, start–end and length, with the end day when it differs', () => {
    const local = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).toISOString()
    expect(sessionRange(local(2026, 11, 18, 19, 58), local(2026, 11, 18, 22, 39))).toBe('11/18 19:58–22:39 · 2:41')
    expect(sessionRange(local(2026, 11, 18, 23, 30), local(2026, 11, 19, 1, 5))).toBe('11/18 23:30–11/19 01:05 · 1:35')
  })
})

describe('pre-stream checks', () => {
  const signedIn = (eventsub: boolean): Schemas['Overview'] => ({
    client_id: 'cid',
    status: { state: 'signed_in', user_id: '42', login: 'me' },
    eventsub_connected: eventsub,
    irc_connected: false,
  })
  const live = (obs: 'connected' | 'reconnecting'): LiveState => ({ status: 'open', lastSeq: 1, obs: { state: obs, reason: null } })
  const modules: Schemas['ModuleState'][] = [
    { id: 'chat', enabled: true, closed: false },
    { id: 'alerts', enabled: true, closed: false },
    { id: 'checkin', enabled: false, closed: false },
    { id: 'redemption', enabled: true, closed: false },
    { id: 'bgm', enabled: true, closed: false },
    // Optional on screen: never a check.
    { id: 'counter', enabled: true, closed: false },
  ]
  const overlays = { chat: 1, alerts: 0, checkin: 0, sound: 0, bgm: 1, counter: 0, marathon: 0, other: 1 }

  it('lists Twitch, EventSub, OBS and the overlay of each enabled feature, with where to fix them', () => {
    expect(preStreamChecks(signedIn(true), live('connected'), modules, overlays)).toEqual([
      { id: 'twitch', ok: true, fix: '/settings/connections#twitch' },
      { id: 'eventsub', ok: true, fix: '/settings/connections#twitch' },
      { id: 'obs', ok: true, fix: '/settings/connections#obs' },
      { id: 'overlay.chat', ok: true, fix: '/chat' },
      { id: 'overlay.alerts', ok: false, fix: '/alerts' },
      { id: 'overlay.bgm', ok: true, fix: '/music' },
    ])
  })

  it('counts nothing as fine before it is known', () => {
    const checks = preStreamChecks(undefined, { status: 'loading', lastSeq: 0 }, undefined, undefined)
    expect(checks.map((c) => [c.id, c.ok])).toEqual([
      ['twitch', false],
      ['eventsub', false],
      ['obs', false],
    ])
    expect(preStreamChecks(signedIn(false), live('reconnecting'), modules, overlays).filter((c) => !c.ok).map((c) => c.id)).toEqual([
      'eventsub',
      'obs',
      'overlay.alerts',
    ])
  })
})
