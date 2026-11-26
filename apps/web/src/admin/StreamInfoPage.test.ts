// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import StreamInfoPage from './StreamInfoPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const refused = (code: string) => Promise.resolve({ error: { code }, response: new Response(null, { status: 400 }) })
let put: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    path === '/api/admin/stream-info'
      ? ok({ title: '週二點歌夜', category: { id: '26936', name: 'Music' }, tags: ['中文'], language: 'zh' })
      : ok([{ id: '509658', name: 'Just Chatting' }])) as never)
  put = vi.fn(() => ok())
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(StreamInfoPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}
const update = (w: Awaited<ReturnType<typeof open>>) => w.findAll('button').find((b) => b.text() === t('streamInfo.update'))!

it("shows what Twitch has, and sends the changes", async () => {
  const w = await open()
  expect((w.get('#si-title').element as HTMLInputElement).value).toBe('週二點歌夜')
  expect((w.get('#si-category').element as HTMLInputElement).value).toBe('Music')
  expect(w.text()).toContain('中文')
  await w.get('#si-title').setValue('雜談')
  await w.get('#si-tags').setValue('唱歌')
  await w.get('#si-tags').trigger('keydown', { key: 'Enter' })
  await w.get(`[aria-label="${t('streamInfo.removeTag', { tag: '中文' })}"]`).trigger('click')
  await update(w).trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/stream-info', {
    body: { title: '雜談', category_id: '26936', tags: ['唱歌'], language: 'zh' },
  })
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'success', summary: t('streamInfo.updated') }))
})

it('says why Twitch refused, and stays put while it cannot load', async () => {
  put.mockImplementation(() => refused('stream_info.refused'))
  const w = await open()
  await update(w).trigger('click')
  await flushPromises()
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.stream_info.refused') }))

  vi.spyOn(client, 'GET').mockImplementation((() => refused('twitch.sign_in_required')) as never)
  const failed = await open()
  expect(failed.get('[role="alert"]').text()).toBe(t('errors.twitch.sign_in_required'))
  expect(update(failed).attributes('disabled')).toBeDefined()
})

it('saves a tag still in the box, and keeps a category typed but not picked from being saved', async () => {
  const w = await open()
  await w.get('#si-tags').setValue('唱歌')
  await w.get('#si-category').setValue('Mus')
  await w.get('#si-category').trigger('change')
  await update(w).trigger('click')
  await flushPromises()
  expect(put).not.toHaveBeenCalled()
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('streamInfo.pickCategory') }))
  // Emptied: no category.
  await w.get('#si-category').setValue('')
  await update(w).trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/stream-info', {
    body: { title: '週二點歌夜', category_id: '', tags: ['中文', '唱歌'], language: 'zh' },
  })
})

it('fills the category list from the latest search only', async () => {
  const answers: Record<string, (v: unknown) => void> = {}
  vi.spyOn(client, 'GET').mockImplementation(((path: string, init?: { params: { query: { query: string } } }) =>
    path === '/api/admin/stream-info'
      ? ok({ title: 't', category: null, tags: [], language: 'zh' })
      : new Promise((resolve) => (answers[init!.params.query.query] = resolve))) as never)
  const w = await open()
  const box = w.findComponent({ name: 'AutoComplete' })
  box.vm.$emit('complete', { query: 'mu' })
  box.vm.$emit('complete', { query: 'music' })
  answers.music({ data: [{ id: '26936', name: 'Music' }], response: new Response(null, { status: 200 }) })
  await flushPromises()
  answers.mu({ data: [{ id: '1', name: 'Mummy' }], response: new Response(null, { status: 200 }) })
  await flushPromises()
  expect(box.props('suggestions')).toEqual([{ id: '26936', name: 'Music' }])
})
