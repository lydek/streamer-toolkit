// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { i18n } from '../i18n'
import FullPage from './FullPage.vue'
import { navOpened } from './page'
import { testRouter } from './testRouter'

const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))
const hiddenPages = vi.hoisted(() => ({ set: new Set<string>() }))
vi.mock('./features', async (original) => ({
  ...(await original<typeof import('./features')>()),
  useModules: () => ({ hidden: computed(() => hiddenPages.set) }),
}))

const page = (props: Record<string, string>) =>
  mount(FullPage, { props: { title: '背景音樂', ...props }, global: { plugins: [testRouter(), i18n, PrimeVue] } })

afterEach(() => {
  toastAdd.mockReset()
  hiddenPages.set = new Set()
})

describe('FullPage header', () => {
  it('shows the tile icon, the status and where the overlay is in OBS, linking to the OBS sources page', async () => {
    const wrapper = page({ tile: 'bgm', status: '播放中：Lo-fi', overlay: 'bgm' })
    await flushPromises()
    expect(wrapper.get('.badge').classes()).toContain('tint-yellow')
    expect(wrapper.get('.status').text()).toBe('播放中：Lo-fi')
    // No Host answers here, so OBS counts as not connected.
    const link = wrapper.get('.page-end a')
    expect(link.attributes('href')).toBe('/obs-sources')
    expect(link.text()).toBe('OBS 沒連上')
  })

  it('shows neither a status nor a copy button on a page without them', () => {
    const wrapper = page({})
    expect(wrapper.find('.page-end').exists()).toBe(false)
    expect(wrapper.find('.badge').exists()).toBe(false)
  })
})

describe('FullPage page tabs', () => {
  const at = async (path: string) => {
    const router = testRouter()
    await router.push(path)
    return mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] } })
  }

  it("lists the category's pages, marks the open one and names the row after the category", async () => {
    const wrapper = await at('/checkin')
    const nav = wrapper.get('nav.page-tabs')
    expect(nav.attributes('aria-label')).toBe('打卡')
    expect(nav.findAll('a').map((a) => [a.text(), a.attributes('aria-current')])).toEqual([
      ['打卡名單', 'page'],
      ['卡片樣式', undefined],
    ])
  })

  it('shows a page not built yet greyed, with no link and out of the tab order', async () => {
    const off = (await at('/redemptions')).get('nav.page-tabs .off')
    expect(off.element.tagName).toBe('SPAN')
    expect(off.attributes('aria-disabled')).toBe('true')
    expect(off.attributes('tabindex')).toBeUndefined()
    expect(off.text()).toContain('獎勵')
  })

  it('has no row for a page without a category, or when only one page of the category is left', async () => {
    expect((await at('/music')).find('nav.page-tabs').exists()).toBe(false)
    expect((await at('/songs')).find('nav.page-tabs').exists()).toBe(false)
    hiddenPages.set = new Set(['/card-style'])
    expect((await at('/checkin')).find('nav.page-tabs').exists()).toBe(false)
  })

  it('keeps the keyboard on the category or page tab that opened the page, else focuses "← Home"', async () => {
    const router = testRouter()
    await router.push('/card-style')
    navOpened.path = '/card-style'
    const viaTab = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    expect(document.activeElement?.textContent).toBe('卡片樣式')
    viaTab.unmount()
    const direct = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    expect(document.activeElement?.textContent).toContain('回首頁')
    direct.unmount()
    // A category link stays on the app bar, so it keeps focus.
    const category = document.body.appendChild(document.createElement('a'))
    category.href = '#'
    await router.push('/card-style')
    category.focus()
    navOpened.path = '/card-style'
    const viaCategory = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    expect(document.activeElement).toBe(category)
    viaCategory.unmount()
    category.remove()
  })
})
