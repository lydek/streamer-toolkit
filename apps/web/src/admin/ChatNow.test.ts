// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { i18n } from '../i18n'
import type { ChatEvent } from '../overlay/chat/chat'
import ChatNow from './ChatNow.vue'
import { type ChatLog, EMPTY_LOG, pause, receive } from './chatLog'

const chat = (n: number, payload: Partial<ChatEvent['payload']> = {}): ChatEvent => ({
  id: `c${n}`,
  type: 'chat.message',
  version: 1,
  seq: n,
  occurredAt: '2026-11-15T10:00:00.000Z',
  source: 'twitch',
  payload: { userId: '7', userLogin: 'v', userName: 'V', color: null, text: `m${n}`, fragments: [], badges: [], highlighted: false, ...payload },
})

function setup(log: ChatLog, broadcasterId?: string) {
  // The component emits on mount already, before mount() returns.
  let current = log
  const wrapper: ReturnType<typeof mount<typeof ChatNow>> = mount(ChatNow, {
    props: {
      log,
      broadcasterId,
      'onUpdate:log': (next: ChatLog) => {
        current = next
        void Promise.resolve().then(() => wrapper.setProps({ log: current }))
      },
    },
    global: { plugins: [i18n, PrimeVue, ToastService] },
    attachTo: document.body,
  })
  return wrapper
}

/** Fakes the layout jsdom does not do: content `height` px tall in a 100 px box. */
function layout(el: HTMLElement, height: number) {
  Object.defineProperty(el, 'scrollHeight', { configurable: true, value: height })
  Object.defineProperty(el, 'clientHeight', { configurable: true, value: 100 })
}

describe('ChatNow', () => {
  it('is a log that does not announce itself and renders viewer text as text', () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1, { userName: '<b>x</b>', text: '<img src=x onerror=alert(1)>' })]))
    const log = wrapper.get('[role="log"]')
    expect(log.attributes('aria-live')).toBe('off')
    expect(log.find('img').exists()).toBe(false)
    expect(log.find('b').exists()).toBe(false)
    expect(log.text()).toContain('<img src=x onerror=alert(1)>')
    wrapper.unmount()
  })

  it('badges the broadcaster only', () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1, { userId: '42' }), chat(2)]), '42')
    const msgs = wrapper.findAll('.msg')
    expect(msgs[0].find('.badge').text()).toBe(i18n.global.t('chat.broadcaster'))
    expect(msgs[1].find('.badge').exists()).toBe(false)
    wrapper.unmount()
  })

  it('colors names readably', () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1, { color: '#0000FF' })]))
    const style = wrapper.get('.name').attributes('style')
    expect(style).toMatch(/color:/)
    expect(style).not.toMatch(/rgb\(0, 0, 255\)|#0000ff/i)
    wrapper.unmount()
  })

  it('pauses on scrolling up, counts new messages and resumes at the bottom', async () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1), chat(2)]))
    const el = wrapper.get('[role="log"]').element as HTMLElement
    layout(el, 500)
    el.scrollTop = 400
    await wrapper.get('[role="log"]').trigger('scroll')
    el.scrollTop = 200
    await wrapper.get('[role="log"]').trigger('scroll')
    expect(wrapper.props('log').paused).toBe(true)

    await wrapper.setProps({ log: receive(wrapper.props('log'), [chat(3)]) })
    expect(wrapper.text()).toContain(i18n.global.t('chat.paused', { n: 1 }))
    expect(el.scrollTop).toBe(200)

    el.scrollTop = 400
    await wrapper.get('[role="log"]').trigger('scroll')
    expect(wrapper.props('log')).toMatchObject({ paused: false, unseen: 0 })
    wrapper.unmount()
  })

  it('ignores scroll events without movement, such as the drawer moving between aside and dialog', async () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1)]))
    const el = wrapper.get('[role="log"]').element as HTMLElement
    await flushPromises()
    layout(el, 500)
    // Content grew without a scroll to the end yet: not at the bottom, but the reader did not scroll up.
    await wrapper.get('[role="log"]').trigger('scroll')
    expect(wrapper.props('log').paused).toBe(false)
    wrapper.unmount()
  })

  it('announces a lost connection', async () => {
    const wrapper = setup(EMPTY_LOG)
    expect(wrapper.get('[role="status"]').text()).toBe('')
    await wrapper.setProps({ disconnected: true })
    expect(wrapper.get('[role="status"]').text()).toBe(i18n.global.t('chat.disconnected'))
    wrapper.unmount()
  })

  it('has a Pause / Jump to latest button', async () => {
    const wrapper = setup(receive(EMPTY_LOG, [chat(1)]))
    const el = wrapper.get('[role="log"]').element as HTMLElement
    layout(el, 500)
    await wrapper.get('button').trigger('click')
    expect(wrapper.props('log').paused).toBe(true)
    expect(wrapper.get('button').text()).toBe(i18n.global.t('chat.jump'))
    await wrapper.get('button').trigger('click')
    await nextTick()
    expect(wrapper.props('log').paused).toBe(false)
    expect(el.scrollTop).toBe(500)
    wrapper.unmount()
  })

  it('starts following when reopened', async () => {
    const wrapper = setup(pause(receive(EMPTY_LOG, [chat(1)])))
    await flushPromises()
    expect(wrapper.props('log').paused).toBe(false)
    wrapper.unmount()
  })
})
