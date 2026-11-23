// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import RewardPicker from './RewardPicker.vue'

const t = i18n.global.t
const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data: unknown) => Promise.resolve({ data, response: new Response(null, { status: data === undefined ? 204 : 200 }) })
let post: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    ok(
      path === '/api/admin/checkin/rewards'
        ? [{ id: 'r1', title: '喝水' }, { id: 'r2', title: '跳舞' }]
        : { sounds: [], slots: { 'reward.r1': 's9', 'reward.r2': 'silent' }, volume: 100 },
    )) as never)
  post = vi.fn((path: string) =>
    path.includes('media-rules')
      ? Promise.resolve({ error: { code: 'media.not_found' }, response: new Response(null, { status: 404 }) })
      : ok(undefined),
  )
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  toastAdd.mockReset()
})
enableAutoUnmount(afterEach)

describe('RewardPicker', () => {
  it("plays only a reward's local effect, and says when it has none", async () => {
    const wrapper = mount(RewardPicker, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
    await wrapper.get('button').trigger('click')
    await flushPromises()
    const play = (title: string) => wrapper.findAll('button').find((b) => b.attributes('aria-label') === t('dock.rewards.playLabel', { title }))!
    await play('喝水').trigger('click')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 's9' } } })
    expect(toastAdd).toHaveBeenLastCalledWith(expect.objectContaining({ summary: expect.anything() }))
    // Nothing but the sound and the media test: no redemption or other record.
    expect(post.mock.calls.every(([path]) => path === '/api/admin/sounds/{id}/play' || path.includes('media-rules'))).toBe(true)

    post.mockClear()
    await play('跳舞').trigger('click')
    await flushPromises()
    expect(post).not.toHaveBeenCalledWith('/api/admin/sounds/{id}/play', expect.anything())
    expect(toastAdd).toHaveBeenLastCalledWith(expect.objectContaining({ summary: t('dock.rewards.none', { title: '跳舞' }) }))
  })

  it('moves the panel left when it would run past the window', async () => {
    const wrapper = mount(RewardPicker, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
    const rect = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ right: window.innerWidth + 100 } as DOMRect)
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(wrapper.get('.panel').attributes('style')).toContain('left: -108px')
    rect.mockRestore()
  })

  it('reads the rewards and their effects again each time it opens', async () => {
    const wrapper = mount(RewardPicker, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
    const reads = () => vi.mocked(client.GET).mock.calls.filter((c) => (c[0] as string) === '/api/admin/checkin/rewards').length
    await wrapper.get('button').trigger('click')
    await flushPromises()
    await wrapper.get('button').trigger('click')
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(reads()).toBe(2)
  })
})
