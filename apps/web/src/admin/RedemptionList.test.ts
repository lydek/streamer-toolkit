// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import { twitchOverview } from './home'
import RedemptionList from './RedemptionList.vue'
import type { RedemptionRecord } from './redemptions'
import { testRouter } from './testRouter'

const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t

const record = (over: Partial<RedemptionRecord> = {}): RedemptionRecord => ({
  id: 'r1',
  twitch_id: 'rd-1',
  reward_id: 'w-own',
  user_login: 'viewer',
  user_name: 'Viewer',
  reward_title: 'Hydrate',
  reward_cost: 100,
  user_input: '',
  status: 'unfulfilled',
  redeemed_at: '2026-11-16T10:00:00.000Z',
  updated_at: '2026-11-16T10:00:00.000Z',
  played: [],
  ...over,
})

const list = (records: RedemptionRecord[]) =>
  mount(RedemptionList, { props: { records, empty: '沒有紀錄' }, global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })

// Lists left mounted would still react to signing in and out.
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  toastAdd.mockReset()
  twitchOverview.value = undefined
  document.body.innerHTML = ''
})

it('shows the empty text when there are no records', () => {
  expect(list([]).text()).toBe('沒有紀錄')
})

it('shows viewer and Twitch text as text, never as markup', () => {
  const html = '<img src=x onerror="alert(1)"><b>bold</b>'
  const wrapper = list([record({ user_name: html, reward_title: html, user_input: html })])
  expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.find('b').exists()).toBe(false)
  expect(wrapper.find('.input').text()).toBe(html)
  expect(wrapper.find('.name').text()).toBe(html)
})

it('names the status in words with its icon, and leaves out an empty input', () => {
  const wrapper = list([record({ status: 'canceled' }), record({ id: 'r2', status: 'unknown', user_input: 'hi' })])
  const statuses = wrapper.findAll('.status')
  expect(statuses.map((s) => s.text())).toEqual([t('redemption.status.canceled'), t('redemption.status.unknown')])
  expect(statuses[0].find('i').attributes('aria-hidden')).toBe('true')
  expect(wrapper.findAll('.input').map((p) => p.text())).toEqual(['hi'])
  expect(wrapper.find('time').attributes('datetime')).toBe('2026-11-16T10:00:00.000Z')
})

it('says what each redemption played: each item and its result, — for nothing, 沒有紀錄 before this was kept', () => {
  const rows = list([
    record({ id: 'a', played: [{ what: 'card', result: 'played' }, { what: 'media', result: 'file_missing' }] }),
    record({ id: 'b', played: [] }),
    record({ id: 'c', played: null }),
  ]).findAll('.played')
  const label = t('redemption.played.label')
  const entry = (what: string, result: string) =>
    t('redemption.played.entry', { what: t(`redemption.played.what.${what}`), result: t(`redemption.played.result.${result}`) })
  expect(rows.map((r) => r.text())).toEqual([
    `${label}${entry('card', 'played')}、${entry('media', 'file_missing')}`,
    label + t('redemption.played.none'),
    label + t('redemption.played.untracked'),
  ])
})

describe('handling a waiting redemption', () => {
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
  const signedIn = (scopes: string[]) => {
    twitchOverview.value = { client_id: 'cid', eventsub_connected: true, irc_connected: false, scopes, status: { state: 'signed_in', user_id: '1', login: 'me' } }
  }
  const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
  const rewards = [{ id: 'w-own', manageable: true }, { id: 'w-dash', manageable: false }]
  const button = (w: ReturnType<typeof list>, label: string) => w.findAll('button').find((b) => b.text() === label)

  it('marks one of the toolkit’s rewards fulfilled, and refunds only after confirming', async () => {
    signedIn(['channel:manage:redemptions'])
    vi.spyOn(client, 'GET').mockImplementation((() => ok(rewards)) as never)
    const put = vi.fn(() => ok())
    vi.spyOn(client, 'PUT').mockImplementation(put as never)
    const w = list([record(), record({ id: 'r2', twitch_id: 'rd-2', reward_id: 'w-dash' }), record({ id: 'r3', status: 'fulfilled' })])
    await flushPromises()
    const [own, dash, done] = w.findAll('.record')
    expect(own.findAll('button').map((b) => b.text())).toEqual([t('redemption.fulfil'), t('redemption.refund')])
    // Made in the Twitch dashboard: only Twitch can handle it, and hovering says so.
    expect(dash.find('button').exists()).toBe(false)
    expect(dash.get('.not-ours').attributes('title')).toBe(t('redemption.notOurs'))
    expect(done.find('.actions').exists()).toBe(false)

    await button(w, t('redemption.fulfil'))!.trigger('click')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/redemptions/{twitch_id}/status', {
      params: { path: { twitch_id: 'rd-1' } },
      body: { reward_id: 'w-own', status: 'fulfilled' },
    })

    await button(w, t('redemption.refund'))!.trigger('click')
    expect(document.querySelector('#refund-title')?.textContent).toBe(t('redemption.refundTitle', { name: 'Viewer', reward: 'Hydrate' }))
    ;[...document.querySelectorAll<HTMLButtonElement>('dialog button')].find((b) => b.textContent?.includes(t('features.cancel')))!.click()
    await flushPromises()
    expect(put).toHaveBeenCalledTimes(1)
    await button(w, t('redemption.refund'))!.trigger('click')
    ;[...document.querySelectorAll<HTMLButtonElement>('dialog button')].find((b) => b.textContent === t('redemption.refund'))!.click()
    await flushPromises()
    expect(put).toHaveBeenLastCalledWith('/api/admin/redemptions/{twitch_id}/status', {
      params: { path: { twitch_id: 'rd-1' } },
      body: { reward_id: 'w-own', status: 'canceled' },
    })
  })

  it('offers to grant the scope first when the sign-in lacks it', async () => {
    signedIn([])
    vi.spyOn(client, 'GET').mockImplementation((() => ok(rewards)) as never)
    const w = list([record()])
    await flushPromises()
    expect(w.get('.record').findAll('button').map((b) => b.text())).toEqual([t('redemption.grant')])
  })

  it('reads the rewards once signed in, and forgets them on signing out', async () => {
    const get = vi.spyOn(client, 'GET').mockImplementation((() => ok(rewards)) as never)
    vi.spyOn(client, 'PUT').mockImplementation((() => ok()) as never)
    const w = list([record()])
    await flushPromises()
    expect(get).not.toHaveBeenCalled()
    expect(w.find('.actions').exists()).toBe(false)
    signedIn(['channel:manage:redemptions'])
    await flushPromises()
    expect(get).toHaveBeenCalledTimes(1)
    expect(button(w, t('redemption.fulfil'))).toBeDefined()
    twitchOverview.value = { ...twitchOverview.value!, scopes: [], status: { state: 'signed_out' } }
    await flushPromises()
    expect(w.find('.actions').exists()).toBe(false)
  })

  it('reads the rewards only when something is waiting', async () => {
    signedIn([])
    const get = vi.spyOn(client, 'GET').mockImplementation((() => ok(rewards)) as never)
    list([record({ status: 'fulfilled' })])
    await flushPromises()
    expect(get).not.toHaveBeenCalled()
  })
})
