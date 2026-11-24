// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import RewardPlaybackPage from './RewardPlaybackPage.vue'

const t = i18n.global.t
const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })

let slots: Record<string, string>
let rules: Schemas['MediaRuleBody'][]
let put: ReturnType<typeof vi.fn>, post: ReturnType<typeof vi.fn>, del: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  slots = { 'reward.gone': 's1' }
  rules = [{ reward_id: 'r2', media_id: 'v1', position: 'full', volume: null }]
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => {
    if (path === '/api/admin/checkin/rewards') {
      return ok([
        { id: 'r-checkin', title: '每日打卡' },
        { id: 'r1', title: '喝水提醒' },
        { id: 'r2', title: '丟枕頭' },
      ])
    }
    if (path === '/api/admin/checkin/settings') return ok({ reward_id: 'r-checkin' })
    if (path === '/api/admin/sounds') {
      return ok({ sounds: [{ id: 's1', label: '叮咚', duration_ms: 500, used_by: [] }], slots: { ...slots } })
    }
    if (path === '/api/admin/screen-tools/media') {
      return ok([
        { id: 'v1', title: '驚嚇', format: 'webm', duration_ms: 3000, url: '/media/v1', missing: false },
        { id: 'g1', title: '貓咪', format: 'gif', duration_ms: 2000, url: '/media/g1', missing: false },
      ])
    }
    if (path === '/api/admin/screen-tools/media-rules') return ok(structuredClone(rules))
    if (path === '/api/admin/overlays') return ok({ connected: false, sources: [] })
    if (path === '/api/admin/modules') return ok([])
    return ok({})
  }) as never)
  put = vi.fn((path: string, init?: { params?: { path?: Record<string, string> }; body?: Record<string, unknown> }) => {
    if (path === '/api/admin/sounds/slots/{slot}') {
      const slot = init!.params!.path!.slot
      const sound = init!.body!.sound as string | null
      if (sound) slots[slot] = sound
      else delete slots[slot]
      return ok()
    }
    if (path === '/api/admin/screen-tools/media-rules/{reward_id}') {
      const reward_id = init!.params!.path!.reward_id
      const body = init!.body as unknown as Schemas['MediaRuleInput']
      const rule = { reward_id, ...body, volume: body.volume ?? null }
      rules = [...rules.filter((r) => r.reward_id !== reward_id), rule]
      return ok(rule)
    }
    return ok()
  })
  post = vi.fn(() => ok())
  del = vi.fn(() => ok())
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'DELETE').mockImplementation(del as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

async function mountPage() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/reward-playback', component: RewardPlaybackPage },
      { path: '/rewards', component: { template: '<div />' } },
      { path: '/redemptions', component: { template: '<div />' } },
      { path: '/obs-sources', component: { template: '<div />' } },
    ],
  })
  await router.push('/reward-playback')
  const wrapper = mount(RewardPlaybackPage, {
    global: { plugins: [i18n, PrimeVue, ToastService, router] },
    attachTo: document.body,
  })
  await flushPromises()
  return wrapper
}

it('shows one table with check-in note, gone rewards, and unified OBS testing', async () => {
  const wrapper = await mountPage()
  expect(wrapper.findAll('table')).toHaveLength(1)

  const checkinRow = wrapper.get('[data-reward="r-checkin"]')
  expect(checkinRow.text()).toContain(t('rewardPlayback.checkinManaged'))
  expect(checkinRow.findAll('[role="combobox"]')).toHaveLength(0)

  const goneRow = wrapper.get('[data-reward="gone"]')
  expect(goneRow.text()).toContain('gone')
  const goneBtn = goneRow.get('button')
  expect(goneBtn.attributes('disabled')).toBeUndefined()
  await goneBtn.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 's1' } } })

  const r1Row = wrapper.get('[data-reward="r1"]')
  expect(r1Row.get('button').attributes('disabled')).toBeDefined()

  const r2Row = wrapper.get('[data-reward="r2"]')
  await r2Row.get('button').trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/screen-tools/media-rules/{reward_id}/test', { params: { path: { reward_id: 'r2' } } })

  // Custom volume switch saves follow-master -> 100%.
  await r2Row.get('input[type="checkbox"]').setValue(false)
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/media-rules/{reward_id}', {
    params: { path: { reward_id: 'r2' } },
    body: { media_id: 'v1', position: 'full', volume: 100 },
  })
})

it('serializes back-to-back media rule updates and still exposes legacy rules on the check-in reward', async () => {
  slots = { 'reward.r-checkin': 's1' }
  const wrapper = await mountPage()
  // Check-in reward with an existing saved sound still renders controls so it can be cleared.
  const checkinRow = wrapper.get('[data-reward="r-checkin"]')
  expect(checkinRow.text()).toContain(t('rewardPlayback.checkinManaged'))
  expect(checkinRow.findAll('[role="combobox"]').length).toBeGreaterThan(0)

  const answer = put.getMockImplementation() as (...args: unknown[]) => unknown
  let release = () => {}
  put.mockImplementationOnce((...args: unknown[]) => new Promise((r) => (release = () => r(answer(...args)))))
  const vm = wrapper.vm as unknown as { saveRule: (reward: string, patch: object) => Promise<unknown> }
  const both = Promise.all([vm.saveRule('r2', { position: 'center' }), vm.saveRule('r2', { volume: 50 })])
  await flushPromises()
  release()
  await both
  expect(put).toHaveBeenLastCalledWith('/api/admin/screen-tools/media-rules/{reward_id}', {
    params: { path: { reward_id: 'r2' } },
    body: { media_id: 'v1', position: 'center', volume: 50 },
  })
})
