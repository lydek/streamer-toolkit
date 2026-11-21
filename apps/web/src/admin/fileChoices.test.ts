// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { ApiError } from './api'
import BackupRestore from './BackupRestore.vue'
import CheckinImport from './CheckinImport.vue'

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))
// GET: the backup page's sizes, which these tests do not look at.
const { client } = vi.hoisted(() => ({ client: { POST: vi.fn(), GET: vi.fn(() => new Promise(() => {})) } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))
const { previewRestore } = vi.hoisted(() => ({ previewRestore: vi.fn() }))
vi.mock('./backup', async (original) => ({ ...(await original<typeof import('./backup')>()), previewRestore }))

const ok = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })

/** Picks `file` in the component's file input; returns what the input's value was last set to. */
async function choose(input: HTMLInputElement, file: File) {
  const set: string[] = []
  Object.defineProperty(input, 'files', { configurable: true, value: [file] })
  Object.defineProperty(input, 'value', { configurable: true, get: () => 'C:\\fakepath\\x', set: (v) => set.push(v) })
  input.dispatchEvent(new Event('change'))
  await flushPromises()
  return set
}

beforeEach(() => client.POST.mockReset())
afterEach(() => (document.body.innerHTML = ''))

// A browser fires no change for the file already chosen: a refused one is cleared, so it can be chosen again once
// fixed.
describe('a refused file can be chosen again', () => {
  it('in the check-in import', async () => {
    const wrapper = mount(CheckinImport, { global: { plugins: [i18n, PrimeVue] } })
    const input = wrapper.get('input[type="file"]').element as HTMLInputElement
    expect(await choose(input, new File(['{oops'], 'bad.json')), 'not JSON').toEqual([''])
    client.POST.mockRejectedValueOnce(new ApiError('checkin.import_invalid'))
    expect(await choose(input, new File(['{}'], 'refused.json')), 'refused by the Host').toEqual([''])
  })

  it('in the backup restore', async () => {
    const wrapper = mount(BackupRestore, { global: { plugins: [i18n, PrimeVue] } })
    const input = wrapper.get('input[type="file"]').element as HTMLInputElement
    previewRestore.mockRejectedValueOnce(new ApiError('backup.invalid'))
    expect(await choose(input, new File(['x'], 'bad.stbackup'))).toEqual([''])
    expect(wrapper.get('[role="alert"]').text()).toBe(i18n.global.t('errors.backup.invalid'))
  })
})

describe('CheckinImport', () => {
  it('moves focus to the file field once the import is done and its button is gone', async () => {
    const wrapper = mount(CheckinImport, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
    const input = wrapper.get('input[type="file"]').element as HTMLInputElement
    client.POST.mockResolvedValueOnce(ok({ added: 1, updated: 0, skipped: [] }))
    await choose(input, new File(['{"1":{"totalCheckIns":1}}'], 'ok.json'))
    const confirm = wrapper.get('button')
    ;(confirm.element as HTMLButtonElement).focus()

    client.POST.mockResolvedValueOnce(ok({ added: 1, updated: 0 }))
    await confirm.trigger('click')
    await flushPromises()
    expect(wrapper.find('button').exists()).toBe(false)
    expect(document.activeElement).toBe(input)
  })
})
