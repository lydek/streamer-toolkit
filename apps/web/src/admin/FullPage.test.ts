// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { i18n } from '../i18n'
import FullPage from './FullPage.vue'
import { servicesDown } from './home'
import { navOpened } from './page'
import { testRouter } from './testRouter'

const t = i18n.global.t

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
    expect(link.text()).toBe(t('obsStatus.offline'))
  })

  it("titles a page tab with the tab's own name and icon, and other pages with the title given", async () => {
    const router = testRouter()
    await router.push('/alert-events')
    const tab = mount(FullPage, { props: { title: '特效', tile: 'alerts' }, global: { plugins: [router, i18n, PrimeVue] } })
    expect(tab.get('h1').text()).toBe(t('pages.alertEvents.name'))
    expect(tab.get('.badge i').classes()).toContain('pi-list')
    expect(tab.get('.badge').classes()).toContain('tint-yellow')
    expect(tab.find('button').exists()).toBe(false)
    // A page without a home tile takes its own color.
    await router.push('/leaderboard')
    const board = mount(FullPage, { global: { plugins: [router, i18n, PrimeVue] } })
    expect(board.get('.badge').classes()).toContain('tint-yellow')
    await router.push('/settings')
    const other = mount(FullPage, { props: { title: '設定' }, global: { plugins: [router, i18n, PrimeVue] } })
    expect(other.get('h1').text()).toBe('設定')
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
    expect(nav.attributes('aria-label')).toBe(t('nav.categories.checkin'))
    expect(nav.findAll('a').map((a) => [a.text(), a.attributes('aria-current')])).toEqual([
      [t('nav.pageTabs.checkin'), 'page'],
      [t('pages.cardStyle.name'), undefined],
      [t('pages.checkinRules.name'), undefined],
    ])
  })

  it('shows a page not built yet greyed, with no link and out of the tab order', async () => {
    const off = (await at('/redemptions')).get('nav.page-tabs .off')
    expect(off.element.tagName).toBe('SPAN')
    expect(off.attributes('aria-disabled')).toBe('true')
    expect(off.attributes('tabindex')).toBeUndefined()
    expect(off.text()).toContain(t('nav.pageTabs.rewards'))
  })

  it('has no row for a page without a category, or when only one page of the category is left', async () => {
    expect((await at('/leaderboard')).find('nav.page-tabs').exists()).toBe(false)
    expect((await at('/songs')).find('nav.page-tabs').exists()).toBe(false)
    hiddenPages.set = new Set(['/card-style', '/checkin-rules'])
    expect((await at('/checkin')).find('nav.page-tabs').exists()).toBe(false)
  })

  it('keeps the keyboard on the category or page tab that opened the page, else focuses the title', async () => {
    const router = testRouter()
    await router.push('/card-style')
    navOpened.path = '/card-style'
    const viaTab = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    expect(document.activeElement?.textContent).toBe(t('pages.cardStyle.name'))
    viaTab.unmount()
    await router.push('/card-style')
    const direct = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    expect(document.activeElement?.tagName).toBe('H1')
    expect(document.activeElement?.textContent).toBe(t('pages.cardStyle.name'))
    direct.unmount()
    // The header's first button when it has some.
    const withActions = mount(FullPage, {
      slots: { actions: '<button type="button">隱藏</button><button type="button">匯入</button>' },
      global: { plugins: [router, i18n, PrimeVue] },
      attachTo: document.body,
    })
    expect(document.activeElement?.textContent).toBe('隱藏')
    expect(withActions.find('.page-end .actions').exists()).toBe(true)
    withActions.unmount()
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

describe('FullPage service notice', () => {
  afterEach(() => Object.assign(servicesDown, { twitch: false, obs: false }))
  const at = async (path: string) => {
    const router = testRouter()
    await router.push(path)
    const wrapper = mount(FullPage, { props: { title: 'x' }, global: { plugins: [router, i18n, PrimeVue] } })
    await flushPromises()
    return wrapper
  }

  it("warns on a page whose feature needs a service that is down, linking to that service's settings", async () => {
    servicesDown.obs = true
    const wrapper = await at('/scenes')
    const notice = wrapper.get('.service-warn')
    expect(notice.text()).toContain(t('services.banner', { service: 'OBS' }))
    expect(notice.get('a').attributes('href')).toBe('/settings/connections#obs')
    // Twitch down does not concern the scene page; one notice per service, never repeated.
    servicesDown.twitch = true
    await flushPromises()
    expect(wrapper.findAll('.service-warn')).toHaveLength(1)
  })

  it('stays quiet while the service is up, and on pages that do not need it', async () => {
    expect((await at('/scenes')).find('.service-warn').exists()).toBe(false)
    servicesDown.obs = true
    expect((await at('/counters')).find('.service-warn').exists()).toBe(false)
  })
})
