// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import CardStylePage from './CardStylePage.vue'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const style: Schemas['CardStyleBody'] = {
  preset: 'simple',
  background_color: '#181624',
  background_image: null,
  stamp_color: '#d8b7ff',
  stamp_image: null,
  per_row: 10,
  title: '#觀眾#',
  description: '累計 #次數# 次',
  animation: 'slide',
  show_repeat: false,
}
let put: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(path === '/api/admin/checkin/settings' ? { card_slots: 10 } : structuredClone(style))) as never)
  put = vi.fn((_: string, init: { body: Schemas['CardStyleBody'] }) => ok(init.body))
  post = vi.fn((path: string) => (path === '/api/admin/checkin/images' ? ok({ name: 'img.png', url: '/checkin/img.png' }) : ok()))
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  toastAdd.mockReset()
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('switches to the journal colors, uploads a stamp image, saves and tests in OBS', async () => {
  const wrapper = mount(CardStylePage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  await wrapper.findAll('[aria-labelledby="cs-preset"] button').find((b) => b.text() === '手帳')!.trigger('click')
  const input = wrapper.get<HTMLInputElement>('#cs-stamp-image')
  Object.defineProperty(input.element, 'files', { value: [new File(['x'], 'stamp.png')] })
  await input.trigger('change')
  await flushPromises()
  await wrapper.get('#cs-title').setValue('#觀眾# 打卡成功')

  await wrapper.findAll('button').find((b) => b.text() === '在 OBS 試播')!.trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/checkin/card-style', {
    body: { ...style, preset: 'journal', background_color: '#fbf6ee', stamp_color: '#c0392b', stamp_image: 'img.png', title: '#觀眾# 打卡成功' },
  })
  expect(post).toHaveBeenCalledWith('/api/admin/checkin/card-style/test')
  expect(wrapper.get('img.thumb').attributes('src')).toBe('/checkin/img.png')
})
