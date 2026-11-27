// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import BotGreetingsPage from './BotGreetingsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const saved = () => ({
  follow: { enabled: true, message: '謝謝 {觀眾} 的追隨' },
  first_chat: { enabled: false, message: '歡迎 {觀眾}' },
  returning: { enabled: true, message: '{觀眾} 歡迎回來', post: false, days: 7 },
  sub: { enabled: true, message: '感謝訂閱' },
  bits: { enabled: true, small: '小', medium: '中', large: '大' },
  raid: { enabled: true, message: '感謝 {數量} 人' },
})

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation((() => ok(saved())) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(BotGreetingsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}
const toggle = (w: Awaited<ReturnType<typeof open>>, row: string) =>
  w.get(`[aria-label="${t('bot.greetings.enabled', { name: t(`bot.greetings.${row}.name`) })}"]`)

it('shows each greeting as saved, and saves every change at once', async () => {
  const put = vi.fn((_: string, init: { body: unknown }) => ok(init.body))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  const w = await open()
  expect((w.get('#greet-follow').element as HTMLInputElement).value).toBe('謝謝 {觀眾} 的追隨')
  expect((toggle(w, 'firstChat').element as HTMLInputElement).checked).toBe(false)

  await toggle(w, 'firstChat').trigger('change')
  await flushPromises()
  expect(put).toHaveBeenLastCalledWith('/api/admin/bot/greetings', { body: { ...saved(), first_chat: { enabled: true, message: '歡迎 {觀眾}' } } })

  await w.get('#greet-raid').setValue('謝謝揪團')
  await w.get('#greet-raid').trigger('change')
  await flushPromises()
  expect((put.mock.lastCall![1] as { body: { raid: unknown } }).body.raid).toEqual({ enabled: true, message: '謝謝揪團' })

  // 回訪: post instead of only tagging.
  await w.findAll('.p-togglebutton').find((b) => b.text() === t('bot.greetings.returning.modes.say'))!.trigger('click')
  await flushPromises()
  expect((put.mock.lastCall![1] as { body: { returning: { post: boolean } } }).body.returning.post).toBe(true)
})

it('says why a change was refused and shows what is saved again', async () => {
  vi.spyOn(client, 'PUT').mockImplementation((() =>
    Promise.resolve({ error: { code: 'bot.invalid_greetings' }, response: new Response(null, { status: 400 }) })) as never)
  const w = await open()
  await w.get('#greet-sub').setValue('')
  await w.get('#greet-sub').trigger('change')
  await flushPromises()
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.bot.invalid_greetings') }))
  expect((w.get('#greet-sub').element as HTMLInputElement).value).toBe('感謝訂閱')
})
