// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import ChatPreview from './ChatPreview.vue'

const calls = vi.hoisted(() => ({
  get: vi.fn(),
  listener: undefined as ((msg: Incoming) => void) | undefined,
}))

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: calls.get },
  api: (p: Promise<unknown>) => p,
}))
vi.mock('./home', () => ({
  useLiveEvents: (_ms: number, onMessage: (msg: Incoming) => void) => (calls.listener = onMessage),
}))

beforeEach(() => {
  calls.get.mockReset()
  calls.get.mockResolvedValue({ error: null })
})

async function setup() {
  const wrapper = mount(ChatPreview, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

describe('ChatPreview', () => {
  it('previews in an iframe sandboxed with allow-scripts only', async () => {
    const wrapper = await setup()
    const frame = wrapper.get('iframe').element as HTMLIFrameElement
    // Without allow-same-origin the preview has the null origin: Tauri IPC (and so `invoke`) and the admin
    // token stay out of its reach. Any other token (allow-same-origin, allow-top-navigation...) widens that.
    expect(frame.getAttribute('sandbox')?.trim().split(/\s+/)).toEqual(['allow-scripts'])
    expect(frame.getAttribute('src')).toBe('/overlay/chat/?preview')
    expect(document.querySelector('[data-chat-list]')).toBeNull()
    wrapper.unmount()
  })

  it('posts sample messages to the preview while the chat is empty, then live events', async () => {
    const wrapper = await setup()
    const frame = wrapper.get('iframe').element as HTMLIFrameElement
    const post = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => {})
    await wrapper.get('iframe').trigger('load')
    const first = post.mock.calls[0][0] as Extract<Incoming, { type: 'snapshot' }>
    expect(first.type).toBe('snapshot')
    expect(first.payload['chat.message'].length).toBeGreaterThan(0)
    expect(first.payload['chat.message'].every((m) => m.source === 'simulation')).toBe(true)

    const event: Incoming = {
      id: 'c1',
      type: 'chat.message',
      version: 1,
      seq: 1,
      occurredAt: '',
      source: 'twitch',
      payload: { userId: '7', userLogin: 'v', userName: 'V', color: null, text: 'hi', fragments: [] },
    }
    calls.listener!(event)
    expect(post).toHaveBeenLastCalledWith(event, '*')
    await wrapper.get('iframe').trigger('load')
    const reloaded = post.mock.calls.at(-1)![0] as Extract<Incoming, { type: 'snapshot' }>
    expect(reloaded.payload['chat.message']).toEqual([event])
    wrapper.unmount()
  })

  it('shows why the user template is not in use, and checks again when it changes', async () => {
    calls.get.mockResolvedValueOnce({ error: 'template.invalid' })
    const wrapper = await setup()
    expect(calls.get).toHaveBeenCalledWith('/api/admin/templates/chat')
    expect(wrapper.get('[role="alert"]').text()).toBe(i18n.global.t('errors.template.invalid'))
    calls.listener!({ type: 'template.changed', id: 't', version: 1, seq: 2, occurredAt: '', source: 'system', payload: { overlay: 'chat' } })
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    wrapper.unmount()
  })
})
