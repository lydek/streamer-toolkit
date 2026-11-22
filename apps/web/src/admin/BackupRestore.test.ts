// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import BackupRestore from './BackupRestore.vue'

const { previewRestore, saveBackup, toastAdd } = vi.hoisted(() => ({
  previewRestore: vi.fn(),
  saveBackup: vi.fn(async (): Promise<string | false> => 'mine.stbackup'),
  toastAdd: vi.fn(),
}))
vi.mock('./backup', async (original) => ({ ...(await original<typeof import('./backup')>()), previewRestore, saveBackup }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

beforeEach(() => {
  vi.spyOn(client, 'GET').mockResolvedValue({
    data: { data: 3 * 1024 ** 2, media: 2 * 1024 ** 3 },
    response: new Response(null, { status: 200 }),
  } as never)
})
enableAutoUnmount(afterEach)
afterEach(() => vi.restoreAllMocks())

it('shows each choice with its size, exports the chosen kind and names the saved file', async () => {
  const wrapper = mount(BackupRestore, { global: { plugins: [i18n, PrimeVue] } })
  await flushPromises()
  const choices = wrapper.findAll('[aria-labelledby="backup-choice"] button')
  expect(choices.map((b) => b.text())).toEqual(['全部 2.0 GB', '資料 3.0 MB', '媒體 2.0 GB'])
  await choices.find((b) => b.text().startsWith('媒體'))!.trigger('click')
  await wrapper.findAll('button').find((b) => b.text() === '匯出備份')!.trigger('click')
  await flushPromises()
  expect(saveBackup).toHaveBeenCalledWith('media')
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: '已匯出備份：mine.stbackup' }))
})

it('says which kinds a backup holds and that only those are replaced', async () => {
  previewRestore.mockResolvedValueOnce({
    created_at: '2026-11-22T00:00:00Z',
    app_version: '1.2.3',
    redemptions: 0,
    checkins: 0,
    templates: 1,
    sounds: 2,
    music: 0,
    media: 0,
    contents: ['media'],
  })
  const wrapper = mount(BackupRestore, { global: { plugins: [i18n, PrimeVue] } })
  const input = wrapper.get('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: [new File(['x'], 'm.stbackup')], configurable: true })
  await input.trigger('change')
  await flushPromises()
  expect(wrapper.text()).toContain('包含：媒體')
  expect(wrapper.text()).toContain('音效 2 個')
  expect(wrapper.text()).not.toContain('兌換紀錄')
  expect(wrapper.text()).toContain('設定與紀錄不動')
  expect(wrapper.text()).toContain('播放清單與獎勵的連結不會回來')
})
