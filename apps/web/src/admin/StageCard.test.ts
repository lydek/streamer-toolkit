// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { i18n } from '../i18n'
import { client } from './api'
import type { LiveState } from './home'
import StageCard from './StageCard.vue'
import { useStage } from './stage'
import { testRouter } from './testRouter'

const { exportRecord } = vi.hoisted(() => ({ exportRecord: vi.fn() }))
vi.mock('./streamRecord', () => ({ exportRecord }))

const t = i18n.global.t

// Home shares one stage between its blocks; this stands in for it with both blocks.
const Blocks = defineComponent({
  props: { live: { type: Object, required: true }, modules: { type: Array, default: undefined } },
  setup(props) {
    const stage = useStage({ live: () => props.live as LiveState, modules: () => props.modules as never })
    return () => h('div', [h(StageCard, { part: 'checklist', stage }), h(StageCard, { part: 'session', stage })])
  },
})
const summary = (endedAt: string | null) => ({
  startedAt: '2026-11-21T10:00:00.000Z',
  endedAt,
  endedSinceLaunch: true,
  peakViewers: 42,
  follows: 1,
  subs: 4,
  bits: 150,
  raids: 1,
  redemptions: 1,
  chatMessages: 10,
  firstChats: 2,
})

async function card(endedAt: string | null) {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    Promise.resolve({
      data: path === '/api/admin/stream/summary' ? summary(endedAt) : undefined,
      response: new Response(null, { status: 200 }),
    })) as never)
  const live = {
    status: 'open' as const,
    lastSeq: 1,
    stream: endedAt ? { state: 'offline' as const, startedAt: null, viewers: null } : { state: 'live' as const, startedAt: '2026-11-21T10:00:00.000Z', viewers: 42 },
  }
  const wrapper = mount(Blocks, { props: { live }, global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

beforeEach(() => exportRecord.mockReset())
enableAutoUnmount(afterEach)
afterEach(() => vi.restoreAllMocks())

describe('StageCard export', () => {
  it('offers the record after the stream and says when it is saved', async () => {
    const wrapper = await card('2026-11-21T12:00:00.000Z')
    exportRecord.mockResolvedValue(true)
    const button = wrapper.findAll('button').find((b) => b.text() === t('record.export'))!
    await button.trigger('click')
    await flushPromises()
    expect(exportRecord).toHaveBeenCalledWith('2026-11-21T10:00:00.000Z')
    expect(wrapper.get('[role="status"]').text()).toBe(t('record.exported'))

    exportRecord.mockResolvedValue(false)
    await button.trigger('click')
    await flushPromises()
    // A cancelled save says nothing.
    expect(wrapper.find('[role="status"]').exists()).toBe(false)
  })

  it('masks the peak viewer count with the other numbers, and reveals just that one', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data: path === '/api/admin/stream/summary' ? summary('2026-11-21T12:00:00.000Z') : undefined,
        response: new Response(null, { status: 200 }),
      })) as never)
    const live = { status: 'open' as const, lastSeq: 1, stream: { state: 'offline' as const, startedAt: null, viewers: null } }
    const reveal = vi.fn()
    const Masked = defineComponent({
      setup() {
        const stage = useStage({ live: () => live as LiveState, modules: () => undefined })
        return () => h(StageCard, { part: 'session', stage, masked: () => true, onReveal: reveal })
      },
    })
    const wrapper = mount(Masked, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
    await flushPromises()
    expect(wrapper.get('.line').text()).toContain(t('stage.peak', { n: '––' }))
    expect(wrapper.get('.line').text()).not.toContain('42')
    await wrapper.get('.line button').trigger('click')
    expect(reveal).toHaveBeenCalledWith('stats.peak')
    // Every ID must pass the Host's check (lowercase, digits, `.`, `-`), or the reveal is refused.
    for (const b of wrapper.findAll('.stats button')) await b.trigger('click')
    expect(reveal.mock.calls.map(([id]) => id)).toContain('stats.chat-messages')
    for (const [id] of reveal.mock.calls) expect(id).toMatch(/^[a-z0-9.-]+$/)
  })

  it('offers nothing while live', async () => {
    const wrapper = await card(null)
    expect(wrapper.findAll('button').some((b) => b.text() === t('record.export'))).toBe(false)
  })
})

describe('StageCard checks', () => {
  it('links an overlay that is not in OBS yet to the OBS sources page, with its status', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/overlays'
            ? { chat: 0, alerts: 0, checkin: 0, sound: 0, bgm: 0, counter: 0, marathon: 0, break: 0, warnings: 0, leaderboard: 0, polls: 0, other: 0 }
            : path === '/api/admin/obs/sources'
              ? { connected: true, local: true, live_scene: 'Live', scenes: ['Live'], sources: [] }
              : null,
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = mount(Blocks, {
      props: { live: { status: 'open', lastSeq: 1 }, modules: [{ id: 'chat', enabled: true, closed: false }] },
      global: { plugins: [testRouter(), i18n, PrimeVue] },
      attachTo: document.body,
    })
    await flushPromises()
    const row = wrapper.findAll('.checks li').find((li) => li.text().includes(t('stage.checks.overlay.chat.bad')))!
    // Twitch and OBS still lead to where they are fixed; they are problems, a missing overlay only a warning.
    expect(wrapper.findAll('.checks li.bad a').length).toBe(3)
    expect(row.classes()).toContain('warn')
    expect(wrapper.findAll('.checks li.bad').length).toBe(3)
    const link = row.get('a')
    expect(link.attributes('href')).toBe('/obs-sources')
    expect(link.text()).toBe(t('obsStatus.missing'))
  })

  it('starts with every check, folds to none even with checks not ready, keeps the count, and unfolds again', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      Promise.resolve({
        data:
          path === '/api/admin/overlays'
            ? { chat: 1, alerts: 0, checkin: 0, sound: 0, bgm: 0, counter: 0, marathon: 0, break: 0, warnings: 0, leaderboard: 0, polls: 0, other: 0 }
            : null,
        response: new Response(null, { status: 200 }),
      })) as never)
    const wrapper = mount(Blocks, {
      props: { live: { status: 'open', lastSeq: 1 }, modules: [{ id: 'chat', enabled: true, closed: false }] },
      global: { plugins: [testRouter(), i18n, PrimeVue] },
      attachTo: document.body,
    })
    await flushPromises()
    const list = () => wrapper.get('.checks')
    const rows = () => wrapper.findAll('.checks li')
    const all = rows().length
    expect(rows().filter((li) => li.classes('ok'))).toHaveLength(1)
    expect(rows().some((li) => !li.classes('ok'))).toBe(true)
    const ready = t('stage.checks.ready', { ok: 1, all })
    expect(wrapper.get('.ready').text()).toBe(ready)
    await wrapper.findAll('button').find((b) => b.attributes('aria-expanded') === 'true')!.trigger('click')
    expect(list().isVisible()).toBe(false)
    expect(wrapper.get('.ready').text()).toBe(ready)
    await wrapper.findAll('button').find((b) => b.attributes('aria-expanded') === 'false')!.trigger('click')
    expect(list().isVisible()).toBe(true)
    expect(rows()).toHaveLength(all)
  })
})
