// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import CloseHint from './CloseHint.vue'

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

let emit: (show: boolean) => void

async function open() {
  const wrapper = mount(CloseHint, { global: { plugins: [i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  emit(true)
  await flushPromises()
  return { wrapper, dialog: wrapper.get('dialog').element as HTMLDialogElement }
}

beforeEach(() => {
  invoke.mockReset().mockResolvedValue(undefined)
  listen.mockReset().mockImplementation(async (_event: string, handler: (e: { payload: boolean }) => void) => {
    emit = (show) => handler({ payload: show })
    return () => {}
  })
  // jsdom has no modal dialogs; the real behavior is checked by hand (docs/dev.md).
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true
  }
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement, value?: string) {
    if (value !== undefined) this.returnValue = value
    this.open = false
    this.dispatchEvent(new Event('close'))
  }
})

describe('CloseHint', () => {
  it('opens on the event and reports it was shown', async () => {
    const { dialog } = await open()
    expect(listen).toHaveBeenCalledWith('close-hint', expect.any(Function))
    expect(dialog.open).toBe(true)
    expect(invoke).toHaveBeenCalledWith('close_hint', { action: 'shown' })
  })

  it('sends hide with the "don\'t show again" choice', async () => {
    const { wrapper, dialog } = await open()
    await wrapper.get('input[type=checkbox]').setValue(true)
    await wrapper.get('button').trigger('click')
    expect(dialog.open).toBe(false)
    expect(invoke).toHaveBeenLastCalledWith('close_hint', { action: { hide: { dont_ask_again: true } } })
    expect(invoke).not.toHaveBeenCalledWith('close_hint', { action: 'cancel' })
  })

  it('sends cancel when the dialog closes on its own (Esc)', async () => {
    const { dialog } = await open()
    dialog.close()
    expect(invoke).toHaveBeenLastCalledWith('close_hint', { action: 'cancel' })
  })

  it('closes without cancel when Rust already hid the window', async () => {
    const { dialog } = await open()
    emit(false)
    expect(dialog.open).toBe(false)
    expect(invoke).not.toHaveBeenCalledWith('close_hint', { action: 'cancel' })
  })
})
