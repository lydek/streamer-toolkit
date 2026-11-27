// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import BotTimersPage from './BotTimersPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const timer = (id: string, nextAt: string | null, enabled = true) => ({
  id,
  name: '喝水提醒',
  message: '大家記得喝水 <b>💧</b>',
  every_minutes: 30,
  min_chat_lines: 5,
  enabled,
  next_at: nextAt,
})

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true
  }
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false
    this.dispatchEvent(new Event('close'))
  }
})
beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open(list: unknown[]) {
  vi.spyOn(client, 'GET').mockImplementation((() => ok(list)) as never)
  const w = mount(BotTimersPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}

it('shows each timed message as text with when it is next due, or that it waits for the stream', async () => {
  const in12 = new Date(Date.now() + 12 * 60_000).toISOString()
  const w = await open([timer('t1', in12), timer('t2', null), timer('t3', null, false)])
  expect(w.get('[data-timer="t1"] .message').text()).toBe('大家記得喝水 <b>💧</b>')
  expect(w.find('[data-timer="t1"] .message b').exists()).toBe(false)
  expect(w.get('[data-timer="t1"]').text()).toContain(t('bot.minutes', { n: 30 }))
  expect(w.get('[data-timer="t1"]').text()).toContain(t('bot.timers.lines', { n: 5 }))
  expect(w.get('[data-timer="t1"] [data-next]').text()).toBe(t('bot.timers.inMinutes', { n: 12 }))
  expect(w.get('[data-timer="t2"] [data-next]').text()).toBe(t('bot.timers.offline'))
  expect(w.get('[data-timer="t3"] [data-next]').text()).toBe('—')
})

it('adds a timed message, and says why the Host refused one', async () => {
  const post = vi.fn((): Promise<unknown> => Promise.resolve({ error: { code: 'bot.invalid_timer' }, response: new Response(null, { status: 400 }) }))
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  const w = await open([])
  expect(w.text()).toContain(t('bot.timers.empty'))
  await w.findAll('button').find((b) => b.text() === t('bot.timers.add'))!.trigger('click')
  await w.get('#timer-name').setValue('Discord 宣傳')
  await w.get('#timer-message').setValue('輸入 !discord 看連結')
  await w.get('#timer-form').trigger('submit')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/bot/timers', {
    body: { name: 'Discord 宣傳', message: '輸入 !discord 看連結', every_minutes: 15, min_chat_lines: 5, enabled: true },
  })
  expect(w.get('.timer-dialog [role="alert"]').text()).toBe(t('errors.bot.invalid_timer'))
})

it('turns a timed message off from its switch', async () => {
  const put = vi.fn(() => ok(timer('t1', null, false)))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  const w = await open([timer('t1', null)])
  await w.get(`[aria-label="${t('bot.timers.enableLabel', { name: '喝水提醒' })}"]`).trigger('change')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/bot/timers/{id}', {
    params: { path: { id: 't1' } },
    body: { name: '喝水提醒', message: '大家記得喝水 <b>💧</b>', every_minutes: 30, min_chat_lines: 5, enabled: false },
  })
})
