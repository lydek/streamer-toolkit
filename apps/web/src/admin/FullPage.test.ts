// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import FullPage from './FullPage.vue'
import { testRouter } from './testRouter'

const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const page = (props: Record<string, string>) =>
  mount(FullPage, { props: { title: '背景音樂', ...props }, global: { plugins: [testRouter(), i18n, PrimeVue] } })

afterEach(() => toastAdd.mockReset())

describe('FullPage header', () => {
  it('shows the tile icon, the status and copies the overlay URL', async () => {
    const writeText = vi.fn(() => Promise.resolve())
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const wrapper = page({ tile: 'bgm', status: '播放中：Lo-fi', overlay: 'bgm' })
    expect(wrapper.get('.badge').classes()).toContain('tint-yellow')
    expect(wrapper.get('.status').text()).toBe('播放中：Lo-fi')
    const copy = wrapper.findAll('button').find((b) => b.text() === '複製 OBS 網址')!
    await copy.trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith(`${location.origin}/overlay/bgm/`)
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'success' }))
  })

  it('shows neither a status nor a copy button on a page without them', () => {
    const wrapper = page({})
    expect(wrapper.find('.page-end').exists()).toBe(false)
    expect(wrapper.find('.badge').exists()).toBe(false)
  })
})
