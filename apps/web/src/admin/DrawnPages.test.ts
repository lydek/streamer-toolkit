// @vitest-environment jsdom
// Pages drawn ahead of their backend (UI-G): they render the prototype's layout empty, with backend actions disabled.
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { Component } from 'vue'
import { i18n } from '../i18n'
import StreamInfoPage from './StreamInfoPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

// PrimeVue's Select reads matchMedia, which jsdom lacks.
beforeAll(() => vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} })))

async function page(component: Component, path: string) {
  const router = testRouter([{ path, component }])
  await router.push(path)
  return mount({ template: '<RouterView />' }, { global: { plugins: [router, i18n, PrimeVue] } })
}

const button = (w: Awaited<ReturnType<typeof page>>, label: string) => w.findAll('button').find((b) => b.text() === label)!

describe('直播資訊', () => {
  it('shows the form with the update disabled and no templates', async () => {
    const w = await page(StreamInfoPage, '/stream-info')
    expect(w.get('h1').text()).toBe(t('pages.streamInfo.name'))
    expect(w.find('#si-title').exists()).toBe(true)
    expect(button(w, t('streamInfo.update')).attributes('disabled')).toBeDefined()
    expect(w.get('.empty-cell').text()).toBe(t('streamInfo.templates.empty'))
  })
})
