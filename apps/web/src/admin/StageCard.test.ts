// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import StageCard from './StageCard.vue'
import { testRouter } from './testRouter'

const { exportRecord } = vi.hoisted(() => ({ exportRecord: vi.fn() }))
vi.mock('./streamRecord', () => ({ exportRecord }))

const t = i18n.global.t
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
  const wrapper = mount(StageCard, { props: { live }, global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
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

  it('offers nothing while live', async () => {
    const wrapper = await card(null)
    expect(wrapper.findAll('button').some((b) => b.text() === t('record.export'))).toBe(false)
  })
})
