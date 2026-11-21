// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import OutputDevicePicker from './OutputDevicePicker.vue'
import { chooseOutput, outputDevice } from './outputDevice'

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('navigator', {
    mediaDevices: {
      enumerateDevices: () => Promise.resolve([{ kind: 'audiooutput', deviceId: 'cable', label: 'CABLE Input' }]),
    },
  })
})
enableAutoUnmount(afterEach)
afterEach(() => {
  chooseOutput(undefined)
  vi.unstubAllGlobals()
})

it('shows the default device as chosen until another is picked, and back', async () => {
  const wrapper = mount(OutputDevicePicker, { global: { plugins: [i18n, PrimeVue] } })
  await flushPromises()
  expect(wrapper.get('.p-select-label').text()).toBe('系統預設')

  wrapper.getComponent({ name: 'Select' }).vm.$emit('update:modelValue', 'cable')
  await flushPromises()
  expect(outputDevice.value).toEqual({ id: 'cable', label: 'CABLE Input' })
  expect(wrapper.get('.p-select-label').text()).toBe('CABLE Input')

  wrapper.getComponent({ name: 'Select' }).vm.$emit('update:modelValue', 'default')
  await flushPromises()
  expect(outputDevice.value).toBeUndefined()
  expect(wrapper.get('.p-select-label').text()).toBe('系統預設')
})
