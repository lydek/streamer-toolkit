// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import ImageSizesPage from './ImageSizesPage.vue'
import { IMAGE_SIZES_MAX_BYTES, zipName } from './imageSizes'
import { testRouter } from './testRouter'

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))

const image = (px: number, tooLarge = false) => ({ px, bytes: 2048, data_url: 'data:image/png;base64,AA==', too_large: tooLarge })

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'POST').mockResolvedValue({
    data: { images: [image(72, true), image(36), image(18)] },
    response: new Response(null, { status: 200 }),
  } as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

async function page() {
  const wrapper = mount(ImageSizesPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

async function pick(wrapper: Awaited<ReturnType<typeof page>>, file: File) {
  const input = wrapper.find('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
  await input.trigger('change')
  await flushPromises()
}

it('shows only the latest choice when an earlier answer comes back later', async () => {
  const wrapper = await page()
  let slow: (answer: unknown) => void = () => {}
  vi.mocked(client.POST).mockImplementationOnce(() => new Promise((r) => (slow = r)) as never)
  vi.mocked(client.POST).mockResolvedValueOnce({
    data: { images: [image(36)] },
    response: new Response(null, { status: 200 }),
  } as never)
  const input = wrapper.find('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: [new File(['png'], 'logo.png', { type: 'image/png' })], configurable: true })
  await input.trigger('change')
  await wrapper.findAll('[aria-labelledby="is-kind"] button')[1]!.trigger('click')
  await flushPromises()
  slow({ data: { images: [image(72)] }, response: new Response(null, { status: 200 }) })
  await flushPromises()
  expect(wrapper.findAll('.sheet.dark figcaption').map((c) => c.text())).toEqual(['36 px · 2.0 KB'])
})

it('sends the picture with the chosen size and fit, and shows each size on dark and light with the over-limit one in red', async () => {
  const wrapper = await page()
  await pick(wrapper, new File(['png'], 'logo.png', { type: 'image/png' }))
  expect(client.POST).toHaveBeenCalledWith(
    '/api/admin/image-sizes',
    expect.objectContaining({ params: { query: { kind: 'badge', fit: 'pad', zip: false } } }),
  )
  expect(wrapper.findAll('.sheet')).toHaveLength(2)
  expect(wrapper.findAll('.sheet.dark img')).toHaveLength(3)
  expect(wrapper.findAll('figcaption.over').map((c) => c.text())).toEqual(['72 px · 2.0 KB', '72 px · 2.0 KB'])
  expect(wrapper.text()).toContain('25 KB')

  // A new choice works the same picture out again.
  await wrapper.findAll('[aria-labelledby="is-fit"] button').find((b) => b.text() === '裁切置中')!.trigger('click')
  await flushPromises()
  expect(client.POST).toHaveBeenLastCalledWith(
    '/api/admin/image-sizes',
    expect.objectContaining({ params: { query: { kind: 'badge', fit: 'crop', zip: false } } }),
  )
})

it('refuses a picture over 10 MB without sending it', async () => {
  const wrapper = await page()
  const big = new File(['x'], 'big.png', { type: 'image/png' })
  Object.defineProperty(big, 'size', { value: IMAGE_SIZES_MAX_BYTES + 1 })
  await pick(wrapper, big)
  expect(client.POST).not.toHaveBeenCalled()
  expect(wrapper.find('[role="alert"]').text()).toContain('10 MB')
})

it('names the zip after the picture and the size', () => {
  expect(zipName('my.logo.png', 'badge')).toBe('my.logo-badge.zip')
  expect(zipName('.png', 'points')).toBe('image-points.zip')
})
