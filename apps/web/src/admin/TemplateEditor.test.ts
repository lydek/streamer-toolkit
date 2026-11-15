// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { h } from 'vue'
import { RouterView, createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import { ApiError } from './api'
import TemplateEditor from './TemplateEditor.vue'

const calls = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
  listener: undefined as ((msg: Incoming) => void) | undefined,
}))

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: calls.get, PUT: calls.put },
  api: (p: Promise<unknown>) => p,
}))
vi.mock('./home', () => ({
  useLiveEvents: (_ms: number, onMessage: (msg: Incoming) => void) => (calls.listener = onMessage),
}))

const source = { html: '<ul data-chat-list></ul>', css: '.a{}', revision: 'r1' }

beforeEach(() => {
  calls.get.mockReset()
  calls.put.mockReset()
  calls.get.mockImplementation(async (path: string) =>
    path === '/api/admin/templates/chat/source' ? source : { error: null },
  )
})

async function setup() {
  // Rendered by a router, as on the full page, so its leave guard is active.
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { render: () => null } },
      { path: '/chat', component: TemplateEditor },
    ],
  })
  await router.push('/chat')
  const wrapper = mount({ render: () => h(RouterView) }, {
    global: { plugins: [i18n, PrimeVue, router] },
    attachTo: document.body,
  })
  await flushPromises()
  return Object.assign(wrapper, { router })
}

describe('TemplateEditor', () => {
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

  it('saves with the revision it loaded and stops on a conflict', async () => {
    const wrapper = await setup()
    await wrapper.get('#template-css').setValue('.b{}')
    calls.put.mockRejectedValueOnce(new ApiError('template.conflict'))
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(calls.put).toHaveBeenCalledWith('/api/admin/templates/chat/source', {
      body: { html: source.html, css: '.b{}', revision: 'r1' },
    })
    expect(wrapper.get('[role="alert"]').text()).toBe(i18n.global.t('errors.template.conflict'))
    // The edit stays until the user chooses to reload.
    expect((wrapper.get('#template-css').element as HTMLTextAreaElement).value).toBe('.b{}')
    expect(wrapper.text()).toContain(i18n.global.t('editor.reload'))

    calls.put.mockResolvedValueOnce({ ...source, css: '.b{}', revision: 'r2' })
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('[role="status"]').text()).toBe(i18n.global.t('editor.saved'))
    wrapper.unmount()
  })

  it('keeps what is typed while a save is in flight', async () => {
    const wrapper = await setup()
    await wrapper.get('#template-html').setValue('first')
    let finish!: (v: object) => void
    calls.put.mockReturnValueOnce(new Promise((r) => (finish = r)))
    await wrapper.get('form').trigger('submit')
    await wrapper.get('#template-html').setValue('typed')
    finish({ html: 'first', css: source.css, revision: 'r2' })
    await flushPromises()
    expect((wrapper.get('#template-html').element as HTMLTextAreaElement).value).toBe('typed')
    // Still differs from what was saved, so it can be saved again.
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeUndefined()
    wrapper.unmount()
  })

  it('asks before leaving with unsaved edits', async () => {
    const wrapper = await setup()
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    await wrapper.get('#template-html').setValue('edited')
    await wrapper.router.push('/')
    expect(confirm).toHaveBeenCalledOnce()
    expect(wrapper.router.currentRoute.value.path).toBe('/chat')
    confirm.mockReturnValue(true)
    await wrapper.router.push('/')
    expect(wrapper.router.currentRoute.value.path).toBe('/')
    confirm.mockRestore()
    wrapper.unmount()
  })

  it('leaves without asking when nothing changed', async () => {
    const wrapper = await setup()
    const confirm = vi.spyOn(window, 'confirm')
    await wrapper.router.push('/')
    expect(confirm).not.toHaveBeenCalled()
    expect(wrapper.router.currentRoute.value.path).toBe('/')
    confirm.mockRestore()
    wrapper.unmount()
  })

  it('keeps CRLF line endings of files saved with them', async () => {
    calls.get.mockImplementation(async (path: string) =>
      path === '/api/admin/templates/chat/source' ? { html: 'a\r\nb', css: 'c\nd', revision: 'r1' } : { error: null },
    )
    const wrapper = await setup()
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeDefined()
    await wrapper.get('#template-html').setValue('a\nb\nx')
    calls.put.mockImplementation(async (_p: string, { body }: { body: object }) => ({ ...body, revision: 'r2' }))
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(calls.put.mock.calls[0][1].body).toMatchObject({ html: 'a\r\nb\r\nx', css: 'c\nd' })
    // Saved: nothing left to save until the next edit.
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeDefined()
    wrapper.unmount()
  })
})
