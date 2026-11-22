// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import ImageSizesPage from './ImageSizesPage.vue'
import { IMAGE_SIZES_MAX_BYTES, zipName } from './imageSizes'
import { testRouter } from './testRouter'

const t = i18n.global.t

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
  expect(wrapper.findAll('.sheet.dark figcaption').map((c) => c.text())).toEqual([t('imageSizes.size', { px: 36, kb: '2.0' })])
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
  expect(wrapper.findAll('figcaption.over').map((c) => c.text())).toEqual([t('imageSizes.size', { px: 72, kb: '2.0' }), t('imageSizes.size', { px: 72, kb: '2.0' })])
  expect(wrapper.text()).toContain(t('imageSizes.tooLarge'))

  // A new choice works the same picture out again.
  await wrapper.findAll('[aria-labelledby="is-fit"] button').find((b) => b.text() === t('imageSizes.fits.crop'))!.trigger('click')
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
  expect(wrapper.find('[role="alert"]').text()).toContain(t('errors.image_sizes.too_large'))
})

it("drops the last picture's sizes for a picked square until the new picture is read, and says when it is not", async () => {
  const wrapper = await page()
  await pick(wrapper, new File(['png'], 'logo.png', { type: 'image/png' }))
  await wrapper.findAll('[aria-labelledby="is-fit"] button').find((b) => b.text() === t('imageSizes.fits.manual'))!.trigger('click')
  await flushPromises()
  // The browser has not read the picture's size yet (jsdom never does): nothing to show or download.
  expect(wrapper.findAll('.sheet')).toHaveLength(0)
  const download = wrapper.findAll('button').find((b) => b.text() === t('imageSizes.download'))!
  expect(download.attributes('disabled')).toBeDefined()

  const big = new File(['x'], 'big.png', { type: 'image/png' })
  Object.defineProperty(big, 'size', { value: IMAGE_SIZES_MAX_BYTES + 1 })
  await pick(wrapper, big)
  expect(wrapper.find('[role="alert"]').text()).toContain(t('errors.image_sizes.too_large'))
})

it('names the zip after the picture and the size', () => {
  expect(zipName('my.logo.png', 'badge')).toBe('my.logo-badge.zip')
  expect(zipName('.png', 'points')).toBe('image-points.zip')
})
