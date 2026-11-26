// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import ChatComposer from './ChatComposer.vue'
import { twitchOverview } from './home'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
let post: ReturnType<typeof vi.fn>
const overview = (state: 'signed_in' | 'signed_out', scopes: string[]) => ({
  client_id: 'cid',
  eventsub_connected: true,
  irc_connected: false,
  scopes,
  status: state === 'signed_in' ? { state, user_id: '1', login: 'me' } : { state },
})

beforeEach(() => {
  post = vi.fn((path: string) => (path === '/api/admin/chat/send' ? ok({ message_id: 'm1' }) : ok({})))
  vi.spyOn(client, 'POST').mockImplementation(post as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  toastAdd.mockReset()
  twitchOverview.value = undefined
})

async function open() {
  const router = testRouter([{ path: '/settings/:section', component: { render: () => null } }])
  await router.push('/')
  return { w: mount(ChatComposer, { global: { plugins: [router, i18n, PrimeVue] } }), router }
}

it('waits for a sign-in, and offers to grant the chat scope when it is missing', async () => {
  twitchOverview.value = overview('signed_out', []) as never
  const { w: out } = await open()
  expect(out.get('input').attributes('disabled')).toBeDefined()

  twitchOverview.value = overview('signed_in', ['user:read:chat']) as never
  const { w, router } = await open()
  expect(w.find('input').exists()).toBe(false)
  await w.get('button').trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/twitch/login', { body: { add_scopes: ['user:write:chat'] } })
  expect(router.currentRoute.value.fullPath).toBe('/settings/connections#twitch')
})

it('sends on Enter and clears the box, and says why Twitch did not send it', async () => {
  twitchOverview.value = overview('signed_in', ['user:write:chat']) as never
  const { w } = await open()
  await w.get('input').setValue('  大家好  ')
  await w.get('input').trigger('keydown', { key: 'Enter' })
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/chat/send', { body: { text: '大家好' } })
  expect((w.get('input').element as HTMLInputElement).value).toBe('')

  post.mockImplementation(() => Promise.resolve({ error: { code: 'chat.send_dropped' }, response: new Response(null, { status: 422 }) }))
  await w.get('input').setValue('again')
  await w.get('input').trigger('keydown', { key: 'Enter' })
  await flushPromises()
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.chat.send_dropped') }))
  expect((w.get('input').element as HTMLInputElement).value).toBe('again')
})
