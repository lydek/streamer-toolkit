// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'
import { i18n } from '../i18n'
import TabBarEditor from './TabBarEditor.vue'

const t = i18n.global.t
const bar = reactive({ groups: null as string[] | null, position: 'top' as const })
const saveTabBar = vi.fn<(next: unknown) => Promise<void>>(async () => {})
vi.mock('./tabbar', () => ({ useTabBar: () => bar, saveTabBar: (next: unknown) => saveTabBar(next) }))

enableAutoUnmount(afterEach)
beforeAll(() => {
  // jsdom has no modal dialogs; the real behavior is checked by hand (docs/dev.md).
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true
  }
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false
    this.dispatchEvent(new Event('close'))
  }
})

async function opened() {
  const wrapper = mount(TabBarEditor, {
    props: { groups: ['home', 'interact', 'chat'] },
    global: { plugins: [i18n, PrimeVue] },
    attachTo: document.body,
  })
  ;(wrapper.vm as unknown as { open: () => void }).open()
  await flushPromises()
  return wrapper
}
const order = () => [...document.querySelectorAll<HTMLElement>('[data-row]')].map((li) => li.dataset.row)
const button = (row: string, dir: string) => document.querySelector<HTMLButtonElement>(`[data-row="${row}"] [data-dir="${dir}"]`)!
const live = () => document.querySelector('[aria-live="polite"]')!.textContent

describe('TabBarEditor', () => {
  it('moves a row with its arrow button, keeps focus on it and says where it went', async () => {
    await opened()
    button('home', 'down').focus()
    button('home', 'down').click()
    await flushPromises()
    expect(order()).toEqual(['interact', 'home', 'chat'])
    expect(document.activeElement).toBe(button('home', 'down'))
    expect(document.querySelector('[data-row="home"]')!.classList.contains('moved')).toBe(true)
    expect(live()).toBe(t('arrange.moved', { name: t('nav.groups.home'), position: 2 }))
  })

  it('moves focus to the other arrow when the row reaches an end', async () => {
    await opened()
    button('interact', 'up').click()
    await flushPromises()
    expect(order()).toEqual(['interact', 'home', 'chat'])
    expect(document.activeElement).toBe(button('interact', 'down'))
  })

  it('drops a dragged row before or after the row it lands on, and saves that order', async () => {
    const wrapper = await opened()
    const rows = () => wrapper.findAll('[data-row]')
    const target = rows()[0]
    vi.spyOn(target.element, 'getBoundingClientRect').mockReturnValue({ top: 0, height: 20 } as DOMRect)
    await rows()[2].trigger('dragstart')
    await target.trigger('dragover', { clientY: 15 })
    await target.trigger('drop')
    expect(order()).toEqual(['home', 'chat', 'interact'])
    await wrapper.find('.sheet-actions button').trigger('click')
    await flushPromises()
    expect(saveTabBar).toHaveBeenCalledWith({ groups: ['home', 'chat', 'interact'], position: 'top' })
  })
})
