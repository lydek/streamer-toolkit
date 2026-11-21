// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import type { Schemas } from './api'
import CardPreview from './CardPreview.vue'

enableAutoUnmount(afterEach)

const style: Schemas['CardStyleBody'] = {
  preset: 'journal',
  background_color: '#fbf6ee',
  background_image: null,
  stamp_color: '#c0392b',
  stamp_image: 'img.png',
  per_row: 5,
  title: '#觀眾# 打卡',
  description: '#章數#/#格數#',
  animation: 'pop',
  show_repeat: false,
}

it('sends each sandboxed preview a sample card in the form’s style, again after a change', async () => {
  const wrapper = mount(CardPreview, { props: { style, slots: 10 }, global: { plugins: [i18n] }, attachTo: document.body })
  const frames = wrapper.findAll('iframe')
  expect(frames.map((f) => f.attributes('sandbox'))).toEqual(['allow-scripts', 'allow-scripts'])
  expect(frames[0].attributes('src')).toBe('/overlay/checkin/?preview')
  const posts = frames.map((f) => vi.spyOn((f.element as HTMLIFrameElement).contentWindow!, 'postMessage').mockImplementation(() => {}))
  await frames[0].trigger('load')
  const card = posts[0].mock.calls.find(([m]) => (m as { type: string }).type === 'checkin.stamped')![0] as {
    payload: Schemas['CheckinStampedPayload']
  }
  expect(card.payload.title).toBe('小魚 打卡')
  expect(card.payload.description).toBe('2/10')
  expect(card.payload.style.stampUrl).toBe('/checkin/img.png')
  expect(posts[0].mock.calls[0][1]).toBe('*')

  await wrapper.setProps({ style: { ...style, title: '換了' } })
  await flushPromises()
  const titles = posts[1].mock.calls.flatMap(([m]) => {
    const msg = m as { type: string; payload: Schemas['CheckinStampedPayload'] }
    return msg.type === 'checkin.stamped' ? [msg.payload.title] : []
  })
  expect(titles).toContain('換了')
})
