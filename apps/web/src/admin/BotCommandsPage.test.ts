// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import BotCommandsPage from './BotCommandsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const taken = () =>
  Promise.resolve({ error: { code: 'commands.taken', module: 'counter', label: '死亡次數' }, response: new Response(null, { status: 409 }) })

const command = { id: 'c1', name: 'discord', allowed: 'subscriber', reply: '歡迎加入 <b>Discord</b>', cooldown_seconds: 30, enabled: true }
let list: unknown[]

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
  list = [command]
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/bot/status' ? { dropped: 2 } : list)) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(BotCommandsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}
const button = (w: Awaited<ReturnType<typeof open>>, label: string) => w.findAll('button').find((b) => b.text() === label)!

it('lists the commands with their reply as text, and says how many messages were dropped', async () => {
  const w = await open()
  const row = w.get('[data-command="c1"]')
  expect(row.text()).toContain('!discord')
  expect(row.get('.reply').text()).toBe('歡迎加入 <b>Discord</b>')
  expect(row.find('.reply b').exists()).toBe(false)
  expect(row.text()).toContain(t('commands.allowed.subscriber'))
  expect(row.text()).toContain(t('bot.seconds', { n: 30 }))
  expect(w.get('[data-dropped]').text()).toBe(t('bot.dropped', { n: 2 }))
})

it('adds a command, saying which feature already has the name', async () => {
  const post = vi.fn((): Promise<unknown> => taken())
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  const w = await open()
  await button(w, t('bot.commands.add')).trigger('click')
  await w.get('#cmd-name').setValue('!死亡')
  await w.get('#cmd-reply').setValue('死了 {計數:死亡次數} 次')
  await w.get('#cmd-form').trigger('submit')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/bot/commands', {
    body: { name: '!死亡', reply: '死了 {計數:死亡次數} 次', allowed: 'everyone', cooldown_seconds: 30, enabled: true },
  })
  expect(w.get('.command-dialog [role="alert"]').text()).toBe(t('errors.commands.taken', { feature: t('modules.names.counter'), label: '死亡次數' }))

  post.mockImplementation(() => ok({ ...command, id: 'c2', name: 'death' }))
  await w.get('#cmd-name').setValue('death')
  await w.get('#cmd-form').trigger('submit')
  await flushPromises()
  expect(w.get('.command-dialog').attributes('open')).toBeUndefined()
})

it('turns a command off from its switch, and deletes one after asking', async () => {
  const put = vi.fn(() => ok(command))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  const del = vi.fn(() => ok())
  vi.spyOn(client, 'DELETE').mockImplementation(del as never)
  const w = await open()
  await w.get(`[aria-label="${t('bot.commands.enableLabel', { name: 'discord' })}"]`).trigger('change')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/bot/commands/{id}', {
    params: { path: { id: 'c1' } },
    body: { name: 'discord', reply: '歡迎加入 <b>Discord</b>', allowed: 'subscriber', cooldown_seconds: 30, enabled: false },
  })

  await w.get(`[aria-label="${t('bot.commands.editLabel', { name: 'discord' })}"]`).trigger('click')
  await button(w, t('bot.commands.delete')).trigger('click')
  expect(w.get('#cmd-title').text()).toBe(t('bot.commands.deleteTitle', { name: 'discord' }))
  await button(w, t('bot.commands.delete')).trigger('click')
  await flushPromises()
  expect(del).toHaveBeenCalledWith('/api/admin/bot/commands/{id}', { params: { path: { id: 'c1' } } })
})
