// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import PollsOverlay from './PollsOverlay.vue'

const t = i18n.global.t
const NOW = '2026-11-27T10:00:00Z'

let socket: { url: string; onmessage?: (e: { data: string }) => void }
beforeEach(() => {
  vi.useFakeTimers({ now: Date.parse(NOW) })
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor(public url: string) {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    },
  )
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

/** `endsAt`: when voting ends, or ended. */
const poll = (status: string, votes: [number, number], endsAt = '2026-11-27T10:01:12Z') => ({
  id: 'p1',
  title: '<b>下一首？</b>',
  status,
  endsAt,
  choices: [
    { id: 'c1', title: '晴天', votes: votes[0] },
    { id: 'c2', title: '小幸運', votes: votes[1] },
  ],
})
const prediction = (status: string, winner: string | null = null, endedAt = '') => ({
  id: 'd1',
  title: '吃雞？',
  status,
  winningOutcomeId: winner,
  locksAt: '2026-11-27T10:00:30Z',
  endedAt,
  outcomes: [
    { id: 'o1', title: '能', users: 18, points: 12400 },
    { id: 'o2', title: '不能', users: 11, points: 8900 },
  ],
})
const contest = (seq: number, payload: unknown) => ({
  id: `v${seq}`,
  type: 'twitch.contest',
  version: 1,
  seq,
  occurredAt: NOW,
  source: 'twitch',
  payload,
})
const send = (msg: unknown) => socket.onmessage!({ data: JSON.stringify(msg) })
const tick = async (ms: number) => {
  vi.advanceTimersByTime(ms)
  await flushPromises()
}

it('is empty with nothing on, shows a running poll as text with its time left, and its result for a while after', async () => {
  const w = mount(PollsOverlay, { global: { plugins: [i18n] } })
  expect(socket.url).toContain('/ws/overlay?kind=polls')
  send({ type: 'snapshot', version: 1, seq: 1, payload: { 'twitch.contest': [] } })
  await flushPromises()
  expect(w.html()).not.toContain('section')

  send(contest(2, { poll: poll('active', [3, 1]), prediction: null }))
  await flushPromises()
  expect(w.get('.title').text()).toBe('<b>下一首？</b>')
  expect(w.find('.title b').exists()).toBe(false)
  expect(w.findAll('.value').map((v) => v.text())).toEqual(['75%', '25%'])
  expect(w.get('.foot').text()).toBe('1:12')

  // Ended early, now.
  send(contest(3, { poll: poll('terminated', [3, 1], NOW), prediction: null }))
  await flushPromises()
  expect(w.get('.foot').text()).toBe(t('overlay.polls.ended'))
  expect(w.get('.row.lead').text()).toContain('晴天')
  await tick(9_000)
  expect(w.find('[data-poll]').exists()).toBe(true)
  await tick(2_000)
  expect(w.find('[data-poll]').exists()).toBe(false)
})

it('shows a running prediction until it is settled, then the winner for a while', async () => {
  const w = mount(PollsOverlay, { global: { plugins: [i18n] } })
  // Joining while one runs: the snapshot has it.
  send({ type: 'snapshot', version: 1, seq: 5, payload: { 'twitch.contest': [contest(5, { poll: null, prediction: prediction('locked') })] } })
  await flushPromises()
  expect(w.get('.foot').text()).toBe(t('overlay.polls.locked'))
  expect(w.findAll('.value').map((v) => v.text())).toEqual(['12,400', '8,900'])
  // An older event is not applied over the snapshot.
  send(contest(4, { poll: null, prediction: prediction('active') }))
  await flushPromises()
  expect(w.get('.foot').text()).toBe(t('overlay.polls.locked'))

  send(contest(6, { poll: null, prediction: prediction('resolved', 'o2', NOW) }))
  await flushPromises()
  expect(w.get('.foot').text()).toBe(t('overlay.polls.won', { name: '不能' }))
  expect(w.get('.row.lead').text()).toContain('不能')
  await tick(11_000)
  expect(w.find('[data-prediction]').exists()).toBe(false)
})

it('shows the result that ended last, not an older poll', async () => {
  const w = mount(PollsOverlay, { global: { plugins: [i18n] } })
  // A poll ended a few minutes ago; the prediction is settled now.
  const earlier = poll('completed', [3, 1], '2026-11-27T09:55:00Z')
  send(contest(1, { poll: earlier, prediction: prediction('resolved', 'o1', NOW) }))
  await flushPromises()
  expect(w.find('[data-poll]').exists()).toBe(false)
  expect(w.get('.foot').text()).toBe(t('overlay.polls.won', { name: '能' }))
})

it('never brings back a result that ended earlier, even when it arrives now', async () => {
  const w = mount(PollsOverlay, { global: { plugins: [i18n] } })
  // E.g. the admin page read Twitch's latest poll and prediction, from yesterday's stream.
  send(
    contest(1, {
      poll: poll('completed', [3, 1], '2026-11-26T21:00:00Z'),
      prediction: prediction('canceled', null, '2026-11-26T21:30:00Z'),
    }),
  )
  await flushPromises()
  expect(w.find('section').exists()).toBe(false)
  // Without Twitch's end time it is not shown either; a running one without a time shows no countdown.
  send(contest(2, { poll: poll('completed', [3, 1], ''), prediction: null }))
  await flushPromises()
  expect(w.find('section').exists()).toBe(false)
  send(contest(3, { poll: poll('active', [3, 1], ''), prediction: null }))
  await flushPromises()
  expect(w.get('.foot').text()).toBe('')
})
