// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import HomeTile from './HomeTile.vue'
import { openerSelector } from './page'
import { testRouter } from './testRouter'

const tile = () =>
  mount(HomeTile, {
    props: { id: 'stream-info', name: '直播資訊', icon: 'pi pi-pencil', tint: 'muted', line: { state: 'loading' }, expanded: false, unbuilt: true },
    global: { plugins: [testRouter(), i18n] },
  })

afterEach(() => vi.unstubAllEnvs())

it('keeps a feature not built yet a grey block that goes nowhere', () => {
  const w = tile()
  expect(w.get('.tile.off').element.tagName).toBe('DIV')
  expect(w.find('a').exists()).toBe(false)
})

it('opens the page of a feature not built yet in dev mode', () => {
  vi.stubEnv('MODE', 'development')
  const link = tile().get('a.tile')
  expect(link.attributes('href')).toBe('/stream-info')
  // Back on home, focus returns to it (page.ts).
  expect(openerSelector(link.element)).toBe('[data-page-opener="stream-info"]')
})
