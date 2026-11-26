// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import PollsPage from './PollsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const refused = (code: string) => Promise.resolve({ error: { code }, response: new Response(null, { status: 400 }) })

const poll = (status: string, votes: [number, number]) => ({
  id: 'p1',
  title: '下一首？',
  status,
  endsAt: new Date(Date.now() + 72_000).toISOString(),
  choices: [
    { id: 'c1', title: '晴天', votes: votes[0] },
    { id: 'c2', title: '小幸運', votes: votes[1] },
  ],
})
let templates: unknown[]
let sockets: { onmessage?: (e: { data: string }) => void }[]

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  sockets = []
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        sockets.push(this)
      }
      close() {}
    },
  )
  templates = [{ title: '吃什麼？', choices: ['拉麵', '咖哩'], seconds: 60 }]
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(path === '/api/admin/polls' ? { poll: poll('active', [3, 1]) } : templates)) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(PollsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}
const button = (w: Awaited<ReturnType<typeof open>>, label: string) => w.findAll('button').find((b) => b.text() === label)!
const live = (payload: unknown) =>
  sockets[0]!.onmessage!({ data: JSON.stringify({ type: 'twitch.contest', id: 'e', version: 1, seq: 9, occurredAt: '', source: 'twitch', payload }) })

it('shows the running poll from Twitch with its result and time left, follows it live, and ends it early', async () => {
  const end = vi.fn(() => ok({ poll: poll('terminated', [3, 1]) }))
  vi.spyOn(client, 'POST').mockImplementation(end as never)
  const w = await open()
  expect(w.text()).toContain('下一首？')
  expect(w.get('[data-choice="c1"]').text()).toContain('75%')
  expect(w.get('[data-poll-state]').text()).toMatch(/1:1\d/)

  live({ poll: poll('active', [3, 3]) })
  await flushPromises()
  expect(w.get('[data-choice="c1"]').text()).toContain('50%')
  expect(w.text()).toContain(t('polls.voters', { n: 6 }))

  await button(w, t('polls.end')).trigger('click')
  await flushPromises()
  expect(end).toHaveBeenCalledWith('/api/admin/polls/{id}/end', { params: { path: { id: 'p1' } } })
  expect(w.get('[data-poll-state]').text()).toBe(t('polls.status.terminated'))
  expect(button(w, t('polls.end'))).toBeUndefined()
})

it('starts a poll from the form once it has a question and two choices, up to five choices', async () => {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/polls' ? { poll: null } : [])) as never)
  const post = vi.fn(() => ok({ poll: poll('active', [0, 0]) }))
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  const w = await open()
  expect(w.text()).toContain(t('polls.none.poll'))
  const start = () => button(w, t('polls.start'))
  expect(start().attributes('disabled')).toBeDefined()
  await w.get('#polls-question').setValue(' 下一首？ ')
  const inputs = () => w.findAll('[role="group"] input')
  await inputs()[0]!.setValue('晴天')
  expect(start().attributes('disabled')).toBeDefined()
  await inputs()[1]!.setValue('小幸運')
  const add = button(w, t('polls.addOption'))
  for (let i = 0; i < 3; i++) await add.trigger('click')
  expect(inputs()).toHaveLength(5)
  expect(add.attributes('disabled')).toBeDefined()
  await start().trigger('click')
  await flushPromises()
  // Empty choices are left out.
  expect(post).toHaveBeenCalledWith('/api/admin/polls', { body: { title: '下一首？', choices: ['晴天', '小幸運'], seconds: 120 } })
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'success', summary: t('polls.started') }))
  // One running: no second start until it ends.
  expect(start().attributes('disabled')).toBeDefined()
})

it('keeps saved polls to start in one click, and says why Twitch refused one', async () => {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/polls' ? { poll: null } : templates)) as never)
  const put = vi.fn((_: string, init: { body: unknown[] }) => ok(init.body))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  const post = vi.fn(() => refused('polls.refused'))
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  const w = await open()
  const saved = w.get('.saved li')
  expect(saved.text()).toContain('拉麵 / 咖哩')
  await saved.findAll('button').find((b) => b.text() === t('polls.start'))!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/polls', { body: templates[0] })
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.polls.refused') }))

  // Filling the form from one, then saving another.
  await saved.get('button.link').trigger('click')
  expect((w.get('#polls-question').element as HTMLInputElement).value).toBe('吃什麼？')
  await w.get('#polls-question').setValue('喝什麼？')
  await button(w, t('polls.save')).trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/polls/templates', {
    body: [templates[0], { title: '喝什麼？', choices: ['拉麵', '咖哩'], seconds: 60 }],
  })
  await w.get(`[aria-label="${t('polls.deleteSaved', { name: '吃什麼？' })}"]`).trigger('click')
  await flushPromises()
  expect(put).toHaveBeenLastCalledWith('/api/admin/polls/templates', { body: [{ title: '喝什麼？', choices: ['拉麵', '咖哩'], seconds: 60 }] })
})

const prediction = (status: string, winner: string | null = null) => ({
  id: 'd1',
  title: '吃雞？',
  status,
  winningOutcomeId: winner,
  locksAt: new Date(Date.now() + 50_000).toISOString(),
  outcomes: [
    { id: 'o1', title: '能', users: 18, points: 12400 },
    { id: 'o2', title: '不能', users: 11, points: 8900 },
  ],
})

describe('predictions', () => {
  beforeAll(() => {
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.open = true
    }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement, value?: string) {
      if (value !== undefined) this.returnValue = value
      this.open = false
      this.dispatchEvent(new Event('close'))
    }
  })
  const dialogButton = (label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>('dialog button')].find((b) => b.textContent === label)!

  it('locks a running prediction, then resolves it only after confirming the winner', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      ok(path === '/api/admin/polls' ? { poll: null, prediction: prediction('active') } : [])) as never)
    const post = vi.fn((_: string, init: { body: { action: string } }) =>
      ok({ poll: null, prediction: init.body.action === 'lock' ? prediction('locked') : prediction('resolved', 'o1') }))
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const w = await open()
    expect(w.get('[data-outcome="o1"]').text()).toContain(t('polls.stake', { points: '12,400', users: 18 }))
    expect(w.get('[data-prediction-state]').text()).toMatch(/0:[45]\d/)

    await button(w, t('polls.lock')).trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/predictions/{id}/end', { params: { path: { id: 'd1' } }, body: { action: 'lock' } })
    expect(w.get('[data-prediction-state]').text()).toBe(t('polls.predictionStatus.locked'))

    await button(w, t('polls.wins', { name: '能' })).trigger('click')
    expect(document.querySelector('#polls-confirm-title')?.textContent).toBe(t('polls.resolveTitle', { name: '能' }))
    dialogButton(t('features.cancel')).click()
    await flushPromises()
    expect(post).toHaveBeenCalledTimes(1)
    await button(w, t('polls.wins', { name: '能' })).trigger('click')
    dialogButton(t('polls.wins', { name: '能' })).click()
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith('/api/admin/predictions/{id}/end', {
      params: { path: { id: 'd1' } },
      body: { action: 'resolve', winning_outcome_id: 'o1' },
    })
    expect(w.get('[data-prediction-state]').text()).toBe(t('polls.predictionStatus.resolved', { name: '能' }))
    expect(button(w, t('polls.cancel'))).toBeUndefined()
  })

  it('starts a prediction with up to ten outcomes and its own betting time', async () => {
    vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/polls' ? { poll: null, prediction: null } : [])) as never)
    const post = vi.fn(() => ok({ poll: null, prediction: prediction('active') }))
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const w = await open()
    await w.findAll('.p-togglebutton').find((b) => b.text() === t('polls.kinds.prediction'))!.trigger('click')
    expect(w.text()).not.toContain(t('polls.save'))
    await w.get('#polls-question').setValue('吃雞？')
    const inputs = () => w.findAll('[role="group"] input')
    await inputs()[0]!.setValue('能')
    await inputs()[1]!.setValue('不能')
    const add = button(w, t('polls.addOutcome'))
    for (let i = 0; i < 8; i++) await add.trigger('click')
    expect(inputs()).toHaveLength(10)
    expect(add.attributes('disabled')).toBeDefined()
    await button(w, t('polls.start')).trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/predictions', { body: { title: '吃雞？', outcomes: ['能', '不能'], seconds: 120 } })
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'success', summary: t('polls.predictionStarted') }))
  })
})

it('says a poll runs without a countdown when Twitch gives no time, and trims a question too long for a prediction', async () => {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(path === '/api/admin/polls' ? { poll: { ...poll('active', [1, 1]), endsAt: '' }, prediction: null } : [])) as never)
  const w = await open()
  expect(w.get('[data-poll-state]').text()).toBe(t('polls.running'))
  await w.get('#polls-question').setValue('字'.repeat(60))
  await w.findAll('.p-togglebutton').find((b) => b.text() === t('polls.kinds.prediction'))!.trigger('click')
  expect((w.get('#polls-question').element as HTMLInputElement).value).toBe('字'.repeat(45))
})
