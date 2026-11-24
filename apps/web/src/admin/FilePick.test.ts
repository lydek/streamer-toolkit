// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import FilePick from './FilePick.vue'

const t = i18n.global.t
const mountPick = (props: Record<string, unknown>) =>
  mount(FilePick, { props, global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })

describe('FilePick', () => {
  it('opens the hidden input from its button, passes on what was picked and clears the choice', async () => {
    const wrapper = mountPick({ label: '匯入音效檔', inputId: 'f', accept: '.mp3' })
    const input = wrapper.get<HTMLInputElement>('#f')
    expect(input.attributes('hidden')).toBeDefined()
    const click = vi.spyOn(input.element, 'click')
    await wrapper.get('button').trigger('click')
    expect(click).toHaveBeenCalled()

    const file = new File(['x'], 'ding.mp3')
    const cleared: string[] = []
    Object.defineProperty(input.element, 'files', { configurable: true, value: [file] })
    Object.defineProperty(input.element, 'value', { configurable: true, get: () => 'C:\\fakepath\\ding.mp3', set: (v) => cleared.push(v) })
    await input.trigger('change')
    expect(wrapper.emitted('pick')).toEqual([[[file]]])
    expect(cleared).toEqual([''])
    wrapper.unmount()
  })

  it('takes a dropped file in a drop zone, one unless it allows several, and none while disabled', async () => {
    const wrapper = mountPick({ dropText: '把 .stbackup 拖到這裡' })
    expect(wrapper.text()).toContain('把 .stbackup 拖到這裡')
    expect(wrapper.get('button').text()).toBe(t('filePick.choose'))
    // The button is read with the zone's text.
    const described = wrapper.get('button').attributes('aria-describedby')!.split(' ')
    expect(described.map((id) => document.getElementById(id)?.textContent)).toEqual(['把 .stbackup 拖到這裡'])
    const files = [new File(['a'], 'a.stbackup'), new File(['b'], 'b.stbackup')]
    const drop = (el: ReturnType<typeof wrapper.get>) => {
      const e = new Event('drop', { cancelable: true })
      Object.defineProperty(e, 'dataTransfer', { value: { files } })
      el.element.dispatchEvent(e)
    }
    drop(wrapper.get('.drop'))
    expect(wrapper.emitted('pick')).toEqual([[[files[0]]]])

    // Clicking anywhere in the zone, its button included, opens the picker once.
    const click = vi.spyOn(wrapper.get<HTMLInputElement>('input').element, 'click')
    await wrapper.get('button').trigger('click')
    await wrapper.get('.drop p').trigger('click')
    expect(click).toHaveBeenCalledTimes(2)

    // Dragging over its own text keeps it lit; leaving the zone does not.
    const zone = wrapper.get('.drop')
    await zone.trigger('dragover')
    const leave = (to: Element | null) => {
      const e = new Event('dragleave')
      Object.defineProperty(e, 'relatedTarget', { value: to })
      zone.element.dispatchEvent(e)
    }
    leave(zone.get('p').element)
    await wrapper.vm.$nextTick()
    expect(zone.classes()).toContain('over')
    leave(document.body)
    await wrapper.vm.$nextTick()
    expect(zone.classes()).not.toContain('over')
    await wrapper.setProps({ disabled: true })
    drop(wrapper.get('.drop'))
    expect(wrapper.emitted('pick')).toHaveLength(1)
    wrapper.unmount()
  })
})
