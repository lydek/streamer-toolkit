// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import { measureUnmeasured } from './media'
import MediaLibrary from './MediaLibrary.vue'

const t = i18n.global.t

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
let items: Schemas['MediaItem'][]
let post: ReturnType<typeof vi.fn>, put: ReturnType<typeof vi.fn>, del: ReturnType<typeof vi.fn>

const gif: Schemas['MediaItem'] = { id: 'g1', title: '貓咪跳舞', format: 'gif', duration_ms: 5000, url: '/media/g1', missing: false }
const video: Schemas['MediaItem'] = { id: 'v1', title: '驚嚇', format: 'webm', duration_ms: null, url: '/media/v1', missing: false }

beforeEach(() => {
  items = [structuredClone(gif)]
  vi.spyOn(client, 'GET').mockImplementation((() => ok(structuredClone(items))) as never)
  post = vi.fn((_: string, init: { params: { query: { name: string } } }) => {
    const added = { ...video, title: init.params.query.name }
    items.push(added)
    return ok(added)
  })
  put = vi.fn((_: string, init: { params: { path: { id: string } }; body: Schemas['MediaUpdate'] }) => {
    const item = items.find((i) => i.id === init.params.path.id)
    if (item && init.body.duration_ms) item.duration_ms = init.body.duration_ms
    return ok(item ?? { ...video, ...init.body, id: init.params.path.id })
  })
  del = vi.fn((_: string, init: { params: { path: { id: string } } }) => {
    items = items.filter((i) => i.id !== init.params.path.id)
    return ok()
  })
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  vi.spyOn(client, 'DELETE').mockImplementation(del as never)
})
enableAutoUnmount(afterEach)
afterEach(() => vi.restoreAllMocks())

it('saves the lengths it can read and leaves the rest for next time', async () => {
  const measure = vi.fn(async (url: string) => (url === '/media/v1' ? 3200 : undefined))
  const saved = await measureUnmeasured([gif, video, { ...video, id: 'v2', url: '/media/v2' }, { ...video, id: 'v3', missing: true }], measure)
  expect(saved).toBe(true)
  expect(measure).toHaveBeenCalledTimes(2)
  expect(put).toHaveBeenCalledTimes(1)
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/media/{id}', {
    params: { path: { id: 'v1' } },
    body: { title: '驚嚇', duration_ms: 3200 },
  })
})

it('imports files, sets a GIF length and deletes after a second press', async () => {
  const wrapper = mount(MediaLibrary, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  expect(wrapper.text()).toContain('貓咪跳舞')

  const input = wrapper.get<HTMLInputElement>('#media-file')
  Object.defineProperty(input.element, 'files', { value: [new File(['GIF89a'], 'dance.gif')] })
  await input.trigger('change')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/screen-tools/media', expect.objectContaining({ params: { query: { name: 'dance.gif' } } }))
  expect(wrapper.emitted('imported')).toEqual([[1]])
  expect(wrapper.text()).toContain('dance.gif')

  const seconds = wrapper.get('[data-media="g1"] input')
  await seconds.setValue('8')
  await seconds.trigger('blur')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/media/{id}', {
    params: { path: { id: 'g1' } },
    body: { title: '貓咪跳舞', duration_ms: 8000 },
  })

  await wrapper.get(`[data-media="g1"] button[aria-label="${t('screenTools.media.delete', { title: '貓咪跳舞' })}"]`).trigger('click')
  expect(del).not.toHaveBeenCalled()
  await wrapper.findAll('[data-media="g1"] button').find((b) => b.text() === t('screenTools.media.confirmDelete'))!.trigger('click')
  await flushPromises()
  expect(del).toHaveBeenCalledTimes(1)
  expect(wrapper.find('[data-media="g1"]').exists()).toBe(false)
})
