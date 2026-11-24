// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import BackupRestore from './BackupRestore.vue'

const t = i18n.global.t

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
  // Literal on purpose: the sizes' wording and rounding are what this checks.
  expect(choices.map((b) => b.text())).toEqual(['全部 2.0 GB', '資料 3.0 MB', '媒體 2.0 GB'])
  await choices.find((b) => b.text().startsWith(t('backup.choices.media')))!.trigger('click')
  await wrapper.findAll('button').find((b) => b.text() === t('backup.export'))!.trigger('click')
  await flushPromises()
  expect(saveBackup).toHaveBeenCalledWith('media')
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ summary: t('backup.exported', { file: 'mine.stbackup' }) }))
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
  expect(wrapper.text()).toContain(t('backup.contains', { kinds: t('backup.choices.media') }))
  expect(wrapper.text()).toContain(t('backup.files', { templates: 1, sounds: 2, music: 0, media: 0 }))
  expect(wrapper.text()).not.toContain(t('backup.records', { redemptions: 0, checkins: 0 }))
  expect(wrapper.text()).toContain(t('backup.restoreOnly.media'))
})

it('shows the upload, then the check, and cancelling stops it with no error and lets the same file be chosen again', async () => {
  let report!: (p: { phase: 'upload' | 'check'; sent: number; total: number }) => void
  let aborted: AbortSignal | undefined
  previewRestore.mockImplementationOnce(
    (_file: File, onProgress: typeof report, signal: AbortSignal) =>
      new Promise((_, reject) => {
        report = onProgress
        aborted = signal
        signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))
      }),
  )
  const wrapper = mount(BackupRestore, { global: { plugins: [i18n, PrimeVue] } })
  const input = wrapper.get<HTMLInputElement>('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: [new File(['x'], 'big.stbackup')], configurable: true })
  await input.trigger('change')
  report({ phase: 'upload', sent: 1.2 * 1024 ** 3, total: 3.4 * 1024 ** 3 })
  await flushPromises()
  // Literal on purpose: both numbers in the total's unit.
  expect(wrapper.get('.progress [role="status"]').text()).toBe('上傳中 1.2 / 3.4 GB')
  expect(input.attributes('disabled')).toBeDefined()
  report({ phase: 'check', sent: 3.4 * 1024 ** 3, total: 3.4 * 1024 ** 3 })
  await flushPromises()
  expect(wrapper.get('.progress [role="status"]').text()).toBe(t('backup.checking'))
  await wrapper.get('.progress button').trigger('click')
  await flushPromises()
  expect(aborted?.aborted).toBe(true)
  expect(wrapper.find('.progress').exists()).toBe(false)
  expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  expect(input.attributes('disabled')).toBeUndefined()
  expect(input.element.value).toBe('')
})
