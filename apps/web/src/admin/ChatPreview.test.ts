// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import Select from 'primevue/select'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import { ApiError } from './api'
import ChatPreview from './ChatPreview.vue'

const calls = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
  toast: vi.fn(),
  listener: undefined as ((msg: Incoming) => void) | undefined,
}))

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: calls.get, PUT: calls.put },
  api: (p: Promise<unknown>) => p,
}))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: calls.toast }) }))
vi.mock('./home', async (original) => ({
  ...(await original<typeof import('./home')>()),
  useLiveEvents: (_ms: number, onMessage: (msg: Incoming) => void) => (calls.listener = onMessage),
}))

beforeEach(() => {
  // PrimeVue's Select reads matchMedia, which jsdom lacks.
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  calls.get.mockReset()
  calls.get.mockResolvedValue({ selected: 'default', error: null })
  calls.put.mockReset()
  calls.put.mockResolvedValue(undefined)
  calls.toast.mockReset()
})

async function setup() {
  const wrapper = mount(ChatPreview, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

const frames = (wrapper: Awaited<ReturnType<typeof setup>>) =>
  wrapper.findAll('iframe').map((f) => f.element as HTMLIFrameElement)
const radio = (wrapper: Awaited<ReturnType<typeof setup>>, id: string) =>
  wrapper.get(`input[value="${id}"]`).element as HTMLInputElement

describe('ChatPreview', () => {
  it('previews each built-in template in an iframe sandboxed with allow-scripts only', async () => {
    const wrapper = await setup()
    // Without allow-same-origin a preview has the null origin: Tauri IPC (and so `invoke`) and the admin
    // token stay out of its reach. Any other token (allow-same-origin, allow-top-navigation...) widens that.
    for (const frame of frames(wrapper)) {
      expect(frame.getAttribute('sandbox')?.trim().split(/\s+/)).toEqual(['allow-scripts'])
    }
    expect(frames(wrapper).map((f) => f.getAttribute('src'))).toEqual(
      ['default', 'compact', 'bubble'].map((id) => `/overlay/chat/?preview&template=${id}`),
    )
    expect(document.querySelector('[data-chat-list]')).toBeNull()
    wrapper.unmount()
  })

  it('posts sample messages to a preview while the chat is empty, then live events to all', async () => {
    const wrapper = await setup()
    const [frame, ...others] = frames(wrapper)
    const post = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => {})
    const otherPosts = others.map((f) => vi.spyOn(f.contentWindow!, 'postMessage').mockImplementation(() => {}))
    await wrapper.get('iframe').trigger('load')
    const first = post.mock.calls[0][0] as Extract<Incoming, { type: 'snapshot' }>
    expect(first.type).toBe('snapshot')
    expect(first.payload['chat.message'].length).toBeGreaterThan(0)
    expect(first.payload['chat.message'].every((m) => m.source === 'simulation')).toBe(true)
    // Only the preview that loaded gets its snapshot.
    for (const other of otherPosts) expect(other).not.toHaveBeenCalled()

    const event: Incoming = {
      id: 'c1',
      type: 'chat.message',
      version: 1,
      seq: 1,
      occurredAt: '',
      source: 'twitch',
      payload: { userId: '7', userLogin: 'v', userName: 'V', color: null, text: 'hi', fragments: [], badges: [], highlighted: false },
    }
    calls.listener!(event)
    for (const p of [post, ...otherPosts]) expect(p).toHaveBeenLastCalledWith(event, '*')
    await wrapper.get('iframe').trigger('load')
    const reloaded = post.mock.calls.at(-1)![0] as Extract<Incoming, { type: 'snapshot' }>
    expect(reloaded.payload['chat.message']).toEqual([event])
    wrapper.unmount()
  })

  it('shows why the user template is not in use, and checks again when it changes', async () => {
    calls.get.mockResolvedValueOnce({ selected: 'custom', error: 'template.invalid' })
    const wrapper = await setup()
    expect(calls.get).toHaveBeenCalledWith('/api/admin/templates/chat')
    expect(wrapper.get('[role="alert"]').text()).toBe(i18n.global.t('errors.template.invalid'))
    expect(wrapper.findAll('input:checked')).toHaveLength(0)
    // Another overlay's change is not this picker's business.
    calls.listener!({ type: 'template.changed', id: 't', version: 1, seq: 2, occurredAt: '', source: 'system', payload: { overlay: 'alerts' } })
    await flushPromises()
    expect(calls.get).toHaveBeenCalledTimes(1)
    calls.listener!({ type: 'template.changed', id: 't', version: 1, seq: 3, occurredAt: '', source: 'system', payload: { overlay: 'chat' } })
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('checks the selected template and switches when another is picked', async () => {
    calls.get.mockResolvedValue({ selected: 'compact', error: null })
    const wrapper = await setup()
    expect(radio(wrapper, 'compact').checked).toBe(true)
    calls.get.mockResolvedValue({ selected: 'bubble', error: null })
    await wrapper.get('input[value="bubble"]').setValue(true)
    await flushPromises()
    expect(calls.put).toHaveBeenCalledWith('/api/admin/templates/chat', { body: { selected: 'bubble' } })
    expect(radio(wrapper, 'bubble').checked).toBe(true)
    expect(calls.toast).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('sends quick picks one at a time, skipping to the latest', async () => {
    const wrapper = await setup()
    let release!: () => void
    calls.put.mockImplementationOnce(() => new Promise<void>((done) => (release = done)))
    await wrapper.get('input[value="compact"]').setValue(true)
    await wrapper.get('input[value="bubble"]').setValue(true)
    await wrapper.get('input[value="default"]').setValue(true)
    expect(calls.put).toHaveBeenCalledTimes(1)
    release()
    await flushPromises()
    expect(calls.put.mock.calls.map((c) => c[1].body.selected)).toEqual(['compact', 'default'])
    wrapper.unmount()
  })

  it('reports a failed switch and shows the template still in use', async () => {
    const wrapper = await setup()
    calls.put.mockRejectedValueOnce(new ApiError('settings.save_failed'))
    await wrapper.get('input[value="bubble"]').setValue(true)
    await flushPromises()
    expect(calls.toast).toHaveBeenCalledWith(
      expect.objectContaining({ severity: 'error', summary: i18n.global.t('errors.settings.save_failed') }),
    )
    expect(radio(wrapper, 'default').checked).toBe(true)
    expect(radio(wrapper, 'bubble').checked).toBe(false)
    wrapper.unmount()
  })

  it('starts previews with the text size the Host announces', async () => {
    const wrapper = await setup()
    const size: Incoming = { type: 'chat.text_size', id: 's', version: 1, seq: 4, occurredAt: '', source: 'system', payload: { textSize: 32 } }
    const [frame] = frames(wrapper)
    const post = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => {})
    calls.listener!(size)
    await flushPromises()
    expect(post).toHaveBeenLastCalledWith(size, '*')
    await wrapper.get('iframe').trigger('load')
    const reloaded = post.mock.calls.at(-1)![0] as Extract<Incoming, { type: 'snapshot' }>
    expect(reloaded.payload['chat.text_size']).toEqual([size])
    // The template page only picks the template: the text size is the overlays' appearance now.
    expect(wrapper.findComponent(Select).exists()).toBe(false)
    wrapper.unmount()
  })

  it("starts previews with the overlays' appearance, as the overlay in OBS has it", async () => {
    const wrapper = await setup()
    const look: Incoming = { type: 'screen.appearance', id: 'a', version: 1, seq: 5, occurredAt: '', source: 'system', payload: { theme: 'contrast', textSize: 150 } }
    const [frame] = frames(wrapper)
    const post = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => {})
    calls.listener!(look)
    await flushPromises()
    await wrapper.get('iframe').trigger('load')
    const reloaded = post.mock.calls.at(-1)![0] as Extract<Incoming, { type: 'snapshot' }>
    expect(reloaded.payload['screen.appearance']).toEqual([look])
    wrapper.unmount()
  })
})
