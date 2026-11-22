// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import MediaRules from './MediaRules.vue'

const t = i18n.global.t

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
let rules: Schemas['MediaRuleBody'][]
let put: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>, del: ReturnType<typeof vi.fn>

const media: Schemas['MediaItem'][] = [
  { id: 'g1', title: '貓咪跳舞', format: 'gif', duration_ms: 5000, url: '/media/g1', missing: false },
  { id: 'v1', title: '驚嚇', format: 'webm', duration_ms: 3000, url: '/media/v1', missing: false },
]

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  rules = [{ reward_id: 'r1', media_id: 'v1', position: 'full', volume: null }]
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(path === '/api/admin/checkin/rewards' ? [{ id: 'r1', title: '驚嚇一下' }, { id: 'r2', title: '跳舞' }] : structuredClone(rules))) as never)
  put = vi.fn((_: string, init: { params: { path: { reward_id: string } }; body: Schemas['MediaRuleInput'] }) => {
    const rule = { reward_id: init.params.path.reward_id, ...init.body, volume: init.body.volume ?? null }
    rules = [...rules.filter((r) => r.reward_id !== rule.reward_id), rule]
    return ok(rule)
  })
  post = vi.fn(() => ok())
  del = vi.fn((_: string, init: { params: { path: { reward_id: string } } }) => {
    rules = rules.filter((r) => r.reward_id !== init.params.path.reward_id)
    return ok()
  })
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'DELETE').mockImplementation(del as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('builds each change from the rule the previous one saved', async () => {
  const wrapper = mount(MediaRules, { props: { media }, global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  const answer = put.getMockImplementation() as (...args: unknown[]) => unknown
  let release = () => {}
  put.mockImplementationOnce((...args: unknown[]) => new Promise((r) => (release = () => r(answer(...args)))))
  const vm = wrapper.vm as unknown as { save: (reward: string, patch: object) => Promise<unknown> }
  const both = Promise.all([vm.save('r1', { position: 'center' }), vm.save('r1', { volume: 50 })])
  await flushPromises()
  release()
  await both
  expect(put).toHaveBeenLastCalledWith('/api/admin/screen-tools/media-rules/{reward_id}', {
    params: { path: { reward_id: 'r1' } },
    body: { media_id: 'v1', position: 'center', volume: 50 },
  })
})

it('drops the rules of a file deleted from the library', async () => {
  const wrapper = mount(MediaRules, { props: { media }, global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  const test = () => wrapper.get('[data-reward="r1"]').findAll('button').find((b) => b.text() === t('screenTools.rules.test'))!
  expect(test().attributes('disabled')).toBeUndefined()
  // The Host deleted the rule with the file; the library hands the shorter list down.
  rules = []
  await wrapper.setProps({ media: media.filter((m) => m.id !== 'v1') })
  await flushPromises()
  expect(test().attributes('disabled')).toBeDefined()
})

it('lists each reward with its media, switches the volume to its own and tests it', async () => {
  const wrapper = mount(MediaRules, { props: { media }, global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  const row = () => wrapper.get('[data-reward="r1"]')
  expect(row().text()).toContain('驚嚇一下')
  expect(row().text()).toContain('0:03')

  await row().get('input[type="checkbox"]').setValue(false)
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/media-rules/{reward_id}', {
    params: { path: { reward_id: 'r1' } },
    body: { media_id: 'v1', position: 'full', volume: 100 },
  })

  await row().findAll('button').find((b) => b.text() === t('screenTools.rules.test'))!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/screen-tools/media-rules/{reward_id}/test', { params: { path: { reward_id: 'r1' } } })
  expect(wrapper.get('[data-reward="r2"]').findAll('button').find((b) => b.text() === t('screenTools.rules.test'))!.attributes('disabled')).toBeDefined()
})
