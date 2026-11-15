// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import type { ChatEvent, Incoming } from './chat'
import ChatOverlay from './ChatOverlay.vue'

vi.mock('./template', async (original) => {
  const real = await original<typeof import('./template')>()
  const html = '<ul data-chat-list></ul><template data-chat-message><li data-field="text"></li></template>'
  return { ...real, loadTemplate: async (base: string, rev: string) => real.parseTemplate(html, base, rev)! }
})

const chat = (n: number): ChatEvent => ({
  id: `c${n}`,
  type: 'chat.message',
  version: 1,
  seq: n,
  occurredAt: '',
  source: 'simulation',
  payload: { userId: '7', userLogin: 'v', userName: 'V', color: null, text: `m${n}`, fragments: [] },
})
const snapshot = (...ns: number[]): Incoming => ({
  type: 'snapshot',
  version: 1,
  seq: 0,
  payload: { 'chat.message': ns.map(chat), 'obs.status': [] },
})
const post = (data: Incoming, init: MessageEventInit = {}) =>
  window.dispatchEvent(new MessageEvent('message', { data, source: window.parent, origin: location.origin, ...init }))

afterEach(() => {
  vi.unstubAllGlobals()
  history.replaceState(null, '', '/')
})

describe('ChatOverlay preview mode', () => {
  it('opens no WebSocket and renders only messages from the parent window on the same origin', async () => {
    const sockets = vi.fn()
    vi.stubGlobal('WebSocket', sockets)
    history.replaceState(null, '', '/overlay/chat/?preview')
    const wrapper = mount(ChatOverlay, { global: { plugins: [i18n] }, attachTo: document.body })
    expect(sockets).not.toHaveBeenCalled()

    post(snapshot(1), { origin: 'http://evil.example' })
    post(snapshot(2), { source: null })
    await flushPromises()
    expect(wrapper.findAll('li')).toHaveLength(0)

    post(snapshot(3))
    await flushPromises()
    expect(wrapper.findAll('li').map((li) => li.text())).toEqual(['m3'])
    post(chat(4))
    await flushPromises()
    expect(wrapper.findAll('li').map((li) => li.text())).toEqual(['m3', 'm4'])
    wrapper.unmount()
  })

  it('connects to the Host outside preview mode', () => {
    const sockets = vi.fn(function (this: { close: () => void }) {
      this.close = () => {}
    })
    vi.stubGlobal('WebSocket', sockets)
    history.replaceState(null, '', '/overlay/chat/')
    const wrapper = mount(ChatOverlay, { global: { plugins: [i18n] } })
    expect(sockets).toHaveBeenCalledOnce()
    wrapper.unmount()
  })
})
