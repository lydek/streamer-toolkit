// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import SoundSlots from './SoundSlots.vue'

const t = i18n.global.t

const calls = vi.hoisted(() => ({ get: vi.fn(), post: vi.fn() }))

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: calls.get, POST: calls.post, PUT: vi.fn() },
  api: (p: Promise<unknown>) => p,
}))

beforeEach(() => {
  // PrimeVue's Select reads matchMedia, which jsdom lacks.
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  calls.get.mockResolvedValue({
    sounds: [{ id: 'bell', label: 'Bell', volume: 100, used_by: [], duration_ms: 3000 }],
    slots: { 'reward.r1': 'bell', 'reward.r2': 'bell' },
    volume: 100,
  })
  calls.post.mockReset()
  calls.post.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('SoundSlots', () => {
  it('turns the OBS play button into a stop until the sound has had its length', async () => {
    const wrapper = mount(SoundSlots, {
      props: { slots: [{ slot: 'reward.r1', label: 'A' }, { slot: 'reward.r2', label: 'B' }], playable: true },
      global: { plugins: [i18n, PrimeVue] },
    })
    await flushPromises()
    vi.useFakeTimers()
    const buttons = () => wrapper.findAll('button').map((b) => b.text())
    const [play, stop] = [t('sound.playInObs'), t('sound.stopInObs')]
    expect(buttons()).toEqual([play, play])

    await wrapper.findAll('button')[0].trigger('click')
    await flushPromises()
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/sounds/{id}/play', { params: { path: { id: 'bell' } } })
    expect(buttons()).toEqual([stop, play])

    await wrapper.findAll('button')[0].trigger('click')
    await flushPromises()
    expect(calls.post).toHaveBeenLastCalledWith('/api/admin/sounds/stop')
    expect(buttons()).toEqual([play, play])

    // Sending another moves the stop to it; once the sound's length has passed, it plays again.
    await wrapper.findAll('button')[0].trigger('click')
    await flushPromises()
    await wrapper.findAll('button')[1].trigger('click')
    await flushPromises()
    expect(buttons()).toEqual([play, stop])
    vi.advanceTimersByTime(3000)
    await flushPromises()
    expect(buttons()).toEqual([play, play])
    wrapper.unmount()
  })
})
