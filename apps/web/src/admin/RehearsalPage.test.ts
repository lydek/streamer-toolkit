// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import RehearsalPage from './RehearsalPage.vue'
import { testRouter } from './testRouter'

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
let post: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  post = vi.fn((path: string, init?: { body: { script: string } }) => {
    if (path === '/api/admin/simulate/script' && init?.body.script.includes('broken')) {
      return Promise.resolve({ error: { code: 'rehearsal.invalid', line: 3, step: null }, response: new Response(null, { status: 400 }) })
    }
    return path === '/api/admin/simulate/script' ? ok({ steps: 10, duration_ms: 9000 }) : ok()
  })
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    path === '/api/admin/checkin/settings' ? ok({ card_slots: 5 }) : ok()) as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('plays a built-in script at the chosen speed, stops it, and names the line of a broken file', async () => {
  const wrapper = mount(RehearsalPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await wrapper.findAll('[aria-labelledby="rh-speed"] button').find((b) => b.text() === '2 倍')!.trigger('click')
  await wrapper.findAll('li').find((l) => l.text().includes('多人打卡'))!.get('button').trigger('click')
  await flushPromises()
  const body = post.mock.calls[0][1].body as { script: string; speed: string }
  expect(body.speed).toBe('double')
  const stamps = JSON.parse(body.script).steps.map((s: { stamps: number }) => s.stamps)
  // A card of 5 slots: no step asks for more, and the last one fills it.
  expect(stamps).toEqual([1, 2, 3, 4, 5, 5, 5, 5, 5, 5])
  expect(wrapper.text()).toContain('演練中')

  await wrapper.findAll('button').find((b) => b.text() === '停止演練')!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/simulate/script/stop')

  const input = wrapper.get<HTMLInputElement>('#rh-file')
  Object.defineProperty(input.element, 'files', { value: [new File(['{ broken'], 'mine.json')] })
  await input.trigger('change')
  await flushPromises()
  expect(wrapper.get('[role="alert"]').text()).toBe('劇本格式不正確：第 3 行。')
})
