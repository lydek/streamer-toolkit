// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '../i18n'
import RedemptionsPage from './RedemptionsPage.vue'

const { client } = vi.hoisted(() => ({ client: { GET: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))
// Every GET here answers with the redemption list, so the page row's module states come from a stub.
vi.mock('./features', async (original) => ({
  ...(await original<typeof import('./features')>()),
  useModules: () => ({ hidden: computed(() => new Set<string>()) }),
}))

const ok = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })

async function mountPage() {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/redemptions', component: RedemptionsPage }] })
  await router.push('/redemptions')
  return mount(RedemptionsPage, { global: { plugins: [router, i18n, PrimeVue] } })
}

beforeEach(() => {
  vi.stubGlobal('WebSocket', class { close() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
})

describe('RedemptionsPage', () => {
  it('says how many redemptions could not be stored, which the list cannot show', async () => {
    client.GET.mockResolvedValue(ok({ items: [], total: 0, failed_writes: 0 }))
    let wrapper = await mountPage()
    await vi.waitFor(() => expect(wrapper.text()).toContain(i18n.global.t('redemption.empty')))
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    wrapper.unmount()

    client.GET.mockResolvedValue(ok({ items: [], total: 0, failed_writes: 2 }))
    wrapper = await mountPage()
    await vi.waitFor(() =>
      expect(wrapper.get('[role="alert"]').text()).toBe(i18n.global.t('redemption.writeFailed', { n: 2 })),
    )
  })
})
